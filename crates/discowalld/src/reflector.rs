//! The core: interface state, the loop checks, and (in P0) "forward everything between the
//! configured interfaces". The rule chain of P1 slots in where `forward` decides.

use crate::config::Config;
use crate::ifaces::IfaceEvent;
use crate::socket::{Family, MdnsSocket, Received};
use discowall_proto::{Counters, InterfaceStatus, Status};
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

#[derive(Debug, Default, Clone)]
struct Iface {
    index: Option<u32>,
    up: bool,
    joined: HashSet<Family>,
    addrs: Vec<IpAddr>,
    rx: u64,
    tx: u64,
}

pub struct Reflector {
    config_path: String,
    started: Instant,
    /// Configured interfaces in configuration order.
    names: Vec<String>,
    ifaces: HashMap<String, Iface>,
    index_to_name: HashMap<u32, String>,
    /// Every address of the host, on any interface: packets from these are our own reflections.
    own_addrs: HashMap<IpAddr, u32>,
    sockets: Vec<Arc<dyn MdnsSocket>>,
    dedup: HashMap<u64, Instant>,
    dedup_window: Duration,
    dedup_sweep: Instant,
    pub counters: Counters,
}

impl Reflector {
    pub fn new(config: &Config, config_path: String, sockets: Vec<Arc<dyn MdnsSocket>>) -> Self {
        let ifaces = config
            .interfaces
            .iter()
            .map(|n| (n.clone(), Iface::default()))
            .collect();
        Self {
            config_path,
            started: Instant::now(),
            names: config.interfaces.clone(),
            ifaces,
            index_to_name: HashMap::new(),
            own_addrs: HashMap::new(),
            sockets,
            dedup: HashMap::new(),
            dedup_window: Duration::from_millis(config.dedup_window_ms),
            dedup_sweep: Instant::now(),
            counters: Counters::default(),
        }
    }

    fn socket(&self, family: Family) -> Option<&Arc<dyn MdnsSocket>> {
        self.sockets.iter().find(|s| s.family() == family)
    }

    pub fn on_iface_event(&mut self, ev: IfaceEvent) {
        match ev {
            IfaceEvent::Link { index, name, up } => {
                if !self.ifaces.contains_key(&name) {
                    return;
                }
                self.index_to_name.insert(index, name.clone());
                let iface = self.ifaces.get_mut(&name).expect("configured");
                let was_up = iface.up && iface.index == Some(index);
                iface.index = Some(index);
                iface.up = up;
                if up && !was_up {
                    info!(iface = %name, index, "interface up, joining");
                    self.join(&name);
                } else if !up && was_up {
                    info!(iface = %name, index, "interface down, leaving");
                    self.leave(&name);
                }
            }
            IfaceEvent::LinkGone { index } => {
                if let Some(name) = self.index_to_name.remove(&index) {
                    info!(iface = %name, index, "interface gone");
                    let iface = self.ifaces.get_mut(&name).expect("configured");
                    iface.index = None;
                    iface.up = false;
                    iface.joined.clear();
                }
                self.own_addrs.retain(|_, i| *i != index);
            }
            IfaceEvent::AddrAdded { index, addr } => {
                self.own_addrs.insert(addr, index);
                if let Some(name) = self.index_to_name.get(&index) {
                    let iface = self.ifaces.get_mut(name).expect("configured");
                    if !iface.addrs.contains(&addr) {
                        iface.addrs.push(addr);
                    }
                }
            }
            IfaceEvent::AddrRemoved { index, addr } => {
                self.own_addrs.remove(&addr);
                if let Some(name) = self.index_to_name.get(&index) {
                    self.ifaces
                        .get_mut(name)
                        .expect("configured")
                        .addrs
                        .retain(|a| *a != addr);
                }
            }
            IfaceEvent::Synced => {
                let missing: Vec<&String> = self
                    .names
                    .iter()
                    .filter(|n| self.ifaces[*n].index.is_none())
                    .collect();
                if !missing.is_empty() {
                    warn!(
                        ?missing,
                        "configured interfaces that do not exist yet; waiting for them"
                    );
                }
            }
        }
    }

    fn join(&mut self, name: &str) {
        let Some(index) = self.ifaces[name].index else {
            return;
        };
        for s in &self.sockets {
            match s.join(index) {
                Ok(()) => {
                    self.ifaces
                        .get_mut(name)
                        .expect("configured")
                        .joined
                        .insert(s.family());
                }
                Err(e) => warn!(iface = name, family = ?s.family(), "join failed: {e:#}"),
            }
        }
    }

    fn leave(&mut self, name: &str) {
        let Some(index) = self.ifaces[name].index else {
            return;
        };
        for s in &self.sockets {
            // After a link went down the kernel has dropped the membership already; an error here
            // is expected and not worth a warning.
            if let Err(e) = s.leave(index) {
                debug!(iface = name, family = ?s.family(), "leave: {e:#}");
            }
        }
        self.ifaces
            .get_mut(name)
            .expect("configured")
            .joined
            .clear();
    }

    fn is_duplicate(&mut self, data: &[u8]) -> bool {
        let now = Instant::now();
        if now.duration_since(self.dedup_sweep) > self.dedup_window * 4 {
            let window = self.dedup_window;
            self.dedup.retain(|_, t| now.duration_since(*t) <= window);
            self.dedup_sweep = now;
        }
        let mut h = DefaultHasher::new();
        data.hash(&mut h);
        let key = h.finish();
        match self.dedup.get(&key) {
            Some(t) if now.duration_since(*t) <= self.dedup_window => true,
            _ => {
                self.dedup.insert(key, now);
                false
            }
        }
    }

    /// One packet in. Returns the names of the interfaces it was sent to.
    pub fn on_packet(&mut self, pkt: Received) -> Vec<String> {
        self.counters.rx += 1;
        let Some(in_name) = self.index_to_name.get(&pkt.ifindex).cloned() else {
            self.counters.dropped_unknown_interface += 1;
            return vec![];
        };
        self.ifaces.get_mut(&in_name).expect("configured").rx += 1;
        if self.own_addrs.contains_key(&pkt.src.ip()) {
            self.counters.dropped_own_source += 1;
            return vec![];
        }
        if self.is_duplicate(&pkt.data) {
            self.counters.dropped_duplicate += 1;
            debug!(iface = %in_name, src = %pkt.src, "duplicate dropped");
            return vec![];
        }
        let Some(socket) = self.socket(pkt.family).cloned() else {
            return vec![];
        };
        let targets: Vec<(String, u32)> = self
            .names
            .iter()
            .filter(|n| **n != in_name)
            .filter_map(|n| {
                let i = &self.ifaces[n];
                (i.up && i.joined.contains(&pkt.family))
                    .then(|| (n.clone(), i.index.expect("up implies index")))
            })
            .collect();
        let mut sent = Vec::with_capacity(targets.len());
        for (name, index) in targets {
            match socket.send_multicast(&pkt.data, index) {
                Ok(()) => {
                    self.counters.tx += 1;
                    self.ifaces.get_mut(&name).expect("configured").tx += 1;
                    sent.push(name);
                }
                Err(e) => {
                    self.counters.send_errors += 1;
                    warn!(iface = %name, "send failed: {e:#}");
                }
            }
        }
        if tracing::enabled!(tracing::Level::DEBUG) {
            let what = match crate::packet::Packet::parse(&pkt.data) {
                Ok(p) => p.summary(),
                Err(e) => format!("unparsable ({e})"),
            };
            debug!(iface = %in_name, src = %pkt.src, len = pkt.data.len(), to = ?sent, "forwarded {what}");
        }
        sent
    }

    pub fn status(&self) -> Status {
        Status {
            version: env!("CARGO_PKG_VERSION").to_string(),
            uptime_s: self.started.elapsed().as_secs(),
            config_path: self.config_path.clone(),
            interfaces: self
                .names
                .iter()
                .map(|n| {
                    let i = &self.ifaces[n];
                    InterfaceStatus {
                        name: n.clone(),
                        index: i.index,
                        up: i.up,
                        joined_v4: i.joined.contains(&Family::V4),
                        joined_v6: i.joined.contains(&Family::V6),
                        addresses: i.addrs.clone(),
                        rx: i.rx,
                        tx: i.tx,
                    }
                })
                .collect(),
            counters: self.counters.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;
    use std::sync::Mutex;

    /// Records sends; never receives.
    struct FakeSocket {
        family: Family,
        sent: Mutex<Vec<(Vec<u8>, u32)>>,
        joined: Mutex<Vec<u32>>,
    }

    impl FakeSocket {
        fn new(family: Family) -> Arc<Self> {
            Arc::new(Self {
                family,
                sent: Mutex::new(vec![]),
                joined: Mutex::new(vec![]),
            })
        }
    }

    impl MdnsSocket for FakeSocket {
        fn family(&self) -> Family {
            self.family
        }
        fn recv(&self) -> anyhow::Result<Received> {
            unreachable!()
        }
        fn send_multicast(&self, data: &[u8], ifindex: u32) -> anyhow::Result<()> {
            self.sent.lock().unwrap().push((data.to_vec(), ifindex));
            Ok(())
        }
        fn join(&self, ifindex: u32) -> anyhow::Result<()> {
            self.joined.lock().unwrap().push(ifindex);
            Ok(())
        }
        fn leave(&self, ifindex: u32) -> anyhow::Result<()> {
            self.joined.lock().unwrap().retain(|i| *i != ifindex);
            Ok(())
        }
    }

    fn config(names: &[&str]) -> Config {
        toml::from_str(&format!(
            "interfaces = [{}]\ndedup_window_ms = 200",
            names
                .iter()
                .map(|n| format!("{n:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        ))
        .unwrap()
    }

    fn setup() -> (Reflector, Arc<FakeSocket>) {
        let sock = FakeSocket::new(Family::V4);
        let mut r = Reflector::new(
            &config(&["lan", "iot", "guest"]),
            "test".into(),
            vec![sock.clone()],
        );
        for (i, n) in ["lan", "iot", "guest"].iter().enumerate() {
            r.on_iface_event(IfaceEvent::Link {
                index: i as u32 + 10,
                name: n.to_string(),
                up: true,
            });
            r.on_iface_event(IfaceEvent::AddrAdded {
                index: i as u32 + 10,
                addr: format!("10.{i}.0.1").parse().unwrap(),
            });
        }
        r.on_iface_event(IfaceEvent::Synced);
        (r, sock)
    }

    fn packet(ifindex: u32, src: &str, data: &[u8]) -> Received {
        Received {
            family: Family::V4,
            data: data.to_vec(),
            src: SocketAddr::new(src.parse().unwrap(), 5353),
            ifindex,
        }
    }

    #[test]
    fn joins_configured_interfaces_when_up() {
        let (_r, sock) = setup();
        assert_eq!(*sock.joined.lock().unwrap(), vec![10, 11, 12]);
    }

    #[test]
    fn forwards_to_every_other_up_interface() {
        let (mut r, sock) = setup();
        let to = r.on_packet(packet(10, "10.0.0.7", b"hello"));
        assert_eq!(to, vec!["iot", "guest"]);
        assert_eq!(
            *sock.sent.lock().unwrap(),
            vec![(b"hello".to_vec(), 11), (b"hello".to_vec(), 12)]
        );
        assert_eq!(r.counters.tx, 2);
    }

    #[test]
    fn drops_own_source_and_duplicates() {
        let (mut r, _sock) = setup();
        assert!(r.on_packet(packet(11, "10.1.0.1", b"mine")).is_empty());
        assert_eq!(r.counters.dropped_own_source, 1);
        assert_eq!(r.on_packet(packet(10, "10.0.0.7", b"twice")).len(), 2);
        assert!(r.on_packet(packet(12, "10.2.0.9", b"twice")).is_empty());
        assert_eq!(r.counters.dropped_duplicate, 1);
        std::thread::sleep(Duration::from_millis(250));
        assert_eq!(r.on_packet(packet(12, "10.2.0.9", b"twice")).len(), 2);
    }

    #[test]
    fn unknown_and_down_interfaces() {
        let (mut r, _sock) = setup();
        assert!(r.on_packet(packet(99, "10.9.0.7", b"x")).is_empty());
        assert_eq!(r.counters.dropped_unknown_interface, 1);
        r.on_iface_event(IfaceEvent::Link {
            index: 12,
            name: "guest".into(),
            up: false,
        });
        assert_eq!(r.on_packet(packet(10, "10.0.0.7", b"y")), vec!["iot"]);
        r.on_iface_event(IfaceEvent::LinkGone { index: 11 });
        assert!(r.on_packet(packet(10, "10.0.0.7", b"z")).is_empty());
        let st = r.status();
        assert_eq!(st.interfaces[1].index, None);
        assert!(!st.interfaces[2].up);
        assert_eq!(st.interfaces[0].rx, 2);
    }
}
