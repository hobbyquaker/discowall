//! The host table (research 3.5): what every interface has announced, learned passively from every
//! packet whether forwarded or not. It ties the records of a service together — instance → service
//! type and host (from the SRV), host → addresses — so that a rule on a host or a service can be
//! applied to an A record that names only the host, and it is what `discoctl hosts` shows.

use crate::packet::{self, Data, Packet, Section};
use discowall_proto::{HostView, InstanceView, ServiceView};
use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
struct Instance {
    service: String,
    host: Option<String>,
    port: Option<u16>,
    expires: Instant,
}

#[derive(Debug, Clone, Default)]
struct Host {
    addrs: BTreeMap<IpAddr, Instant>,
    instances: BTreeSet<String>,
    last_seen: Option<Instant>,
}

#[derive(Debug, Default)]
struct Segment {
    instances: BTreeMap<String, Instance>,
    hosts: BTreeMap<String, Host>,
}

/// Per in-interface knowledge.
#[derive(Debug, Default)]
pub struct HostTable {
    segments: BTreeMap<String, Segment>,
}

/// What the table attributes to one record: the bundle it belongs to.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Bundle {
    pub services: Vec<String>,
    pub instances: Vec<String>,
    pub hosts: Vec<String>,
}

fn push(v: &mut Vec<String>, s: &str) {
    if !v.iter().any(|x| x == s) {
        v.push(s.to_string());
    }
}

/// Goodbye packets carry TTL 0; a positive TTL is honoured as the record says.
fn expiry(now: Instant, ttl: u32) -> Instant {
    now + Duration::from_secs(ttl as u64)
}

impl HostTable {
    pub fn learn(&mut self, iface: &str, pkt: &Packet, now: Instant) {
        let seg = self.segments.entry(iface.to_string()).or_default();
        for r in &pkt.records {
            // Probes propose records in the authority section; a proposed name is as good as an
            // announced one for attribution (and it expires like any other).
            if pkt.is_query() && r.section != Section::Authority {
                continue;
            }
            match &r.data {
                Data::Ptr(target) => {
                    if let Some(service) = packet::service_type_of(&r.name) {
                        if packet::is_instance(target) {
                            let e = seg.instances.entry(target.clone()).or_insert(Instance {
                                service: service.clone(),
                                host: None,
                                port: None,
                                expires: expiry(now, r.ttl),
                            });
                            e.service = service;
                            e.expires = expiry(now, r.ttl);
                        }
                    }
                }
                Data::Srv { target, port } => {
                    if let Some(service) = packet::service_type_of(&r.name) {
                        let e = seg.instances.entry(r.name.clone()).or_insert(Instance {
                            service: service.clone(),
                            host: None,
                            port: None,
                            expires: expiry(now, r.ttl),
                        });
                        e.service = service;
                        e.host = Some(target.clone());
                        e.port = Some(*port);
                        e.expires = expiry(now, r.ttl);
                        let h = seg.hosts.entry(target.clone()).or_default();
                        h.instances.insert(r.name.clone());
                        h.last_seen = Some(now);
                    }
                }
                Data::Txt => {
                    if let Some(service) = packet::service_type_of(&r.name) {
                        let e = seg.instances.entry(r.name.clone()).or_insert(Instance {
                            service,
                            host: None,
                            port: None,
                            expires: expiry(now, r.ttl),
                        });
                        e.expires = e.expires.max(expiry(now, r.ttl));
                    }
                }
                Data::Addr(a) => {
                    if packet::is_host(&r.name) {
                        let h = seg.hosts.entry(r.name.clone()).or_default();
                        h.addrs.insert(*a, expiry(now, r.ttl));
                        h.last_seen = Some(now);
                    }
                }
                Data::Other => {}
            }
        }
    }

    /// An SSDP notification or response: the uuid is the instance, the NT/ST its service, the
    /// LOCATION's host its host. A byebye expires it at once.
    pub fn learn_ssdp(&mut self, iface: &str, msg: &crate::ssdp::SsdpMessage, now: Instant) {
        use crate::ssdp::SsdpKind;
        let (Some(uuid), Some(target)) = (&msg.uuid, &msg.target) else {
            return;
        };
        if msg.kind == SsdpKind::Search {
            return;
        }
        let seg = self.segments.entry(iface.to_string()).or_default();
        let ttl = if msg.kind == SsdpKind::Byebye {
            0
        } else {
            msg.max_age.unwrap_or(1800)
        };
        let e = seg.instances.entry(uuid.clone()).or_insert(Instance {
            service: target.clone(),
            host: None,
            port: None,
            expires: expiry(now, ttl),
        });
        // A device announces several types for one uuid; the root device and the specific type
        // both name it. Keep the most specific (a urn) over upnp:rootdevice / the bare uuid.
        if target.starts_with("urn:") || !e.service.starts_with("urn:") {
            e.service = target.clone();
        }
        e.expires = expiry(now, ttl);
        if let Some(host) = &msg.location_host {
            e.host = Some(host.clone());
            e.port = msg.location_port;
            let h = seg.hosts.entry(host.clone()).or_default();
            h.instances.insert(uuid.clone());
            h.last_seen = Some(now);
            if let Some(ip) = msg.location_ip() {
                h.addrs.insert(ip, expiry(now, ttl));
            }
        }
    }

    /// Drops what has expired. Cheap enough to run every few seconds.
    pub fn expire(&mut self, now: Instant) {
        for seg in self.segments.values_mut() {
            seg.instances.retain(|_, i| i.expires > now);
            for h in seg.hosts.values_mut() {
                h.addrs.retain(|_, e| *e > now);
                h.instances.retain(|i| seg.instances.contains_key(i));
            }
            seg.hosts
                .retain(|_, h| !h.addrs.is_empty() || !h.instances.is_empty());
        }
    }

    /// The bundle of a name as the segment knows it: for an instance its service and host, for a
    /// host its instances and their services, for a service type itself.
    pub fn bundle_of(&self, iface: &str, name: &str) -> Bundle {
        let mut b = Bundle::default();
        let seg = self.segments.get(iface);
        if let Some(st) = packet::service_type_of(name) {
            push(&mut b.services, &st);
        }
        if packet::is_instance(name) || name.starts_with("uuid:") {
            push(&mut b.instances, name);
            if let Some(i) = seg.and_then(|s| s.instances.get(name)) {
                push(&mut b.services, &i.service);
                if let Some(h) = &i.host {
                    push(&mut b.hosts, h);
                }
            }
        } else if packet::is_host(name) {
            push(&mut b.hosts, name);
            if let Some(h) = seg.and_then(|s| s.hosts.get(name)) {
                for inst in &h.instances {
                    push(&mut b.instances, inst);
                    if let Some(i) = seg.and_then(|s| s.instances.get(inst)) {
                        push(&mut b.services, &i.service);
                    }
                }
            }
        }
        b
    }

    pub fn services(&self, now: Instant) -> Vec<ServiceView> {
        let mut out = Vec::new();
        for (iface, seg) in &self.segments {
            for (name, i) in &seg.instances {
                out.push(ServiceView {
                    iface: iface.clone(),
                    service: i.service.clone(),
                    instance: name.clone(),
                    host: i.host.clone(),
                    port: i.port,
                    expires_in_s: i.expires.saturating_duration_since(now).as_secs(),
                });
            }
        }
        out.sort_by(|a, b| {
            (&a.iface, &a.service, &a.instance).cmp(&(&b.iface, &b.service, &b.instance))
        });
        out
    }

    /// (hosts, instances) over all segments.
    pub fn counts(&self) -> (usize, usize) {
        self.segments.values().fold((0, 0), |(h, i), s| {
            (h + s.hosts.len(), i + s.instances.len())
        })
    }

    pub fn view(&self, now: Instant) -> Vec<HostView> {
        let mut out = Vec::new();
        for (iface, seg) in &self.segments {
            for (host, h) in &seg.hosts {
                let instances = h
                    .instances
                    .iter()
                    .filter_map(|name| {
                        seg.instances.get(name).map(|i| InstanceView {
                            instance: name.clone(),
                            service: i.service.clone(),
                            port: i.port,
                        })
                    })
                    .collect();
                let expires = h
                    .addrs
                    .values()
                    .copied()
                    .chain(
                        h.instances
                            .iter()
                            .filter_map(|i| seg.instances.get(i))
                            .map(|i| i.expires),
                    )
                    .max()
                    .unwrap_or(now);
                out.push(HostView {
                    iface: iface.clone(),
                    host: host.clone(),
                    addresses: h.addrs.keys().copied().collect(),
                    instances,
                    last_seen_s: h
                        .last_seen
                        .map(|t| now.duration_since(t).as_secs())
                        .unwrap_or(0),
                    expires_in_s: expires.saturating_duration_since(now).as_secs(),
                });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::{Kind, Question, RecordInfo};
    use hickory_proto::op::{Message, MessageType, OpCode};
    use hickory_proto::rr::RecordType;

    fn pkt(kind: Kind, records: Vec<RecordInfo>) -> Packet {
        Packet {
            kind,
            questions: vec![],
            records,
            message: Message::new(0, MessageType::Response, OpCode::Query),
        }
    }

    fn rec(section: Section, name: &str, rtype: RecordType, ttl: u32, data: Data) -> RecordInfo {
        RecordInfo {
            section,
            name: name.into(),
            rtype,
            ttl,
            cache_flush: true,
            data,
        }
    }

    fn announcement(ttl: u32) -> Packet {
        pkt(
            Kind::Announce,
            vec![
                rec(
                    Section::Answer,
                    "_hap._tcp.local",
                    RecordType::PTR,
                    ttl,
                    Data::Ptr("living room._hap._tcp.local".into()),
                ),
                rec(
                    Section::Answer,
                    "living room._hap._tcp.local",
                    RecordType::SRV,
                    ttl,
                    Data::Srv {
                        target: "bridge.local".into(),
                        port: 51826,
                    },
                ),
                rec(
                    Section::Answer,
                    "living room._hap._tcp.local",
                    RecordType::TXT,
                    ttl,
                    Data::Txt,
                ),
                rec(
                    Section::Additional,
                    "bridge.local",
                    RecordType::A,
                    ttl,
                    Data::Addr("192.168.30.17".parse().unwrap()),
                ),
            ],
        )
    }

    #[test]
    fn learns_a_bundle_and_attributes_an_address_record() {
        let mut t = HostTable::default();
        let now = Instant::now();
        t.learn("iot", &announcement(120), now);
        let b = t.bundle_of("iot", "bridge.local");
        assert_eq!(
            b,
            Bundle {
                services: vec!["_hap._tcp".into()],
                instances: vec!["living room._hap._tcp.local".into()],
                hosts: vec!["bridge.local".into()]
            }
        );
        let b = t.bundle_of("iot", "living room._hap._tcp.local");
        assert_eq!(b.hosts, vec!["bridge.local"]);
        assert_eq!(b.services, vec!["_hap._tcp"]);
        // Another segment knows nothing.
        assert_eq!(
            t.bundle_of("lan", "bridge.local").instances,
            Vec::<String>::new()
        );
        let view = t.view(now);
        assert_eq!(view.len(), 1);
        assert_eq!(view[0].host, "bridge.local");
        assert_eq!(view[0].instances[0].port, Some(51826));
        assert_eq!(view[0].expires_in_s, 120);
    }

    #[test]
    fn expires_and_goodbye() {
        let mut t = HostTable::default();
        let now = Instant::now();
        t.learn("iot", &announcement(10), now);
        t.expire(now + Duration::from_secs(5));
        assert_eq!(t.view(now).len(), 1);
        t.expire(now + Duration::from_secs(11));
        assert!(t.view(now).is_empty());
        t.learn("iot", &announcement(120), now);
        // A goodbye: TTL 0 on everything.
        t.learn("iot", &announcement(0), now + Duration::from_secs(1));
        t.expire(now + Duration::from_secs(2));
        assert!(t.view(now).is_empty());
    }

    #[test]
    fn ssdp_notifications_join_the_table() {
        let mut t = HostTable::default();
        let now = Instant::now();
        let alive = crate::ssdp::SsdpMessage::parse(b"NOTIFY * HTTP/1.1\r\nNTS: ssdp:alive\r\nNT: upnp:rootdevice\r\nUSN: uuid:abc::upnp:rootdevice\r\nLOCATION: http://10.0.0.40:1400/xml\r\nCACHE-CONTROL: max-age=60\r\n\r\n").unwrap();
        t.learn_ssdp("iot", &alive, now);
        let specific = crate::ssdp::SsdpMessage::parse(b"NOTIFY * HTTP/1.1\r\nNTS: ssdp:alive\r\nNT: urn:schemas-upnp-org:device:MediaRenderer:1\r\nUSN: uuid:abc::urn:schemas-upnp-org:device:MediaRenderer:1\r\nLOCATION: http://10.0.0.40:1400/xml\r\nCACHE-CONTROL: max-age=60\r\n\r\n").unwrap();
        t.learn_ssdp("iot", &specific, now);
        let b = t.bundle_of("iot", "uuid:abc");
        assert_eq!(
            b.services,
            vec!["urn:schemas-upnp-org:device:mediarenderer:1"]
        );
        assert_eq!(b.hosts, vec!["10.0.0.40"]);
        let b = t.bundle_of("iot", "10.0.0.40");
        assert_eq!(b.instances, vec!["uuid:abc"]);
        let v = t.view(now);
        assert_eq!(v[0].addresses, vec!["10.0.0.40".parse::<IpAddr>().unwrap()]);
        assert_eq!(v[0].instances[0].port, Some(1400));
        let bye = crate::ssdp::SsdpMessage::parse(b"NOTIFY * HTTP/1.1\r\nNTS: ssdp:byebye\r\nNT: upnp:rootdevice\r\nUSN: uuid:abc::upnp:rootdevice\r\n\r\n").unwrap();
        t.learn_ssdp("iot", &bye, now + Duration::from_secs(1));
        t.expire(now + Duration::from_secs(2));
        assert!(t.services(now).is_empty());
    }

    #[test]
    fn plain_queries_teach_nothing_but_probes_do() {
        let mut t = HostTable::default();
        let now = Instant::now();
        let mut q = pkt(Kind::Query, vec![]);
        q.questions.push(Question {
            name: "bridge.local".into(),
            rtype: RecordType::A,
            unicast: false,
        });
        t.learn("iot", &q, now);
        assert!(t.view(now).is_empty());
        let probe = pkt(
            Kind::Probe,
            vec![rec(
                Section::Authority,
                "bridge.local",
                RecordType::A,
                120,
                Data::Addr("192.168.30.17".parse().unwrap()),
            )],
        );
        t.learn("iot", &probe, now);
        assert_eq!(t.view(now)[0].addresses.len(), 1);
    }
}
