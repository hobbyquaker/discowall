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

impl Packet {
    pub fn parse(bytes: &[u8]) -> Result<Self, hickory_proto::serialize::binary::DecodeError> {
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

    #[test]
    fn garbage_is_an_error() {
        assert!(Packet::parse(b"\x00\x01\x02").is_err());
        assert!(Packet::parse(&[]).is_err());
    }
}
