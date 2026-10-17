//! Parsing and classifying an mDNS packet (RFC 6762 on top of the RFC 1035 wire format), behind a
//! small surface so that hickory-proto can be swapped for an own parser later (research 3.3).
//!
//! What the rule chain needs from a packet: is it a query or a response, is it an announcement
//! (a response with the cache-flush bit) or a probe (a query carrying proposed records in the
//! authority section), and which service types, instances, hosts and addresses it names.

use hickory_proto::op::{Message, MessageType};
use hickory_proto::rr::{Name, RData, Record, RecordType};
use std::fmt;
use std::net::IpAddr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Query,
    /// A query whose authority section proposes records: the sender is about to claim a name.
    Probe,
    Response,
    /// A response with at least one cache-flush record: an unsolicited announcement.
    Announce,
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Kind::Query => "query",
            Kind::Probe => "probe",
            Kind::Response => "response",
            Kind::Announce => "announce",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    Answer,
    Authority,
    Additional,
}

/// One question of a query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    pub name: String,
    pub rtype: RecordType,
    /// The QU bit: the querier asks for a unicast response.
    pub unicast: bool,
}

/// What a record says, reduced to what rules and the host table look at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordInfo {
    pub section: Section,
    pub name: String,
    pub rtype: RecordType,
    pub ttl: u32,
    pub cache_flush: bool,
    pub data: Data,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Data {
    Addr(IpAddr),
    /// PTR target: an instance name for a service-type owner, a service type for the meta-query.
    Ptr(String),
    Srv {
        target: String,
        port: u16,
    },
    Txt,
    Other,
}

#[derive(Debug, Clone)]
pub struct Packet {
    pub kind: Kind,
    pub questions: Vec<Question>,
    pub records: Vec<RecordInfo>,
    /// The parsed message, kept for rebuilding after records were dropped.
    pub message: Message,
}

/// The human form of a name: labels as UTF-8 (DNS-SD instance names are UTF-8 with spaces and
/// punctuation, RFC 6763 §4.1.1), joined with dots, a dot inside a label escaped as `\.`, no
/// trailing dot, lowercased once here because DNS compares names case-insensitively.
fn plain(name: &Name) -> String {
    let mut out = String::new();
    for label in name.iter() {
        if !out.is_empty() {
            out.push('.');
        }
        for ch in String::from_utf8_lossy(label).chars() {
            if ch == '.' || ch == '\\' {
                out.push('\\');
            }
            out.push(ch);
        }
    }
    out.to_lowercase()
}

fn record_info(section: Section, r: &Record) -> RecordInfo {
    let data = match &r.data {
        RData::A(a) => Data::Addr(IpAddr::V4(a.0)),
        RData::AAAA(a) => Data::Addr(IpAddr::V6(a.0)),
        RData::PTR(p) => Data::Ptr(plain(&p.0)),
        RData::SRV(s) => Data::Srv {
            target: plain(&s.target),
            port: s.port,
        },
        RData::TXT(_) => Data::Txt,
        _ => Data::Other,
    };
    RecordInfo {
        section,
        name: plain(&r.name),
        rtype: r.record_type(),
        ttl: r.ttl,
        cache_flush: r.mdns_cache_flush,
        data,
    }
}

/// RFC 6763 §6.1: a TXT record with RDLENGTH 0 "is not strictly legal, but should one be received,
/// it should be interpreted as the same as a single empty string". hickory-proto (0.26.3) rejects
/// the whole message (`InvalidEmptyRecord`). This returns a copy in which every such TXT carries
/// one zero byte, with every compression pointer behind an insertion moved along, or `None` when
/// the packet has no empty TXT or cannot be walked (B-4). The copy is only parsed: what is
/// forwarded unchanged are the original bytes; a rebuilt packet carries the TXT as one zero byte,
/// as avahi's reflector sends it (avahi-common/strlst.c, read 2026-10-17).
pub fn normalize_empty_txt(bytes: &[u8]) -> Option<Vec<u8>> {
    // Offsets of compression pointers (the two bytes) and of the empty TXT records' RDLENGTH.
    let mut pointers: Vec<usize> = Vec::new();
    let mut empty_txt: Vec<usize> = Vec::new();

    // Walks a name at `off`; returns the offset behind it. Only the pointer positions matter here,
    // their targets are not followed (they point backwards into names already walked or not).
    fn walk_name(b: &[u8], mut off: usize, pointers: &mut Vec<usize>) -> Option<usize> {
        for _ in 0..128 {
            let len = *b.get(off)? as usize;
            match len & 0xC0 {
                0x00 if len == 0 => return Some(off + 1),
                0x00 => off += 1 + len,
                0xC0 => {
                    b.get(off + 1)?;
                    pointers.push(off);
                    return Some(off + 2);
                }
                _ => return None,
            }
        }
        None
    }
    fn u16_at(b: &[u8], off: usize) -> Option<u16> {
        Some(u16::from_be_bytes([*b.get(off)?, *b.get(off + 1)?]))
    }

    let qd = u16_at(bytes, 4)?;
    let records =
        u16_at(bytes, 6)? as usize + u16_at(bytes, 8)? as usize + u16_at(bytes, 10)? as usize;
    let mut off = 12;
    for _ in 0..qd {
        off = walk_name(bytes, off, &mut pointers)? + 4;
    }
    for _ in 0..records {
        off = walk_name(bytes, off, &mut pointers)?;
        let rtype = u16_at(bytes, off)?;
        let rdlen = u16_at(bytes, off + 8)? as usize;
        let rdata = off + 10;
        let end = rdata + rdlen;
        if end > bytes.len() {
            return None;
        }
        // Names inside the data that may be compressed (RFC 3597 §4 lists the types; NSEC is
        // there because mDNS responders compress it in practice).
        match rtype {
            2 | 5 | 12 | 47 => {
                walk_name(bytes, rdata, &mut pointers)?;
            }
            15 => {
                walk_name(bytes, rdata + 2, &mut pointers)?;
            }
            33 => {
                walk_name(bytes, rdata + 6, &mut pointers)?;
            }
            6 => {
                let next = walk_name(bytes, rdata, &mut pointers)?;
                walk_name(bytes, next, &mut pointers)?;
            }
            16 if rdlen == 0 => empty_txt.push(off + 8),
            _ => {}
        }
        off = end;
    }
    if empty_txt.is_empty() {
        return None;
    }
    // An insertion lands at the start of the (empty) RDATA, i.e. RDLENGTH + 2. A pointer whose
    // target is at or behind an insertion point moves by the number of those insertions.
    let inserts: Vec<usize> = empty_txt.iter().map(|o| o + 2).collect();
    let shift = |target: usize| inserts.iter().filter(|&&i| i <= target).count();
    let mut out = Vec::with_capacity(bytes.len() + inserts.len());
    let mut i = 0;
    while i < bytes.len() {
        if inserts.contains(&i) {
            out.push(0);
        }
        if empty_txt.contains(&i) {
            out.extend_from_slice(&1u16.to_be_bytes());
            i += 2;
            continue;
        }
        if pointers.contains(&i) {
            let target = (u16_at(bytes, i)? & 0x3FFF) as usize;
            let moved = target + shift(target);
            if moved > 0x3FFF {
                return None;
            }
            out.extend_from_slice(&(0xC000 | moved as u16).to_be_bytes());
            i += 2;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    // The last record's RDATA may end the packet: an insertion at the very end.
    if inserts.contains(&bytes.len()) {
        out.push(0);
    }
    Some(out)
}

/// A hand-built announcement like the one of B-4 (D-9: no captured data): PTR, then `txts` TXT
/// records with RDLENGTH 0, then SRV with target `pc.local`, then an A record whose owner is a
/// compression pointer to that target — behind the insertion point, so it must be moved.
#[cfg(test)]
pub(crate) fn empty_txt_announcement(txts: usize) -> Vec<u8> {
    fn label_name(out: &mut Vec<u8>, labels: &[&str]) {
        for l in labels {
            out.push(l.len() as u8);
            out.extend_from_slice(l.as_bytes());
        }
        out.push(0);
    }
    let mut b = vec![0, 0, 0x84, 0, 0, 0, 0, (2 + txts) as u8, 0, 0, 0, 1];
    // PTR _quest._tcp.local -> Quest._quest._tcp.local
    let svc = b.len();
    label_name(&mut b, &["_quest", "_tcp", "local"]);
    b.extend_from_slice(&[0, 12, 0, 1, 0, 0, 0x11, 0x94]);
    let rdlen_at = b.len();
    b.extend_from_slice(&[0, 0]);
    let inst = b.len();
    b.push(5);
    b.extend_from_slice(b"Quest");
    b.extend_from_slice(&(0xC000 | svc as u16).to_be_bytes());
    let rdlen = (b.len() - rdlen_at - 2) as u16;
    b[rdlen_at..rdlen_at + 2].copy_from_slice(&rdlen.to_be_bytes());
    // TXT, RDLENGTH 0, owner -> the instance name
    for _ in 0..txts {
        b.extend_from_slice(&(0xC000 | inst as u16).to_be_bytes());
        b.extend_from_slice(&[0, 16, 0x80, 1, 0, 0, 0x11, 0x94, 0, 0]);
    }
    // SRV 0 0 9000 pc.local, owner -> the instance name
    b.extend_from_slice(&(0xC000 | inst as u16).to_be_bytes());
    b.extend_from_slice(&[0, 33, 0x80, 1, 0, 0, 0, 120]);
    let rdlen_at = b.len();
    b.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0x23, 0x28]);
    let target = b.len();
    label_name(&mut b, &["pc", "local"]);
    let rdlen = (b.len() - rdlen_at - 2) as u16;
    b[rdlen_at..rdlen_at + 2].copy_from_slice(&rdlen.to_be_bytes());
    // A 10.2.0.9, owner -> pc.local in the SRV data
    b.extend_from_slice(&(0xC000 | target as u16).to_be_bytes());
    b.extend_from_slice(&[0, 1, 0x80, 1, 0, 0, 0, 120, 0, 4, 10, 2, 0, 9]);
    b
}

impl Packet {
    /// Parses a packet; one with an empty TXT record (RFC 6763 §6.1) is parsed from a normalised
    /// copy (B-4). The records keep their order, so verdicts per record apply to the original.
    pub fn parse(bytes: &[u8]) -> Result<Self, hickory_proto::serialize::binary::DecodeError> {
        match Self::parse_strict(bytes) {
            Ok(p) => Ok(p),
            Err(e) => match normalize_empty_txt(bytes) {
                Some(copy) => {
                    let p = Self::parse_strict(&copy).map_err(|_| e)?;
                    tracing::debug!("empty TXT record read as one empty string (RFC 6763 §6.1)");
                    Ok(p)
                }
                None => Err(e),
            },
        }
    }

    fn parse_strict(bytes: &[u8]) -> Result<Self, hickory_proto::serialize::binary::DecodeError> {
        let message = Message::from_vec(bytes)?;
        let questions = message
            .queries
            .iter()
            .map(|q| Question {
                name: plain(&q.name),
                rtype: q.query_type,
                unicast: q.mdns_unicast_response(),
            })
            .collect();
        let records: Vec<RecordInfo> = message
            .answers
            .iter()
            .map(|r| record_info(Section::Answer, r))
            .chain(
                message
                    .authorities
                    .iter()
                    .map(|r| record_info(Section::Authority, r)),
            )
            .chain(
                message
                    .additionals
                    .iter()
                    .map(|r| record_info(Section::Additional, r)),
            )
            .collect();
        let kind = match message.metadata.message_type {
            MessageType::Query if records.iter().any(|r| r.section == Section::Authority) => {
                Kind::Probe
            }
            MessageType::Query => Kind::Query,
            MessageType::Response if records.iter().any(|r| r.cache_flush) => Kind::Announce,
            MessageType::Response => Kind::Response,
        };
        Ok(Self {
            kind,
            questions,
            records,
            message,
        })
    }

    pub fn is_query(&self) -> bool {
        matches!(self.kind, Kind::Query | Kind::Probe)
    }

    /// Every service type the packet names (`_hap._tcp`), from questions and records alike.
    pub fn service_types(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut push = |s: Option<String>| {
            if let Some(s) = s {
                if !out.contains(&s) {
                    out.push(s);
                }
            }
        };
        for q in &self.questions {
            push(service_type_of(&q.name));
        }
        for r in &self.records {
            push(service_type_of(&r.name));
            if let Data::Ptr(target) = &r.data {
                push(service_type_of(target));
            }
        }
        out
    }

    /// Every service instance the packet names (`living room._hap._tcp.local`).
    pub fn instances(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut push = |name: &str| {
            if is_instance(name) && !out.iter().any(|o| o == name) {
                out.push(name.to_string());
            }
        };
        for q in &self.questions {
            push(&q.name);
        }
        for r in &self.records {
            push(&r.name);
            if let Data::Ptr(target) = &r.data {
                push(target);
            }
        }
        out
    }

    /// Every host name the packet names: owners of A/AAAA records, SRV targets, and plain `.local`
    /// questions (an A/AAAA lookup).
    pub fn hosts(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut push = |name: &str| {
            if is_host(name) && !out.iter().any(|o| o == name) {
                out.push(name.to_string());
            }
        };
        for q in &self.questions {
            if matches!(q.rtype, RecordType::A | RecordType::AAAA | RecordType::ANY) {
                push(&q.name);
            }
        }
        for r in &self.records {
            match &r.data {
                Data::Addr(_) => push(&r.name),
                Data::Srv { target, .. } => push(target),
                _ => {}
            }
        }
        out
    }

    /// The addresses the packet carries in A/AAAA records.
    pub fn addresses(&self) -> Vec<IpAddr> {
        self.records
            .iter()
            .filter_map(|r| match r.data {
                Data::Addr(a) => Some(a),
                _ => None,
            })
            .collect()
    }

    /// A one-line summary for logs and `discoctl watch`.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if !self.questions.is_empty() {
            parts.push(format!(
                "q[{}]",
                self.questions
                    .iter()
                    .map(|q| format!("{} {}", q.rtype, q.name))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !self.records.is_empty() {
            parts.push(format!(
                "rr[{}]",
                self.records
                    .iter()
                    .map(|r| format!("{} {}", r.rtype, r.name))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        format!("{} {}", self.kind, parts.join(" "))
    }
}

/// `_hap._tcp` for `_hap._tcp.local`, `living room._hap._tcp.local`, `_sub` variants
/// (`_printer._sub._http._tcp.local` → `_http._tcp`); `None` for host names and the meta-query.
pub fn service_type_of(name: &str) -> Option<String> {
    let labels: Vec<&str> = name.split('.').collect();
    // Find the protocol label from the right: `_tcp` or `_udp`, followed by the domain.
    let proto_at = labels.iter().rposition(|l| *l == "_tcp" || *l == "_udp")?;
    if proto_at == 0 {
        return None;
    }
    let service = labels[proto_at - 1];
    if !service.starts_with('_') || service == "_sub" || labels.contains(&"_services") {
        return None;
    }
    Some(format!("{service}.{}", labels[proto_at]))
}

/// A service instance: `<instance>.<_service>.<_proto>.<domain>` with at least one label before the
/// service, and that label not a `_sub` selector.
pub fn is_instance(name: &str) -> bool {
    let labels: Vec<&str> = name.split('.').collect();
    let Some(proto_at) = labels.iter().rposition(|l| *l == "_tcp" || *l == "_udp") else {
        return false;
    };
    proto_at >= 2
        && labels[proto_at - 1].starts_with('_')
        && labels[proto_at - 1] != "_services"
        && !labels[..proto_at - 1].iter().any(|l| l.starts_with('_'))
}

/// A host name: `<label>.local` (or any name without underscore labels), not a service name.
pub fn is_host(name: &str) -> bool {
    !name.is_empty() && !name.split('.').any(|l| l.starts_with('_'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hickory_proto::op::Query;
    use hickory_proto::rr::rdata::{A, AAAA, PTR, SRV, TXT};
    use hickory_proto::rr::DNSClass;

    /// Builds a name from its labels, raw: `from_utf8` refuses the space of an instance name.
    fn name(s: &str) -> Name {
        let labels: Vec<&[u8]> = s
            .trim_end_matches('.')
            .split('.')
            .map(str::as_bytes)
            .collect();
        Name::from_labels(labels).unwrap()
    }

    fn rr(owner: &str, ttl: u32, data: RData, flush: bool) -> Record {
        let mut r = Record::from_rdata(name(owner), ttl, data);
        r.dns_class = DNSClass::IN;
        r.mdns_cache_flush = flush;
        r
    }

    fn browse_query(qu: bool) -> Vec<u8> {
        let mut m = Message::new(0, MessageType::Query, hickory_proto::op::OpCode::Query);
        let mut q = Query::query(name("_hap._tcp.local."), RecordType::PTR);
        q.set_mdns_unicast_response(qu);
        m.add_query(q);
        m.to_vec().unwrap()
    }

    fn announcement() -> Vec<u8> {
        let mut m = Message::response(0, hickory_proto::op::OpCode::Query);
        m.add_answer(rr(
            "_hap._tcp.local.",
            4500,
            RData::PTR(PTR(name("Living Room._hap._tcp.local."))),
            false,
        ));
        m.add_answer(rr(
            "Living Room._hap._tcp.local.",
            120,
            RData::SRV(SRV::new(0, 0, 51826, name("bridge.local."))),
            true,
        ));
        m.add_answer(rr(
            "Living Room._hap._tcp.local.",
            4500,
            RData::TXT(TXT::new(vec!["md=Bridge".into()])),
            true,
        ));
        m.add_additional(rr(
            "bridge.local.",
            120,
            RData::A(A("192.168.30.17".parse().unwrap())),
            true,
        ));
        m.add_additional(rr(
            "bridge.local.",
            120,
            RData::AAAA(AAAA("fe80::1".parse().unwrap())),
            true,
        ));
        m.to_vec().unwrap()
    }

    #[test]
    fn empty_txt_is_read_as_one_empty_string() {
        let raw = empty_txt_announcement(1);
        assert!(Message::from_vec(&raw).is_err(), "hickory rejects it (B-4)");
        let p = Packet::parse(&raw).unwrap();
        assert_eq!(p.kind, Kind::Announce);
        let types: Vec<_> = p.records.iter().map(|r| r.rtype).collect();
        assert_eq!(
            types,
            [
                RecordType::PTR,
                RecordType::TXT,
                RecordType::SRV,
                RecordType::A
            ]
        );
        assert_eq!(p.records[1].name, "quest._quest._tcp.local"); // names are compared lowercased
                                                                  // The A record's owner was a pointer behind the insertion: still pc.local after the shift.
        assert_eq!(p.records[3].name, "pc.local");
        assert_eq!(p.records[3].data, Data::Addr("10.2.0.9".parse().unwrap()));
        assert_eq!(p.hosts(), vec!["pc.local".to_string()]);
    }

    #[test]
    fn two_empty_txts_and_none() {
        let p = Packet::parse(&empty_txt_announcement(2)).unwrap();
        assert_eq!(p.records.len(), 5);
        assert_eq!(p.records[4].name, "pc.local");
        // Nothing to normalise in a well-formed packet.
        assert!(normalize_empty_txt(&announcement()).is_none());
    }

    #[test]
    fn an_empty_record_of_another_type_stays_unparsable() {
        // The A record of the announcement with RDLENGTH 0: garbage, not normalised.
        let mut raw = empty_txt_announcement(0);
        let n = raw.len();
        raw.truncate(n - 6);
        raw.extend_from_slice(&[0, 0]);
        let e = Packet::parse(&raw).unwrap_err();
        // The reason `discoctl watch` and the log show (task 17), hickory-proto 0.26.3's wording.
        assert!(e.to_string().contains("length 0"), "{e}");
    }

    #[test]
    fn classifies_a_browse_query() {
        let p = Packet::parse(&browse_query(true)).unwrap();
        assert_eq!(p.kind, Kind::Query);
        assert!(p.is_query());
        assert_eq!(
            p.questions,
            vec![Question {
                name: "_hap._tcp.local".into(),
                rtype: RecordType::PTR,
                unicast: true
            }]
        );
        assert_eq!(p.service_types(), vec!["_hap._tcp"]);
        assert!(p.instances().is_empty());
        assert!(p.hosts().is_empty());
        assert_eq!(p.summary(), "query q[PTR _hap._tcp.local]");
    }

    #[test]
    fn classifies_an_announcement() {
        let p = Packet::parse(&announcement()).unwrap();
        assert_eq!(p.kind, Kind::Announce);
        assert_eq!(p.records.len(), 5);
        assert_eq!(p.service_types(), vec!["_hap._tcp"]);
        assert_eq!(p.instances(), vec!["living room._hap._tcp.local"]);
        assert_eq!(p.hosts(), vec!["bridge.local"]);
        assert_eq!(
            p.addresses(),
            vec![
                "192.168.30.17".parse::<IpAddr>().unwrap(),
                "fe80::1".parse::<IpAddr>().unwrap()
            ]
        );
        let srv = p
            .records
            .iter()
            .find(|r| r.rtype == RecordType::SRV)
            .unwrap();
        assert_eq!(
            srv.data,
            Data::Srv {
                target: "bridge.local".into(),
                port: 51826
            }
        );
        assert!(srv.cache_flush);
        assert_eq!(p.records[0].section, Section::Answer);
        assert_eq!(p.records[3].section, Section::Additional);
    }

    #[test]
    fn response_without_cache_flush_is_a_plain_response() {
        let mut m = Message::response(0, hickory_proto::op::OpCode::Query);
        m.add_answer(rr(
            "_hap._tcp.local.",
            4500,
            RData::PTR(PTR(name("Living Room._hap._tcp.local."))),
            false,
        ));
        let p = Packet::parse(&m.to_vec().unwrap()).unwrap();
        assert_eq!(p.kind, Kind::Response);
    }

    #[test]
    fn probe_has_authority_records() {
        let mut m = Message::new(0, MessageType::Query, hickory_proto::op::OpCode::Query);
        m.add_query(Query::query(name("bridge.local."), RecordType::ANY));
        m.add_authority(rr(
            "bridge.local.",
            120,
            RData::A(A("192.168.30.17".parse().unwrap())),
            false,
        ));
        let p = Packet::parse(&m.to_vec().unwrap()).unwrap();
        assert_eq!(p.kind, Kind::Probe);
        assert_eq!(p.hosts(), vec!["bridge.local"]);
    }

    #[test]
    fn plain_keeps_utf8_and_escapes_inner_dots() {
        let n =
            Name::from_labels(vec!["Küche 1.OG".as_bytes(), b"_hap", b"_tcp", b"local"]).unwrap();
        assert_eq!(plain(&n), "küche 1\\.og._hap._tcp.local");
        assert_eq!(plain(&Name::root()), "");
    }

    #[test]
    fn name_helpers() {
        assert_eq!(
            service_type_of("_hap._tcp.local").as_deref(),
            Some("_hap._tcp")
        );
        assert_eq!(
            service_type_of("living room._hap._tcp.local").as_deref(),
            Some("_hap._tcp")
        );
        assert_eq!(
            service_type_of("_printer._sub._http._tcp.local").as_deref(),
            Some("_http._tcp")
        );
        assert_eq!(service_type_of("_services._dns-sd._udp.local"), None);
        assert_eq!(service_type_of("bridge.local"), None);
        assert!(is_instance("living room._hap._tcp.local"));
        assert!(!is_instance("_hap._tcp.local"));
        assert!(!is_instance("_printer._sub._http._tcp.local"));
        assert!(!is_instance("_services._dns-sd._udp.local"));
        assert!(is_host("bridge.local"));
        assert!(!is_host("_hap._tcp.local"));
        assert!(!is_host(""));
    }

    /// Not a fuzzer, but the same idea without nightly: random bytes and mutated real packets must
    /// never panic in parse, classification or the rebuild; the parser's contract is "Err or a Packet".
    #[test]
    fn random_and_mutated_input_never_panics() {
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let seeds = [announcement(), browse_query(true)];
        for i in 0..20_000 {
            let bytes: Vec<u8> = if i % 2 == 0 {
                let len = (next() % 300) as usize;
                (0..len).map(|_| next() as u8).collect()
            } else {
                let mut b = seeds[(i / 2) % seeds.len()].clone();
                for _ in 0..1 + next() % 4 {
                    let at = (next() as usize) % b.len();
                    b[at] = next() as u8;
                }
                if next() % 8 == 0 {
                    b.truncate((next() as usize) % b.len());
                }
                b
            };
            if let Ok(p) = Packet::parse(&bytes) {
                let _ = (
                    p.service_types(),
                    p.instances(),
                    p.hosts(),
                    p.addresses(),
                    p.summary(),
                );
                let _ = p.message.to_vec();
            }
        }
    }

    #[test]
    fn garbage_is_an_error() {
        assert!(Packet::parse(b"\x00\x01\x02").is_err());
        assert!(Packet::parse(&[]).is_err());
    }
}
