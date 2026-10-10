//! The core: interface state, the loop checks, the host table, the rule chain, the rebuild of a
//! packet after records were dropped, and the answers to the control socket.

use crate::config::Config;
use crate::hosts::HostTable;
use crate::ifaces::IfaceEvent;
use crate::packet::{Kind, Packet, Section};
use crate::rules::{self, Chain, Subject, Verdict};
use crate::socket::{Family, MdnsSocket, Received};
use anyhow::{Context, Result};
use discowall_proto::{
    Action, Counters, InterfaceStats, InterfaceStatus, PacketKind, Request, Response, RuleSet,
    RuleStats, StatsView, Status, TestResult, TestSpec, WatchDecision, WatchEvent,
};
use hickory_proto::op::Message;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
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
    rules_path: PathBuf,
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
    chain: Chain,
    /// The chain differs from the rules file (edited over the socket, not saved).
    unsaved: bool,
    hosts: HostTable,
    pub counters: Counters,
    /// `discoctl watch` subscribers; nobody listening is fine.
    pub events: broadcast::Sender<WatchEvent>,
}

fn kind_of(k: Kind) -> PacketKind {
    match k {
        Kind::Query => PacketKind::Query,
        Kind::Probe => PacketKind::Probe,
        Kind::Response => PacketKind::Response,
        Kind::Announce => PacketKind::Announce,
    }
}

/// Reads the rules file; a missing file is an empty chain.
fn load_rules(path: &std::path::Path) -> Result<RuleSet> {
    match std::fs::read_to_string(path) {
        Ok(text) => toml::from_str(&text).with_context(|| format!("parsing {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            warn!(
                "no rules file at {}: default policy drop, no rules",
                path.display()
            );
            Ok(RuleSet::default())
        }
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

fn save_rules(path: &std::path::Path, set: &RuleSet) -> Result<()> {
    let text = format!(
        "# discowall rules — written by `discoctl save`; `discoctl apply` reloads it.\n\n{}",
        toml::to_string(set).context("serialising the rules")?
    );
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("renaming to {}", path.display()))?;
    Ok(())
}

impl Reflector {
    pub fn new(
        config: &Config,
        config_path: String,
        sockets: Vec<Arc<dyn MdnsSocket>>,
    ) -> Result<Self> {
        let ifaces = config
            .interfaces
            .iter()
            .map(|n| (n.clone(), Iface::default()))
            .collect();
        let set = load_rules(&config.rules_file)?;
        let chain = Chain::compile(&set)
            .with_context(|| format!("compiling {}", config.rules_file.display()))?;
        info!(rules = chain.rules.len(), default = %chain.default, "rules loaded from {}", config.rules_file.display());
        Ok(Self {
            config_path,
            rules_path: config.rules_file.clone(),
            started: Instant::now(),
            names: config.interfaces.clone(),
            ifaces,
            index_to_name: HashMap::new(),
            own_addrs: HashMap::new(),
            sockets,
            dedup: HashMap::new(),
            dedup_window: Duration::from_millis(config.dedup_window_ms),
            dedup_sweep: Instant::now(),
            chain,
            unsaved: false,
            hosts: HostTable::default(),
            counters: Counters::default(),
            events: broadcast::channel(256).0,
        })
    }

    fn socket(&self, family: Family) -> Option<&Arc<dyn MdnsSocket>> {
        self.sockets.iter().find(|s| s.family() == family)
    }

    /// Every few seconds: expire the host table.
    pub fn tick(&mut self) {
        self.hosts.expire(Instant::now());
    }

    pub fn reload_rules(&mut self) {
        match load_rules(&self.rules_path).and_then(|set| Chain::compile(&set)) {
            Ok(chain) => {
                info!(rules = chain.rules.len(), default = %chain.default, "rules reloaded");
                self.chain = chain;
                self.unsaved = false;
            }
            Err(e) => warn!("rules not reloaded, the old chain stays: {e:#}"),
        }
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

    /// The subject of one name as seen from `in_iface`: the name itself plus what the host table
    /// ties to it (research 3.5), so that a rule on a host matches the PTR of its service and a
    /// rule on a service matches the A record of its host.
    fn subject<'a>(
        &self,
        in_iface: &'a str,
        out_iface: &'a str,
        src: IpAddr,
        kind: PacketKind,
        names: &[&str],
        rrtype: Option<hickory_proto::rr::RecordType>,
    ) -> Subject<'a> {
        let mut s = Subject {
            in_iface,
            out_iface,
            src: Some(src),
            kind,
            rrtype,
            ..Default::default()
        };
        for name in names {
            let b = self.hosts.bundle_of(in_iface, name);
            for st in b.services {
                if !s.services.contains(&st) {
                    s.services.push(st);
                }
            }
            for i in b.instances {
                let short = rules::short_instance(&i);
                if !s.instances.contains(&short) {
                    s.instances.push(short);
                }
            }
            for h in b.hosts {
                let short = rules::short_host(&h);
                if !s.hosts.contains(&short) {
                    s.hosts.push(short);
                }
            }
        }
        s
    }

    /// Applies the chain to a parsed packet for one target interface. Returns the bytes to send,
    /// or `None` when nothing of it may cross.
    fn filter(
        &mut self,
        pkt: &Packet,
        original: &[u8],
        in_iface: &str,
        out_iface: &str,
        src: IpAddr,
    ) -> Filtered {
        let kind = kind_of(pkt.kind);
        let mut keep_q = Vec::with_capacity(pkt.questions.len());
        let mut keep_r = Vec::with_capacity(pkt.records.len());
        let mut dropped = 0usize;
        let mut rules_hit: Vec<usize> = Vec::new();
        let mut log_hits: Vec<(Verdict, String)> = Vec::new();
        for q in &pkt.questions {
            let subject = self.subject(in_iface, out_iface, src, kind, &[&q.name], Some(q.rtype));
            let v = self.chain.decide(&subject);
            if v.log {
                log_hits.push((v, format!("{} {}", q.rtype, q.name)));
            }
            if let Some(n) = v.rule {
                if !rules_hit.contains(&n) {
                    rules_hit.push(n);
                }
            }
            keep_q.push(v.action == Action::Accept);
            if v.action != Action::Accept {
                dropped += 1;
            }
        }
        for r in &pkt.records {
            let mut names: Vec<&str> = vec![&r.name];
            match &r.data {
                crate::packet::Data::Ptr(t) => names.push(t),
                crate::packet::Data::Srv { target, .. } => names.push(target),
                _ => {}
            }
            let subject = self.subject(in_iface, out_iface, src, kind, &names, Some(r.rtype));
            let v = self.chain.decide(&subject);
            if v.log {
                log_hits.push((v, format!("{} {}", r.rtype, r.name)));
            }
            if let Some(n) = v.rule {
                if !rules_hit.contains(&n) {
                    rules_hit.push(n);
                }
            }
            keep_r.push(v.action == Action::Accept);
            if v.action != Action::Accept {
                dropped += 1;
            }
        }
        for (v, what) in log_hits {
            info!(
                "rule {} {} {what}: {} -> {} from {src} ({kind})",
                v.rule
                    .map(|r| r.to_string())
                    .unwrap_or_else(|| "default".into()),
                v.action,
                in_iface,
                out_iface
            );
        }
        self.counters.records_dropped += dropped as u64;
        // A query with no question left, a response with no answer left: nothing to send.
        let alive = if pkt.is_query() {
            keep_q.iter().any(|k| *k)
        } else {
            pkt.records
                .iter()
                .zip(&keep_r)
                .any(|(r, k)| *k && r.section == Section::Answer)
        };
        let bytes = if !alive {
            None
        } else if dropped == 0 {
            Some(original.to_vec())
        } else {
            Some(rebuild(&pkt.message, &keep_q, &keep_r))
        };
        Filtered {
            bytes,
            dropped,
            rules: rules_hit,
        }
    }

    fn emit(
        &self,
        in_iface: &str,
        pkt: &Received,
        kind: String,
        parsed: Option<&Packet>,
        out: Vec<WatchDecision>,
    ) {
        if self.events.receiver_count() == 0 {
            return;
        }
        let ts_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let ev = WatchEvent {
            ts_ms,
            iface: in_iface.to_string(),
            src: pkt.src.to_string(),
            len: pkt.data.len(),
            kind,
            services: parsed.map(|p| p.service_types()).unwrap_or_default(),
            instances: parsed.map(|p| p.instances()).unwrap_or_default(),
            hosts: parsed.map(|p| p.hosts()).unwrap_or_default(),
            out,
        };
        let _ = self.events.send(ev);
    }

    /// One packet in. Returns the names of the interfaces it was sent to.
    pub fn on_packet(&mut self, pkt: Received) -> Vec<String> {
        self.counters.rx += 1;
        let Some(in_name) = self.index_to_name.get(&pkt.ifindex).cloned() else {
            self.counters.dropped_unknown_interface += 1;
            self.emit(
                &format!("#{}", pkt.ifindex),
                &pkt,
                "unknown-interface".into(),
                None,
                vec![],
            );
            return vec![];
        };
        self.ifaces.get_mut(&in_name).expect("configured").rx += 1;
        if self.own_addrs.contains_key(&pkt.src.ip()) {
            self.counters.dropped_own_source += 1;
            self.emit(&in_name, &pkt, "own-source".into(), None, vec![]);
            return vec![];
        }
        if self.is_duplicate(&pkt.data) {
            self.counters.dropped_duplicate += 1;
            debug!(iface = %in_name, src = %pkt.src, "duplicate dropped");
            self.emit(&in_name, &pkt, "duplicate".into(), None, vec![]);
            return vec![];
        }
        let parsed = match Packet::parse(&pkt.data) {
            Ok(p) => p,
            Err(e) => {
                self.counters.dropped_unparsable += 1;
                debug!(iface = %in_name, src = %pkt.src, len = pkt.data.len(), "unparsable, dropped: {e}");
                self.emit(&in_name, &pkt, "unparsable".into(), None, vec![]);
                return vec![];
            }
        };
        self.hosts.learn(&in_name, &parsed, Instant::now());
        let Some(socket) = self.socket(pkt.family).cloned() else {
            return vec![];
        };
        let targets: Vec<(String, Option<u32>)> = self
            .names
            .iter()
            .filter(|n| **n != in_name)
            .map(|n| {
                let i = &self.ifaces[n];
                (
                    n.clone(),
                    (i.up && i.joined.contains(&pkt.family))
                        .then(|| i.index.expect("up implies index")),
                )
            })
            .collect();
        let mut sent = Vec::with_capacity(targets.len());
        let mut decisions = Vec::with_capacity(targets.len());
        let mut filtered_out = 0;
        for (name, index) in targets {
            let Some(index) = index else {
                decisions.push(WatchDecision {
                    iface: name,
                    result: "down".into(),
                    dropped: 0,
                    rules: vec![],
                });
                continue;
            };
            let f = self.filter(&parsed, &pkt.data, &in_name, &name, pkt.src.ip());
            let Some(bytes) = f.bytes else {
                filtered_out += 1;
                decisions.push(WatchDecision {
                    iface: name,
                    result: "dropped".into(),
                    dropped: f.dropped,
                    rules: f.rules,
                });
                continue;
            };
            match socket.send_multicast(&bytes, index) {
                Ok(()) => {
                    self.counters.tx += 1;
                    self.ifaces.get_mut(&name).expect("configured").tx += 1;
                    let result = if f.dropped == 0 { "sent" } else { "partial" };
                    decisions.push(WatchDecision {
                        iface: name.clone(),
                        result: result.into(),
                        dropped: f.dropped,
                        rules: f.rules,
                    });
                    sent.push(name);
                }
                Err(e) => {
                    self.counters.send_errors += 1;
                    warn!(iface = %name, "send failed: {e:#}");
                    decisions.push(WatchDecision {
                        iface: name,
                        result: "error".into(),
                        dropped: f.dropped,
                        rules: f.rules,
                    });
                }
            }
        }
        if filtered_out > 0 && sent.is_empty() {
            self.counters.dropped_by_rules += 1;
        }
        debug!(iface = %in_name, src = %pkt.src, len = pkt.data.len(), to = ?sent, "{}", parsed.summary());
        self.emit(
            &in_name,
            &pkt,
            parsed.kind.to_string(),
            Some(&parsed),
            decisions,
        );
        sent
    }

    pub fn handle(&mut self, request: Request) -> Response {
        match request {
            Request::Ping => Response::Pong,
            Request::Status => Response::Status(self.status()),
            Request::RuleList => Response::Rules(self.chain.view(self.unsaved)),
            Request::RuleAdd { rule, at } => match self.chain.insert(rule, at) {
                Ok(_) => {
                    self.unsaved = true;
                    Response::Ok
                }
                Err(e) => Response::Error {
                    message: format!("{e:#}"),
                },
            },
            Request::RuleDel { index } => match self.chain.remove(index) {
                Ok(_) => {
                    self.unsaved = true;
                    Response::Ok
                }
                Err(e) => Response::Error {
                    message: format!("{e:#}"),
                },
            },
            Request::RuleFlush => {
                self.chain = Chain::empty(self.chain.default);
                self.unsaved = true;
                Response::Ok
            }
            Request::RuleSave => match save_rules(&self.rules_path, &self.chain.to_set()) {
                Ok(()) => {
                    self.unsaved = false;
                    info!(
                        rules = self.chain.rules.len(),
                        "rules saved to {}",
                        self.rules_path.display()
                    );
                    Response::Ok
                }
                Err(e) => Response::Error {
                    message: format!("{e:#}"),
                },
            },
            Request::RuleApply => {
                match load_rules(&self.rules_path).and_then(|set| Chain::compile(&set)) {
                    Ok(chain) => {
                        info!(rules = chain.rules.len(), default = %chain.default, "rules reloaded");
                        self.chain = chain;
                        self.unsaved = false;
                        Response::Ok
                    }
                    Err(e) => Response::Error {
                        message: format!("{e:#}"),
                    },
                }
            }
            Request::RuleTest(t) => Response::Test(self.test(&t)),
            Request::Hosts => Response::Hosts {
                hosts: self.hosts.view(Instant::now()),
            },
            Request::Services => Response::Services {
                services: self.hosts.services(Instant::now()),
            },
            Request::Stats => Response::Stats(self.stats()),
            // Streaming is the control task's business; it never sends this to the core.
            Request::Watch { .. } => Response::Error {
                message: "watch is handled by the control socket".into(),
            },
        }
    }

    fn stats(&self) -> StatsView {
        let (hosts, instances) = self.hosts.counts();
        StatsView {
            uptime_s: self.started.elapsed().as_secs(),
            counters: self.counters.clone(),
            interfaces: self
                .names
                .iter()
                .map(|n| InterfaceStats {
                    name: n.clone(),
                    rx: self.ifaces[n].rx,
                    tx: self.ifaces[n].tx,
                })
                .collect(),
            rules: self
                .chain
                .rules
                .iter()
                .enumerate()
                .map(|(i, r)| RuleStats {
                    index: i + 1,
                    hits: r.hits,
                    rule: r.spec.to_string(),
                })
                .collect(),
            default_hits: self.chain.default_hits,
            hosts,
            instances,
        }
    }

    fn test(&self, t: &TestSpec) -> TestResult {
        let rrtype = t
            .rrtype
            .as_ref()
            .and_then(|s| s.to_uppercase().parse().ok());
        let mut s = Subject {
            in_iface: &t.in_iface,
            out_iface: &t.out,
            src: t.src,
            kind: t.kind,
            rrtype,
            ..Default::default()
        };
        if let Some(x) = &t.service {
            s.services.push(x.clone());
        }
        if let Some(x) = &t.instance {
            s.instances.push(x.clone());
        }
        if let Some(x) = &t.host {
            s.hosts.push(x.clone());
        }
        let v = self.chain.peek(&s);
        TestResult {
            action: v.action,
            rule: v.rule,
        }
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

/// The outcome of `filter` for one target interface.
struct Filtered {
    bytes: Option<Vec<u8>>,
    dropped: usize,
    rules: Vec<usize>,
}

/// The message with only the kept questions and records, re-encoded.
fn rebuild(message: &Message, keep_q: &[bool], keep_r: &[bool]) -> Vec<u8> {
    let mut m = message.clone();
    let mut it = keep_q.iter();
    m.queries.retain(|_| *it.next().unwrap_or(&true));
    let mut it = keep_r.iter();
    m.answers.retain(|_| *it.next().unwrap_or(&true));
    m.authorities.retain(|_| *it.next().unwrap_or(&true));
    m.additionals.retain(|_| *it.next().unwrap_or(&true));
    m.to_vec().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hickory_proto::op::{MessageType, OpCode, Query};
    use hickory_proto::rr::rdata::{A, PTR, SRV, TXT};
    use hickory_proto::rr::{DNSClass, Name, RData, Record, RecordType};
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
        fn take(&self) -> Vec<(Vec<u8>, u32)> {
            std::mem::take(&mut *self.sent.lock().unwrap())
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

    /// A throwaway directory under the system temp dir, removed on drop.
    struct TempDir {
        path: std::path::PathBuf,
    }
    impl TempDir {
        fn new() -> Self {
            static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("discowall-test-{}-{n}", std::process::id()));
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn config(names: &[&str], rules: &str) -> (Config, TempDir) {
        let dir = TempDir::new();
        let rules_path = dir.path.join("rules.toml");
        std::fs::write(&rules_path, rules).unwrap();
        let cfg: Config = toml::from_str(&format!(
            "interfaces = [{}]\ndedup_window_ms = 200\nrules_file = {:?}",
            names
                .iter()
                .map(|n| format!("{n:?}"))
                .collect::<Vec<_>>()
                .join(", "),
            rules_path.display()
        ))
        .unwrap();
        (cfg, dir)
    }

    fn setup(rules: &str) -> (Reflector, Arc<FakeSocket>, TempDir) {
        let sock = FakeSocket::new(Family::V4);
        let (cfg, dir) = config(&["lan", "iot", "guest"], rules);
        let mut r = Reflector::new(&cfg, "test".into(), vec![sock.clone()]).unwrap();
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
        (r, sock, dir)
    }

    const ACCEPT_ALL: &str = "default = \"accept\"\n";

    fn name(s: &str) -> Name {
        Name::from_labels(s.trim_end_matches('.').split('.').map(str::as_bytes)).unwrap()
    }

    fn rr(owner: &str, data: RData, flush: bool) -> Record {
        let mut r = Record::from_rdata(name(owner), 120, data);
        r.dns_class = DNSClass::IN;
        r.mdns_cache_flush = flush;
        r
    }

    fn browse(service: &str) -> Vec<u8> {
        let mut m = Message::new(0, MessageType::Query, OpCode::Query);
        m.add_query(Query::query(
            name(&format!("{service}.local")),
            RecordType::PTR,
        ));
        m.to_vec().unwrap()
    }

    fn announce(instance: &str, service: &str, host: &str, ip: &str) -> Vec<u8> {
        let mut m = Message::new(0, MessageType::Response, OpCode::Query);
        let full = format!("{instance}.{service}.local");
        m.add_answer(rr(
            &format!("{service}.local"),
            RData::PTR(PTR(name(&full))),
            false,
        ));
        m.add_answer(rr(
            &full,
            RData::SRV(SRV::new(0, 0, 1234, name(&format!("{host}.local")))),
            true,
        ));
        m.add_answer(rr(&full, RData::TXT(TXT::new(vec!["a=b".into()])), true));
        m.add_additional(rr(
            &format!("{host}.local"),
            RData::A(A(ip.parse().unwrap())),
            true,
        ));
        m.to_vec().unwrap()
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
        let (_r, sock, _d) = setup(ACCEPT_ALL);
        assert_eq!(*sock.joined.lock().unwrap(), vec![10, 11, 12]);
    }

    #[test]
    fn accept_all_forwards_unchanged_to_every_other_up_interface() {
        let (mut r, sock, _d) = setup(ACCEPT_ALL);
        let q = browse("_hap._tcp");
        let to = r.on_packet(packet(10, "10.0.0.7", &q));
        assert_eq!(to, vec!["iot", "guest"]);
        assert_eq!(sock.take(), vec![(q.clone(), 11), (q, 12)]);
        assert_eq!(r.counters.tx, 2);
    }

    #[test]
    fn drops_own_source_duplicates_and_garbage() {
        let (mut r, _sock, _d) = setup(ACCEPT_ALL);
        let q = browse("_hap._tcp");
        assert!(r.on_packet(packet(11, "10.1.0.1", &q)).is_empty());
        assert_eq!(r.counters.dropped_own_source, 1);
        assert_eq!(r.on_packet(packet(10, "10.0.0.7", &q)).len(), 2);
        assert!(r.on_packet(packet(12, "10.2.0.9", &q)).is_empty());
        assert_eq!(r.counters.dropped_duplicate, 1);
        std::thread::sleep(Duration::from_millis(250));
        assert_eq!(r.on_packet(packet(12, "10.2.0.9", &q)).len(), 2);
        assert!(r.on_packet(packet(10, "10.0.0.7", b"not dns")).is_empty());
        assert_eq!(r.counters.dropped_unparsable, 1);
    }

    #[test]
    fn unknown_and_down_interfaces() {
        let (mut r, _sock, _d) = setup(ACCEPT_ALL);
        let q = browse("_hap._tcp");
        assert!(r.on_packet(packet(99, "10.9.0.7", &q)).is_empty());
        assert_eq!(r.counters.dropped_unknown_interface, 1);
        r.on_iface_event(IfaceEvent::Link {
            index: 12,
            name: "guest".into(),
            up: false,
        });
        assert_eq!(
            r.on_packet(packet(10, "10.0.0.7", &browse("_a._tcp"))),
            vec!["iot"]
        );
        r.on_iface_event(IfaceEvent::LinkGone { index: 11 });
        assert!(r
            .on_packet(packet(10, "10.0.0.7", &browse("_b._tcp")))
            .is_empty());
        let st = r.status();
        assert_eq!(st.interfaces[1].index, None);
        assert!(!st.interfaces[2].up);
        assert_eq!(st.interfaces[0].rx, 2);
    }

    const HOMEKIT: &str = r#"
default = "drop"
[[rule]]
in = "lan"
out = "iot"
kind = ["query"]
service = ["_hap._tcp"]
action = "accept"
[[rule]]
in = "iot"
out = "lan"
kind = ["response", "announce"]
host = ["bridge"]
action = "accept"
"#;

    #[test]
    fn homekit_rules_direction_and_bundles() {
        let (mut r, sock, _d) = setup(HOMEKIT);
        let mut events = r.events.subscribe();
        let dropped = |r: &Reflector| r.counters.records_dropped;
        // lan asks for HomeKit: goes to iot only (guest is in no rule → default drop).
        assert_eq!(
            r.on_packet(packet(10, "10.0.0.7", &browse("_hap._tcp"))),
            vec!["iot"]
        );
        assert_eq!(dropped(&r), 1, "the question, for guest");
        // lan asks for AirPlay: nowhere.
        assert!(r
            .on_packet(packet(10, "10.0.0.7", &browse("_airplay._tcp")))
            .is_empty());
        assert_eq!(r.counters.dropped_by_rules, 1);
        assert_eq!(dropped(&r), 3, "the question, for iot and guest");
        // iot asks lan for HomeKit: the wrong direction (after the dedup window: same bytes as lan's).
        std::thread::sleep(Duration::from_millis(250));
        assert!(r
            .on_packet(packet(11, "10.1.0.5", &browse("_hap._tcp")))
            .is_empty());
        assert_eq!(r.counters.dropped_by_rules, 2);
        assert_eq!(dropped(&r), 5);
        sock.take();
        // The bridge announces on iot: everything of it crosses to lan, unchanged.
        let a = announce("Living Room", "_hap._tcp", "bridge", "10.1.0.5");
        assert_eq!(r.on_packet(packet(11, "10.1.0.5", &a)), vec!["lan"]);
        assert_eq!(sock.take(), vec![(a, 10)]);
        assert_eq!(dropped(&r), 9, "4 records for guest, none for lan");
        // A camera announces on iot: nothing crosses.
        let c = announce("Cam", "_hap._tcp", "camera", "10.1.0.6");
        assert!(r.on_packet(packet(11, "10.1.0.6", &c)).is_empty());
        assert_eq!(dropped(&r), 17, "4 records for lan and for guest");
        // A later answer with only the bridge's A record is attributed through the host table.
        let mut m = Message::new(0, MessageType::Response, OpCode::Query);
        m.add_answer(rr(
            "bridge.local",
            RData::A(A("10.1.0.5".parse().unwrap())),
            true,
        ));
        assert_eq!(
            r.on_packet(packet(11, "10.1.0.5", &m.to_vec().unwrap())),
            vec!["lan"]
        );
        assert_eq!(dropped(&r), 18);
        // A mixed answer: the bridge's and the camera's PTRs in one packet → rebuilt with one.
        let mut m = Message::new(0, MessageType::Response, OpCode::Query);
        m.add_answer(rr(
            "_hap._tcp.local",
            RData::PTR(PTR(name("Living Room._hap._tcp.local"))),
            false,
        ));
        m.add_answer(rr(
            "_hap._tcp.local",
            RData::PTR(PTR(name("Cam._hap._tcp.local"))),
            false,
        ));
        assert_eq!(
            r.on_packet(packet(11, "10.1.0.5", &m.to_vec().unwrap())),
            vec!["lan"]
        );
        assert_eq!(dropped(&r), 21, "1 for lan, 2 for guest");
        let sent = sock.take();
        let out = Packet::parse(&sent.last().unwrap().0).unwrap();
        assert_eq!(out.records.len(), 1);
        assert_eq!(out.instances(), vec!["living room._hap._tcp.local"]);
        let hosts = r.hosts.view(Instant::now());
        assert_eq!(hosts.len(), 2);
        // The watch stream saw every packet with its decisions.
        let first = events.try_recv().unwrap();
        assert_eq!(first.kind, "query");
        assert_eq!(first.services, vec!["_hap._tcp"]);
        assert_eq!(first.out.len(), 2);
        assert_eq!(
            (
                first.out[0].iface.as_str(),
                first.out[0].result.as_str(),
                &first.out[0].rules
            ),
            ("iot", "sent", &vec![1])
        );
        assert_eq!(
            (
                first.out[1].iface.as_str(),
                first.out[1].result.as_str(),
                first.out[1].dropped
            ),
            ("guest", "dropped", 1)
        );
        let mut last = first;
        while let Ok(e) = events.try_recv() {
            last = e;
        }
        assert_eq!(last.kind, "response");
        assert_eq!(last.out[0].result, "partial");
        assert_eq!(last.out[0].dropped, 1);
        let st = r.stats();
        assert_eq!(st.rules[1].hits, 4 + 1 + 1);
        assert_eq!((st.hosts, st.instances), (2, 2));
        assert_eq!(st.interfaces[1].rx, 5);
        assert_eq!(r.hosts.services(Instant::now()).len(), 2);
    }

    #[test]
    fn rule_commands_save_and_apply() {
        let (mut r, _sock, d) = setup(HOMEKIT);
        let Response::Rules(list) = r.handle(Request::RuleList) else {
            panic!()
        };
        assert_eq!(list.rules.len(), 2);
        assert!(!list.unsaved);
        let spec: discowall_proto::RuleSpec =
            toml::from_str("host = [\"printer*\"]\naction = \"accept\"").unwrap();
        assert_eq!(
            r.handle(Request::RuleAdd {
                rule: spec.clone(),
                at: Some(1)
            }),
            Response::Ok
        );
        let Response::Rules(list) = r.handle(Request::RuleList) else {
            panic!()
        };
        assert_eq!(list.rules[0].spec, spec);
        assert!(list.unsaved);
        assert!(matches!(
            r.handle(Request::RuleDel { index: 9 }),
            Response::Error { .. }
        ));
        assert_eq!(r.handle(Request::RuleSave), Response::Ok);
        let text = std::fs::read_to_string(d.path.join("rules.toml")).unwrap();
        assert!(text.contains("printer*"), "{text}");
        assert_eq!(r.handle(Request::RuleFlush), Response::Ok);
        let Response::Rules(list) = r.handle(Request::RuleList) else {
            panic!()
        };
        assert!(list.rules.is_empty() && list.unsaved);
        assert_eq!(r.handle(Request::RuleApply), Response::Ok);
        let Response::Rules(list) = r.handle(Request::RuleList) else {
            panic!()
        };
        assert_eq!(list.rules.len(), 3);
        assert!(!list.unsaved);
        let t = TestSpec {
            in_iface: "iot".into(),
            out: "lan".into(),
            kind: PacketKind::Announce,
            host: Some("printer1".into()),
            ..Default::default()
        };
        assert_eq!(
            r.handle(Request::RuleTest(t)),
            Response::Test(TestResult {
                action: Action::Accept,
                rule: Some(1)
            })
        );
        let t = TestSpec {
            in_iface: "iot".into(),
            out: "lan".into(),
            kind: PacketKind::Query,
            ..Default::default()
        };
        assert_eq!(
            r.handle(Request::RuleTest(t)),
            Response::Test(TestResult {
                action: Action::Drop,
                rule: None
            })
        );
    }

    #[test]
    fn missing_rules_file_means_drop_everything() {
        let sock = FakeSocket::new(Family::V4);
        let cfg: Config = toml::from_str(
            "interfaces = [\"a\", \"b\"]\nrules_file = \"/nonexistent/discowall/rules.toml\"",
        )
        .unwrap();
        let mut r = Reflector::new(&cfg, "test".into(), vec![sock]).unwrap();
        r.on_iface_event(IfaceEvent::Link {
            index: 1,
            name: "a".into(),
            up: true,
        });
        r.on_iface_event(IfaceEvent::Link {
            index: 2,
            name: "b".into(),
            up: true,
        });
        assert!(r
            .on_packet(packet(1, "10.0.0.7", &browse("_hap._tcp")))
            .is_empty());
        assert_eq!(r.counters.dropped_by_rules, 1);
    }
}
