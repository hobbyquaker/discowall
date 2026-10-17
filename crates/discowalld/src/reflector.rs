//! The core: interface state, the loop checks, the host table, the rule chain, the rebuild of a
//! packet after records were dropped, and the answers to the control socket.

use crate::config::Config;
use crate::hosts::HostTable;
use crate::ifaces::IfaceEvent;
use crate::packet::{Kind, Packet, Section};
use crate::proxy;
use crate::rules::{self, Chain, Subject, Verdict};
use crate::socket::{Family, MdnsSocket, Proto, Received, ReplyTo, SocketFactory};
use crate::ssdp::{SsdpKind, SsdpMessage};
use anyhow::{Context, Result};
use discowall_proto::{
    Action, Counters, InterfaceStats, InterfaceStatus, PacketKind, Request, Response, RuleSet,
    RuleStats, StatsView, Status, TestResult, TestSpec, UnparsableView, WatchDecision, WatchEvent,
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
use tokio::sync::mpsc;
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
    /// The most recent undecodable packet: (when, protocol, interface, source, reason).
    last_unparsable: Option<(Instant, Proto, String, String, String)>,
    /// Per (source address, interface): when the last info line about an undecodable packet was
    /// written and how many were suppressed since (task 17).
    unparsable_log: HashMap<(IpAddr, String), (Instant, u64)>,
    unparsable_log_window: Duration,
    chain: Chain,
    /// The chain differs from the rules file (edited over the socket, not saved).
    unsaved: bool,
    hosts: HostTable,
    pub counters: Counters,
    /// `discoctl watch` subscribers; nobody listening is fine.
    pub events: broadcast::Sender<WatchEvent>,
    factory: SocketFactory,
    /// Where the proxy's collected answers come back in.
    packet_tx: mpsc::Sender<Received>,
    strip_link_local: bool,
    legacy_window: Duration,
}

fn is_link_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(a) => a.is_link_local(),
        IpAddr::V6(a) => a.segments()[0] & 0xffc0 == 0xfe80,
    }
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

/// `discowalld --check`: loads and compiles a rules file exactly as start and `apply` do, and says
/// what the daemon would run with (B-5). A missing file is not an error — the daemon starts with
/// default drop then — but it is said.
pub fn check_rules(path: &std::path::Path) -> Result<String> {
    if !path.exists() {
        return Ok(format!(
            "{}: no rules file: default drop, nothing crosses",
            path.display()
        ));
    }
    let set = load_rules(path)?;
    let chain = Chain::compile(&set).with_context(|| format!("compiling {}", path.display()))?;
    Ok(format!(
        "{}: ok ({} rules, default {})",
        path.display(),
        chain.rules.len(),
        chain.default
    ))
}

fn render_rules(set: &RuleSet) -> Result<String> {
    Ok(format!(
        "# discowall rules — written by `discoctl save`; `discoctl apply` reloads it.\n\n{}",
        toml::to_string(set).context("serialising the rules")?
    ))
}

fn save_rules(path: &std::path::Path, set: &RuleSet) -> Result<()> {
    let text = render_rules(set)?;
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
        factory: SocketFactory,
        packet_tx: mpsc::Sender<Received>,
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
            last_unparsable: None,
            unparsable_log: HashMap::new(),
            unparsable_log_window: Duration::from_secs(600),
            chain,
            unsaved: false,
            hosts: HostTable::default(),
            counters: Counters::default(),
            events: broadcast::channel(256).0,
            factory,
            packet_tx,
            strip_link_local: config.strip_link_local,
            legacy_window: Duration::from_millis(config.legacy_window_ms),
        })
    }

    fn socket(&self, proto: Proto, family: Family) -> Option<&Arc<dyn MdnsSocket>> {
        self.sockets
            .iter()
            .find(|s| s.proto() == proto && s.family() == family)
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

    /// The same bytes within the window are a duplicate (a second reflector, a bridge that delivers
    /// twice) — per address family: avahi sends one query over IPv4 and IPv6 at the same moment, and
    /// those are two packets, not one (B-2, measured in the lab 2026-10-13).
    fn is_duplicate(&mut self, data: &[u8], family: Family) -> bool {
        let now = Instant::now();
        if now.duration_since(self.dedup_sweep) > self.dedup_window * 4 {
            let window = self.dedup_window;
            self.dedup.retain(|_, t| now.duration_since(*t) <= window);
            self.dedup_sweep = now;
        }
        let mut h = DefaultHasher::new();
        matches!(family, Family::V6).hash(&mut h);
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
        let mut link_local = 0usize;
        for r in &pkt.records {
            if self.strip_link_local {
                if let crate::packet::Data::Addr(ip) = r.data {
                    if is_link_local(ip) {
                        keep_r.push(false);
                        link_local += 1;
                        continue;
                    }
                }
            }
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
        self.counters.dropped_link_local += link_local as u64;
        // The QU bit asks for a unicast answer, which would go to this router instead of the
        // querier once reflected: cleared (research 3.7).
        let qu = pkt
            .questions
            .iter()
            .zip(&keep_q)
            .any(|(q, k)| *k && q.unicast);
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
        } else if dropped == 0 && link_local == 0 && !qu {
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

    /// An undecodable packet (task 17): counted, remembered for `status`/`stats`, shown with its
    /// reason in `watch`, and logged at info at most once per source and interface per window,
    /// with the number suppressed since.
    fn unparsable(&mut self, in_iface: &str, pkt: &Received, reason: String) {
        self.counters.dropped_unparsable += 1;
        let now = Instant::now();
        debug!(iface = %in_iface, src = %pkt.src, len = pkt.data.len(), "unparsable, dropped: {reason}");
        if let Some(suppressed) = self.unparsable_log_due(pkt.src.ip(), in_iface, now) {
            let more = if suppressed > 0 {
                format!(" ({suppressed} more from there since the last report)")
            } else {
                String::new()
            };
            info!(
                "unparsable {} packet from {} on {in_iface}, dropped: {reason}{more}",
                pkt.proto, pkt.src
            );
        }
        self.last_unparsable = Some((
            now,
            pkt.proto,
            in_iface.to_string(),
            pkt.src.to_string(),
            reason.clone(),
        ));
        if self.events.receiver_count() > 0 {
            let ts_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let _ = self.events.send(WatchEvent {
                ts_ms,
                proto: pkt.proto,
                iface: in_iface.to_string(),
                src: pkt.src.to_string(),
                len: pkt.data.len(),
                kind: "unparsable".into(),
                services: vec![],
                instances: vec![],
                hosts: vec![],
                out: vec![],
                reason: Some(reason),
            });
        }
    }

    /// `Some(suppressed)` when an info line is due for this source and interface, `None` while the
    /// window since the last one is running (then the suppressed count grows).
    fn unparsable_log_due(&mut self, src: IpAddr, iface: &str, now: Instant) -> Option<u64> {
        let window = self.unparsable_log_window;
        if self.unparsable_log.len() > 1024 {
            self.unparsable_log
                .retain(|_, (t, _)| now.duration_since(*t) < window);
        }
        match self.unparsable_log.get_mut(&(src, iface.to_string())) {
            Some((last, suppressed)) if now.duration_since(*last) < window => {
                *suppressed += 1;
                None
            }
            Some((last, suppressed)) => {
                let n = *suppressed;
                *last = now;
                *suppressed = 0;
                Some(n)
            }
            None => {
                self.unparsable_log
                    .insert((src, iface.to_string()), (now, 0));
                Some(0)
            }
        }
    }

    fn last_unparsable_view(&self) -> Option<UnparsableView> {
        self.last_unparsable
            .as_ref()
            .map(|(t, proto, iface, src, reason)| UnparsableView {
                proto: *proto,
                iface: iface.clone(),
                src: src.clone(),
                reason: reason.clone(),
                ago_s: t.elapsed().as_secs(),
            })
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
            proto: pkt.proto,
            iface: in_iface.to_string(),
            src: pkt.src.to_string(),
            len: pkt.data.len(),
            kind,
            services: parsed.map(|p| p.service_types()).unwrap_or_default(),
            instances: parsed.map(|p| p.instances()).unwrap_or_default(),
            hosts: parsed.map(|p| p.hosts()).unwrap_or_default(),
            out,
            reason: None,
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
        if self.is_duplicate(&pkt.data, pkt.family) {
            self.counters.dropped_duplicate += 1;
            debug!(iface = %in_name, src = %pkt.src, "duplicate dropped");
            self.emit(&in_name, &pkt, "duplicate".into(), None, vec![]);
            return vec![];
        }
        if pkt.proto == Proto::Ssdp {
            return self.on_ssdp(pkt, &in_name);
        }
        let parsed = match Packet::parse(&pkt.data) {
            Ok(p) => p,
            Err(e) => {
                self.unparsable(&in_name, &pkt, e.to_string());
                return vec![];
            }
        };
        self.hosts.learn(&in_name, &parsed, Instant::now());
        let Some(socket) = self.socket(pkt.proto, pkt.family).cloned() else {
            return vec![];
        };
        if let Some(reply_to) = pkt.reply_to {
            return self.relay_answer(&pkt, &parsed, &in_name, reply_to, &socket);
        }
        if parsed.is_query() && pkt.src.port() != crate::socket::MDNS_PORT {
            return self.proxy_query(&pkt, &parsed, &in_name);
        }
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

    /// An answer the proxy collected for a one-shot querier: through the rules (in: the answering
    /// interface, out: the querier's), then by unicast to the querier from port 5353.
    fn relay_answer(
        &mut self,
        pkt: &Received,
        parsed: &Packet,
        in_name: &str,
        reply_to: ReplyTo,
        socket: &Arc<dyn MdnsSocket>,
    ) -> Vec<String> {
        let Some(out_name) = self.index_to_name.get(&reply_to.ifindex).cloned() else {
            self.counters.dropped_unknown_interface += 1;
            return vec![];
        };
        let f = self.filter(parsed, &pkt.data, in_name, &out_name, pkt.src.ip());
        let decision = match &f.bytes {
            None => WatchDecision {
                iface: out_name.clone(),
                result: "dropped".into(),
                dropped: f.dropped,
                rules: f.rules.clone(),
            },
            Some(bytes) => match socket.send_unicast(bytes, reply_to.addr, reply_to.ifindex) {
                Ok(()) => {
                    self.counters.tx += 1;
                    self.counters.proxied_answers += 1;
                    self.ifaces.get_mut(&out_name).expect("configured").tx += 1;
                    let result = if f.dropped == 0 {
                        "relayed"
                    } else {
                        "relayed-partial"
                    };
                    WatchDecision {
                        iface: out_name.clone(),
                        result: result.into(),
                        dropped: f.dropped,
                        rules: f.rules.clone(),
                    }
                }
                Err(e) => {
                    self.counters.send_errors += 1;
                    warn!(iface = %out_name, to = %reply_to.addr, "unicast relay failed: {e:#}");
                    WatchDecision {
                        iface: out_name.clone(),
                        result: "error".into(),
                        dropped: f.dropped,
                        rules: f.rules.clone(),
                    }
                }
            },
        };
        debug!(iface = %in_name, src = %pkt.src, to = %reply_to.addr, result = %decision.result, "legacy answer {}", parsed.summary());
        let sent = if decision.result.starts_with("relayed") {
            vec![out_name]
        } else {
            vec![]
        };
        self.emit(
            in_name,
            pkt,
            format!("{}/legacy", parsed.kind),
            Some(parsed),
            vec![decision],
        );
        sent
    }

    /// A one-shot query from an ephemeral port: ask on the allowed target interfaces from an
    /// ephemeral socket of our own; the answers come back through `relay_answer`.
    fn proxy_query(&mut self, pkt: &Received, parsed: &Packet, in_name: &str) -> Vec<String> {
        if self.legacy_window.is_zero() {
            self.counters.dropped_by_rules += 1;
            self.emit(
                in_name,
                pkt,
                format!("{}/legacy", parsed.kind),
                Some(parsed),
                vec![],
            );
            return vec![];
        }
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
        let mut queries = Vec::new();
        let mut decisions = Vec::new();
        let mut asked = Vec::new();
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
            let f = self.filter(parsed, &pkt.data, in_name, &name, pkt.src.ip());
            match f.bytes {
                Some(bytes) => {
                    let result = if f.dropped == 0 {
                        "asked"
                    } else {
                        "asked-partial"
                    };
                    decisions.push(WatchDecision {
                        iface: name.clone(),
                        result: result.into(),
                        dropped: f.dropped,
                        rules: f.rules,
                    });
                    queries.push((bytes, index));
                    asked.push(name);
                }
                None => decisions.push(WatchDecision {
                    iface: name,
                    result: "dropped".into(),
                    dropped: f.dropped,
                    rules: f.rules,
                }),
            }
        }
        if queries.is_empty() {
            self.counters.dropped_by_rules += 1;
        } else {
            let reply_to = ReplyTo {
                addr: pkt.src,
                ifindex: pkt.ifindex,
            };
            match proxy::ask(
                &self.factory,
                pkt.proto,
                pkt.family,
                queries,
                reply_to,
                self.legacy_window,
                self.packet_tx.clone(),
            ) {
                Ok(()) => self.counters.proxied_queries += 1,
                Err(e) => {
                    self.counters.send_errors += 1;
                    warn!("legacy proxy failed: {e:#}");
                }
            }
        }
        debug!(iface = %in_name, src = %pkt.src, to = ?asked, "legacy {}", parsed.summary());
        self.emit(
            in_name,
            pkt,
            format!("{}/legacy", parsed.kind),
            Some(parsed),
            decisions,
        );
        asked
    }

    /// SSDP: one message is one subject. Notifications are forwarded by multicast under the rules,
    /// searches are proxied (UPnP answers are always unicast, to the searcher's port), answers the
    /// proxy collected are relayed to the searcher.
    fn on_ssdp(&mut self, pkt: Received, in_name: &str) -> Vec<String> {
        let msg = match SsdpMessage::parse(&pkt.data) {
            Ok(m) => m,
            Err(e) => {
                self.unparsable(in_name, &pkt, e.to_string());
                return vec![];
            }
        };
        self.hosts.learn_ssdp(in_name, &msg, Instant::now());
        let Some(socket) = self.socket(Proto::Ssdp, pkt.family).cloned() else {
            return vec![];
        };
        let kind_name = format!("ssdp-{}", msg.kind);
        // A LOCATION on a link-local address is useless on another segment.
        if self.strip_link_local && msg.location_ip().is_some_and(is_link_local) {
            self.counters.dropped_link_local += 1;
            self.emit_ssdp(in_name, &pkt, kind_name, Some(&msg), vec![]);
            return vec![];
        }
        if let Some(reply_to) = pkt.reply_to {
            // An answer to a proxied search.
            let Some(out_name) = self.index_to_name.get(&reply_to.ifindex).cloned() else {
                self.counters.dropped_unknown_interface += 1;
                return vec![];
            };
            let v = self.ssdp_decide(&msg, in_name, &out_name, pkt.src.ip());
            let decision = if v.action == Action::Accept {
                match socket.send_unicast(&pkt.data, reply_to.addr, reply_to.ifindex) {
                    Ok(()) => {
                        self.counters.tx += 1;
                        self.counters.proxied_answers += 1;
                        self.ifaces.get_mut(&out_name).expect("configured").tx += 1;
                        WatchDecision {
                            iface: out_name.clone(),
                            result: "relayed".into(),
                            dropped: 0,
                            rules: v.rule.into_iter().collect(),
                        }
                    }
                    Err(e) => {
                        self.counters.send_errors += 1;
                        warn!(iface = %out_name, "ssdp unicast relay failed: {e:#}");
                        WatchDecision {
                            iface: out_name.clone(),
                            result: "error".into(),
                            dropped: 0,
                            rules: v.rule.into_iter().collect(),
                        }
                    }
                }
            } else {
                WatchDecision {
                    iface: out_name.clone(),
                    result: "dropped".into(),
                    dropped: 1,
                    rules: v.rule.into_iter().collect(),
                }
            };
            let sent = if decision.result == "relayed" {
                vec![out_name]
            } else {
                vec![]
            };
            self.emit_ssdp(in_name, &pkt, kind_name, Some(&msg), vec![decision]);
            return sent;
        }
        if msg.kind == SsdpKind::Response {
            // A unicast answer that reached our 1900 socket is not ours to forward.
            self.emit_ssdp(in_name, &pkt, kind_name, Some(&msg), vec![]);
            return vec![];
        }
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
        let mut decisions = Vec::with_capacity(targets.len());
        let mut sent = Vec::new();
        let mut queries = Vec::new();
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
            let v = self.ssdp_decide(&msg, in_name, &name, pkt.src.ip());
            let rules: Vec<usize> = v.rule.into_iter().collect();
            if v.log {
                info!(
                    "rule {} {} {}: {} -> {} from {} (ssdp {})",
                    v.rule
                        .map(|r| r.to_string())
                        .unwrap_or_else(|| "default".into()),
                    v.action,
                    msg.summary(),
                    in_name,
                    name,
                    pkt.src,
                    msg.kind
                );
            }
            if v.action != Action::Accept {
                self.counters.records_dropped += 1;
                decisions.push(WatchDecision {
                    iface: name,
                    result: "dropped".into(),
                    dropped: 1,
                    rules,
                });
                continue;
            }
            if msg.is_search() {
                queries.push((pkt.data.clone(), index));
                decisions.push(WatchDecision {
                    iface: name.clone(),
                    result: "asked".into(),
                    dropped: 0,
                    rules,
                });
                sent.push(name);
                continue;
            }
            match socket.send_multicast(&pkt.data, index) {
                Ok(()) => {
                    self.counters.tx += 1;
                    self.ifaces.get_mut(&name).expect("configured").tx += 1;
                    decisions.push(WatchDecision {
                        iface: name.clone(),
                        result: "sent".into(),
                        dropped: 0,
                        rules,
                    });
                    sent.push(name);
                }
                Err(e) => {
                    self.counters.send_errors += 1;
                    warn!(iface = %name, "ssdp send failed: {e:#}");
                    decisions.push(WatchDecision {
                        iface: name,
                        result: "error".into(),
                        dropped: 0,
                        rules,
                    });
                }
            }
        }
        if !queries.is_empty() {
            // Responders wait up to MX seconds (UPnP caps it at 5); collect a little longer.
            let mx = msg.mx.unwrap_or(1).min(5) as u64;
            let window = Duration::from_millis(mx * 1000 + 500);
            let reply_to = ReplyTo {
                addr: pkt.src,
                ifindex: pkt.ifindex,
            };
            match proxy::ask(
                &self.factory,
                Proto::Ssdp,
                pkt.family,
                queries,
                reply_to,
                window,
                self.packet_tx.clone(),
            ) {
                Ok(()) => self.counters.proxied_queries += 1,
                Err(e) => {
                    self.counters.send_errors += 1;
                    warn!("ssdp search proxy failed: {e:#}");
                }
            }
        }
        if sent.is_empty() && decisions.iter().any(|d| d.result == "dropped") {
            self.counters.dropped_by_rules += 1;
        }
        debug!(iface = %in_name, src = %pkt.src, to = ?sent, "{}", msg.summary());
        self.emit_ssdp(in_name, &pkt, kind_name, Some(&msg), decisions);
        sent
    }

    fn ssdp_decide(
        &mut self,
        msg: &SsdpMessage,
        in_iface: &str,
        out_iface: &str,
        src: IpAddr,
    ) -> Verdict {
        let kind = match msg.kind {
            SsdpKind::Search => PacketKind::Query,
            SsdpKind::Response => PacketKind::Response,
            SsdpKind::Alive | SsdpKind::Byebye | SsdpKind::Update => PacketKind::Announce,
        };
        let mut s = Subject {
            proto: Proto::Ssdp,
            in_iface,
            out_iface,
            src: Some(src),
            kind,
            rrtype: None,
            ..Default::default()
        };
        if let Some(t) = &msg.target {
            s.services.push(t.clone());
        }
        if let Some(u) = &msg.uuid {
            s.instances.push(u.clone());
        }
        if let Some(h) = &msg.location_host {
            s.hosts.push(h.clone());
        }
        // What the table knows about the uuid or the host fills in what the message left out
        // (a byebye carries no LOCATION, a search names only a type).
        for name in [msg.uuid.clone(), msg.location_host.clone()]
            .into_iter()
            .flatten()
        {
            let b = self.hosts.bundle_of(in_iface, &name);
            for x in b.services {
                if !s.services.contains(&x) {
                    s.services.push(x);
                }
            }
            for x in b.hosts {
                if !s.hosts.contains(&x) {
                    s.hosts.push(x);
                }
            }
        }
        self.chain.decide(&s)
    }

    fn emit_ssdp(
        &self,
        in_iface: &str,
        pkt: &Received,
        kind: String,
        msg: Option<&SsdpMessage>,
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
            proto: Proto::Ssdp,
            iface: in_iface.to_string(),
            src: pkt.src.to_string(),
            len: pkt.data.len(),
            kind,
            services: msg.and_then(|m| m.target.clone()).into_iter().collect(),
            instances: msg.and_then(|m| m.uuid.clone()).into_iter().collect(),
            hosts: msg
                .and_then(|m| m.location_host.clone())
                .into_iter()
                .collect(),
            out,
            reason: None,
        };
        let _ = self.events.send(ev);
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
            Request::RuleDump => match render_rules(&self.chain.to_set()) {
                Ok(toml) => Response::RulesToml {
                    path: self.rules_path.display().to_string(),
                    toml,
                },
                Err(e) => Response::Error {
                    message: format!("{e:#}"),
                },
            },
            Request::RuleSaved => {
                self.unsaved = false;
                info!(
                    rules = self.chain.rules.len(),
                    "rules saved to {} by the client",
                    self.rules_path.display()
                );
                Response::Ok
            }
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
            last_unparsable: self.last_unparsable_view(),
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
            proto: t.proto,
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
            last_unparsable: self.last_unparsable_view(),
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
    for q in &mut m.queries {
        q.set_mdns_unicast_response(false);
    }
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
        proto: Proto,
        family: Family,
        sent: Mutex<Vec<(Vec<u8>, u32)>>,
        unicast: Mutex<Vec<(Vec<u8>, SocketAddr, u32)>>,
        joined: Mutex<Vec<u32>>,
    }

    impl FakeSocket {
        fn new(family: Family) -> Arc<Self> {
            Self::with_proto(Proto::Mdns, family)
        }
        fn with_proto(proto: Proto, family: Family) -> Arc<Self> {
            Arc::new(Self {
                proto,
                family,
                sent: Mutex::new(vec![]),
                unicast: Mutex::new(vec![]),
                joined: Mutex::new(vec![]),
            })
        }
        fn take(&self) -> Vec<(Vec<u8>, u32)> {
            std::mem::take(&mut *self.sent.lock().unwrap())
        }
    }

    impl MdnsSocket for FakeSocket {
        fn proto(&self) -> Proto {
            self.proto
        }
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
        fn send_unicast(&self, data: &[u8], dst: SocketAddr, ifindex: u32) -> anyhow::Result<()> {
            self.unicast
                .lock()
                .unwrap()
                .push((data.to_vec(), dst, ifindex));
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
        fn set_read_timeout(&self, _timeout: Option<Duration>) -> anyhow::Result<()> {
            Ok(())
        }
    }

    /// The proxy's ephemeral socket: records the query it sent and answers once with what the
    /// test put in, from the interface the query went to.
    struct FakeEphemeral {
        proto: Proto,
        sent: Mutex<Vec<(Vec<u8>, u32)>>,
        answer: Vec<u8>,
        answered: std::sync::atomic::AtomicBool,
    }

    impl MdnsSocket for FakeEphemeral {
        fn proto(&self) -> Proto {
            self.proto
        }
        fn family(&self) -> Family {
            Family::V4
        }
        fn recv(&self) -> anyhow::Result<Received> {
            if self
                .answered
                .swap(true, std::sync::atomic::Ordering::SeqCst)
            {
                anyhow::bail!("timeout");
            }
            let (_, ifindex) = self.sent.lock().unwrap()[0].clone();
            Ok(Received {
                proto: self.proto,
                family: Family::V4,
                data: self.answer.clone(),
                src: SocketAddr::new("10.1.0.5".parse().unwrap(), 5353),
                ifindex,
                reply_to: None,
            })
        }
        fn send_multicast(&self, data: &[u8], ifindex: u32) -> anyhow::Result<()> {
            self.sent.lock().unwrap().push((data.to_vec(), ifindex));
            Ok(())
        }
        fn send_unicast(&self, _: &[u8], _: SocketAddr, _: u32) -> anyhow::Result<()> {
            unreachable!()
        }
        fn join(&self, _: u32) -> anyhow::Result<()> {
            Ok(())
        }
        fn leave(&self, _: u32) -> anyhow::Result<()> {
            Ok(())
        }
        fn set_read_timeout(&self, _: Option<Duration>) -> anyhow::Result<()> {
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

    fn no_factory() -> SocketFactory {
        Arc::new(|_, _, _| anyhow::bail!("no ephemeral sockets in this test"))
    }

    fn setup(rules: &str) -> (Reflector, Arc<FakeSocket>, TempDir) {
        setup_with(rules, no_factory()).0
    }

    fn setup_with(
        rules: &str,
        factory: SocketFactory,
    ) -> (
        (Reflector, Arc<FakeSocket>, TempDir),
        mpsc::Receiver<Received>,
    ) {
        let sock = FakeSocket::new(Family::V4);
        let (cfg, dir) = config(&["lan", "iot", "guest"], rules);
        let (tx, rx) = mpsc::channel(16);
        let mut r = Reflector::new(&cfg, "test".into(), vec![sock.clone()], factory, tx).unwrap();
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
        ((r, sock, dir), rx)
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
            proto: Proto::Mdns,
            family: Family::V4,
            data: data.to_vec(),
            src: SocketAddr::new(src.parse().unwrap(), 5353),
            ifindex,
            reply_to: None,
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
    fn unparsable_says_why_in_watch_status_and_a_limited_log() {
        let (mut r, _sock, _d) = setup(ACCEPT_ALL);
        r.unparsable_log_window = Duration::from_millis(100);
        let mut events = r.events.subscribe();
        assert!(r.on_packet(packet(10, "10.0.0.7", b"not dns")).is_empty());
        let ev = events.try_recv().unwrap();
        assert_eq!(ev.kind, "unparsable");
        let reason = ev.reason.expect("the decode error");
        assert!(!reason.is_empty());
        let last = r.status().last_unparsable.expect("remembered");
        assert_eq!(
            (last.iface.as_str(), last.src.as_str()),
            ("lan", "10.0.0.7:5353")
        );
        assert_eq!(last.reason, reason);
        assert_eq!(r.stats().last_unparsable, Some(last));

        // The SSDP path says why too.
        r.on_packet(ssdp_packet(10, "10.0.0.8", 1900, "garbage\r\n\r\n"));
        let ev = events.try_recv().unwrap();
        assert_eq!((ev.kind.as_str(), ev.proto), ("unparsable", Proto::Ssdp));
        assert!(ev.reason.is_some());

        // The info line: once per source and interface per window, then with the count since.
        let ip: IpAddr = "10.0.0.9".parse().unwrap();
        let t = Instant::now();
        assert_eq!(r.unparsable_log_due(ip, "lan", t), Some(0));
        assert_eq!(r.unparsable_log_due(ip, "lan", t), None);
        assert_eq!(r.unparsable_log_due(ip, "lan", t), None);
        assert_eq!(r.unparsable_log_due(ip, "iot", t), Some(0), "per interface");
        let later = t + Duration::from_millis(150);
        assert_eq!(r.unparsable_log_due(ip, "lan", later), Some(2));
        assert_eq!(r.unparsable_log_due(ip, "lan", later), None);
    }

    #[test]
    fn check_rules_loads_and_compiles_like_start() {
        let d = TempDir::new();
        let f = d.path.join("rules.toml");
        assert!(check_rules(&f)
            .unwrap()
            .contains("no rules file: default drop"));
        std::fs::write(&f, HOMEKIT).unwrap();
        assert!(check_rules(&f)
            .unwrap()
            .ends_with("ok (2 rules, default drop)"));
        for broken in [
            "default = \"acept\"\n",
            "[[rule]]\nkind = [\"anounce\"]\naction = \"accept\"\n",
            "[[rule]]\naction = \"maybe\"\n",
            "[[rule]]\nrrtype = [\"BOGUS\"]\naction = \"accept\"\n",
            "[[rule]]\nsrc = [\"10.0.0.0/33\"]\naction = \"accept\"\n",
            "default = \"accept\"\n[[rule\n",
        ] {
            std::fs::write(&f, broken).unwrap();
            assert!(check_rules(&f).is_err(), "must fail: {broken:?}");
        }
    }

    #[test]
    fn empty_txt_is_forwarded_as_is_or_rebuilt_with_one_zero_byte() {
        // B-4: accept-all forwards the original bytes, empty TXT and all.
        let (mut r, sock, _d) = setup(ACCEPT_ALL);
        let raw = crate::packet::empty_txt_announcement(1);
        assert_eq!(r.on_packet(packet(11, "10.1.0.9", &raw)).len(), 2);
        assert_eq!(r.counters.dropped_unparsable, 0);
        let sent = sock.take();
        assert_eq!(sent.len(), 2);
        assert!(
            sent.iter().all(|(b, _)| *b == raw),
            "forwarded byte-identical"
        );

        // A rule drops the A record: the packet is rebuilt, the TXT goes out as one zero byte
        // (RFC 6763 §6.1, as avahi's reflector sends it) and parses strictly.
        let (mut r, sock, _d) =
            setup("default = \"accept\"\n[[rule]]\nrrtype = [\"A\"]\naction = \"drop\"\n");
        assert_eq!(r.on_packet(packet(11, "10.1.0.9", &raw)).len(), 2);
        let (out, _) = sock.take().remove(0);
        let m = Message::from_vec(&out).expect("the rebuild is strictly valid");
        let txt = m
            .answers
            .iter()
            .find(|rec| rec.record_type() == RecordType::TXT)
            .expect("TXT kept");
        let mut buf = Vec::new();
        let mut enc = hickory_proto::serialize::binary::BinEncoder::new(&mut buf);
        hickory_proto::serialize::binary::BinEncodable::emit(&txt.data, &mut enc).unwrap();
        assert_eq!(buf, [0u8], "one empty string");
        assert!(m.additionals.is_empty(), "the A record was dropped");
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
        // The same bytes over IPv6 at the same moment are avahi's second copy, not a duplicate (B-2).
        let mut twin = packet(10, "10.0.0.7", &q);
        twin.family = Family::V6;
        twin.src = SocketAddr::new("fd00:1::7".parse().unwrap(), 5353);
        r.on_packet(twin);
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
        // The client-side save (what discoctl does): dump, write, acknowledge.
        let Response::RulesToml { path, toml } = r.handle(Request::RuleDump) else {
            panic!()
        };
        assert_eq!(path, d.path.join("rules.toml").display().to_string());
        assert!(toml.contains("printer*"), "{toml}");
        let Response::Rules(list) = r.handle(Request::RuleList) else {
            panic!()
        };
        assert!(list.unsaved, "a dump alone does not count as saved");
        assert_eq!(r.handle(Request::RuleSaved), Response::Ok);
        let Response::Rules(list) = r.handle(Request::RuleList) else {
            panic!()
        };
        assert!(!list.unsaved);
        // The daemon-side save, for clients that still ask for it.
        assert_eq!(r.handle(Request::RuleSave), Response::Ok);
        let text = std::fs::read_to_string(d.path.join("rules.toml")).unwrap();
        assert!(text.contains("printer*"), "{text}");
        assert_eq!(text, toml);
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
    fn strips_link_local_addresses_and_clears_qu() {
        let (mut r, sock, _d) = setup(ACCEPT_ALL);
        let mut m = Message::new(0, MessageType::Response, OpCode::Query);
        m.add_answer(rr(
            "bridge.local",
            RData::A(A("10.1.0.5".parse().unwrap())),
            true,
        ));
        m.add_answer(rr(
            "bridge.local",
            RData::AAAA(hickory_proto::rr::rdata::AAAA("fe80::1".parse().unwrap())),
            true,
        ));
        m.add_answer(rr(
            "bridge.local",
            RData::AAAA(hickory_proto::rr::rdata::AAAA("fd00::1".parse().unwrap())),
            true,
        ));
        m.add_answer(rr(
            "bridge.local",
            RData::A(A("169.254.1.1".parse().unwrap())),
            true,
        ));
        assert_eq!(
            r.on_packet(packet(11, "10.1.0.5", &m.to_vec().unwrap()))
                .len(),
            2
        );
        let out = Packet::parse(&sock.take()[0].0).unwrap();
        assert_eq!(
            out.addresses(),
            vec![
                "10.1.0.5".parse::<IpAddr>().unwrap(),
                "fd00::1".parse::<IpAddr>().unwrap()
            ]
        );
        assert_eq!(r.counters.dropped_link_local, 4, "two records, two targets");
        assert_eq!(
            r.counters.records_dropped, 0,
            "link-local is not a rule drop"
        );
        // A QU question is forwarded without the bit.
        let mut m = Message::new(7, MessageType::Query, OpCode::Query);
        let mut q = Query::query(name("_hap._tcp.local"), RecordType::PTR);
        q.set_mdns_unicast_response(true);
        m.add_query(q);
        assert_eq!(
            r.on_packet(packet(10, "10.0.0.7", &m.to_vec().unwrap()))
                .len(),
            2
        );
        let out = Packet::parse(&sock.take()[0].0).unwrap();
        assert!(!out.questions[0].unicast);
        assert_eq!(out.message.metadata.id, 7, "the id stays");
    }

    #[test]
    fn legacy_query_is_proxied_and_the_answer_relayed_by_unicast() {
        let answer = announce("Living Room", "_hap._tcp", "bridge", "10.1.0.5");
        let eph = Arc::new(FakeEphemeral {
            proto: Proto::Mdns,
            sent: Mutex::new(vec![]),
            answer,
            answered: Default::default(),
        });
        let eph2 = eph.clone();
        let factory: SocketFactory = Arc::new(move |proto, family, ephemeral| {
            assert!(ephemeral && family == Family::V4 && proto == Proto::Mdns);
            Ok(eph2.clone() as Arc<dyn MdnsSocket>)
        });
        let ((mut r, sock, _d), mut rx) = setup_with(HOMEKIT, factory);
        // dig-style: a PTR question from port 40000 on lan.
        let mut q = packet(10, "10.0.0.7", &browse("_hap._tcp"));
        q.src.set_port(40000);
        assert_eq!(
            r.on_packet(q),
            vec!["iot"],
            "asked on iot only (rule 1), guest is dropped"
        );
        assert_eq!(r.counters.proxied_queries, 1);
        assert_eq!(
            eph.sent.lock().unwrap().len(),
            1,
            "the query went out from the ephemeral socket"
        );
        assert_eq!(eph.sent.lock().unwrap()[0].1, 11);
        // The proxy thread delivers the bridge's answer tagged for the querier.
        let relayed = rx.blocking_recv().expect("an answer from the proxy");
        assert_eq!(
            relayed.reply_to,
            Some(ReplyTo {
                addr: "10.0.0.7:40000".parse().unwrap(),
                ifindex: 10
            })
        );
        assert_eq!(r.on_packet(relayed), vec!["lan"]);
        let uni = sock.unicast.lock().unwrap();
        assert_eq!(uni.len(), 1);
        assert_eq!(uni[0].1, "10.0.0.7:40000".parse::<SocketAddr>().unwrap());
        assert_eq!(uni[0].2, 10);
        assert_eq!(r.counters.proxied_answers, 1);
        assert!(sock.sent.lock().unwrap().is_empty(), "nothing multicast");
        // With the window at 0, a legacy query is dropped.
        let cfg: Config = toml::from_str("interfaces = [\"a\", \"b\"]\nlegacy_window_ms = 0\nrules_file = \"/nonexistent/x.toml\"").unwrap();
        let (tx, _rx) = mpsc::channel(1);
        let mut r2 = Reflector::new(
            &cfg,
            "t".into(),
            vec![FakeSocket::new(Family::V4)],
            no_factory(),
            tx,
        )
        .unwrap();
        r2.on_iface_event(IfaceEvent::Link {
            index: 1,
            name: "a".into(),
            up: true,
        });
        r2.on_iface_event(IfaceEvent::Link {
            index: 2,
            name: "b".into(),
            up: true,
        });
        let mut q = packet(1, "10.0.0.7", &browse("_hap._tcp"));
        q.src.set_port(40000);
        assert!(r2.on_packet(q).is_empty());
    }

    const SSDP_RULES: &str = r#"
default = "drop"
[[rule]]
proto = ["ssdp"]
in = "lan"
out = "iot"
kind = ["query"]
action = "accept"
[[rule]]
proto = ["ssdp"]
in = "iot"
out = "lan"
kind = ["announce", "response"]
service = ["urn:schemas-upnp-org:device:mediarenderer:1"]
action = "accept"
"#;

    fn ssdp_packet(ifindex: u32, src: &str, port: u16, text: &str) -> Received {
        Received {
            proto: Proto::Ssdp,
            family: Family::V4,
            data: text.as_bytes().to_vec(),
            src: SocketAddr::new(src.parse().unwrap(), port),
            ifindex,
            reply_to: None,
        }
    }

    const ALIVE_RENDERER: &str = "NOTIFY * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nCACHE-CONTROL: max-age=1800\r\nLOCATION: http://10.1.0.40:1400/xml\r\nNT: urn:schemas-upnp-org:device:MediaRenderer:1\r\nNTS: ssdp:alive\r\nUSN: uuid:rincon_1::urn:schemas-upnp-org:device:MediaRenderer:1\r\n\r\n";
    const ALIVE_CAMERA: &str = "NOTIFY * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nCACHE-CONTROL: max-age=1800\r\nLOCATION: http://10.1.0.41:80/d.xml\r\nNT: urn:schemas-upnp-org:device:Basic:1\r\nNTS: ssdp:alive\r\nUSN: uuid:cam_1::urn:schemas-upnp-org:device:Basic:1\r\n\r\n";
    const BYEBYE_RENDERER: &str = "NOTIFY * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nNT: upnp:rootdevice\r\nNTS: ssdp:byebye\r\nUSN: uuid:rincon_1::upnp:rootdevice\r\n\r\n";
    const SEARCH_ALL: &str = "M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: 1\r\nST: ssdp:all\r\n\r\n";
    const SEARCH_ANSWER: &str = "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=1800\r\nEXT:\r\nLOCATION: http://10.1.0.40:1400/xml\r\nST: urn:schemas-upnp-org:device:MediaRenderer:1\r\nUSN: uuid:rincon_1::urn:schemas-upnp-org:device:MediaRenderer:1\r\n\r\n";

    #[test]
    fn ssdp_notifications_follow_the_rules_and_the_table() {
        let ssdp = FakeSocket::with_proto(Proto::Ssdp, Family::V4);
        let mdns = FakeSocket::new(Family::V4);
        let (cfg, _d) = config(&["lan", "iot", "guest"], SSDP_RULES);
        let (tx, _rx) = mpsc::channel(16);
        let mut r = Reflector::new(
            &cfg,
            "test".into(),
            vec![mdns.clone(), ssdp.clone()],
            no_factory(),
            tx,
        )
        .unwrap();
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
        let mut events = r.events.subscribe();
        // The renderer's alive crosses to lan, the camera's does not.
        assert_eq!(
            r.on_packet(ssdp_packet(11, "10.1.0.40", 1900, ALIVE_RENDERER)),
            vec!["lan"]
        );
        assert!(r
            .on_packet(ssdp_packet(11, "10.1.0.41", 1900, ALIVE_CAMERA))
            .is_empty());
        assert_eq!(ssdp.take(), vec![(ALIVE_RENDERER.as_bytes().to_vec(), 10)]);
        assert!(mdns.take().is_empty());
        // The byebye names only upnp:rootdevice; the table knows the uuid is the renderer.
        assert_eq!(
            r.on_packet(ssdp_packet(11, "10.1.0.40", 1900, BYEBYE_RENDERER)),
            vec!["lan"]
        );
        // An mDNS packet is not touched by ssdp-only rules: the chain's default drops it.
        assert!(r
            .on_packet(packet(10, "10.0.0.7", &browse("_hap._tcp")))
            .is_empty());
        let ev = events.try_recv().unwrap();
        assert_eq!((ev.proto, ev.kind.as_str()), (Proto::Ssdp, "ssdp-alive"));
        assert_eq!(ev.instances, vec!["uuid:rincon_1"]);
        assert_eq!(ev.hosts, vec!["10.1.0.40"]);
        assert_eq!(ev.out[0].result, "sent");
        // The byebye expired the renderer; the next sweep removes it.
        r.hosts.expire(Instant::now() + Duration::from_secs(1));
        let services = r.hosts.services(Instant::now());
        assert_eq!(
            services.iter().filter(|s| s.iface == "iot").count(),
            1,
            "the renderer said goodbye: {services:?}"
        );
    }

    #[test]
    fn ssdp_search_is_proxied_and_the_answer_relayed() {
        let eph = Arc::new(FakeEphemeral {
            proto: Proto::Ssdp,
            sent: Mutex::new(vec![]),
            answer: SEARCH_ANSWER.as_bytes().to_vec(),
            answered: Default::default(),
        });
        let eph2 = eph.clone();
        let factory: SocketFactory = Arc::new(move |proto, _family, ephemeral| {
            assert!(ephemeral && proto == Proto::Ssdp);
            Ok(eph2.clone() as Arc<dyn MdnsSocket>)
        });
        let ssdp = FakeSocket::with_proto(Proto::Ssdp, Family::V4);
        let (cfg, _d) = config(&["lan", "iot", "guest"], SSDP_RULES);
        let (tx, mut rx) = mpsc::channel(16);
        let mut r = Reflector::new(&cfg, "test".into(), vec![ssdp.clone()], factory, tx).unwrap();
        for (i, n) in ["lan", "iot", "guest"].iter().enumerate() {
            r.on_iface_event(IfaceEvent::Link {
                index: i as u32 + 10,
                name: n.to_string(),
                up: true,
            });
        }
        // A search from lan (always from an ephemeral port in SSDP): asked on iot, not on guest.
        assert_eq!(
            r.on_packet(ssdp_packet(10, "10.0.0.7", 50000, SEARCH_ALL)),
            vec!["iot"]
        );
        assert_eq!(r.counters.proxied_queries, 1);
        assert_eq!(eph.sent.lock().unwrap()[0].1, 11);
        assert!(
            ssdp.take().is_empty(),
            "a search is never multicast by the main socket"
        );
        let answer = rx.blocking_recv().expect("the renderer's answer");
        assert_eq!(
            answer.reply_to.map(|r| r.addr),
            Some("10.0.0.7:50000".parse().unwrap())
        );
        assert_eq!(r.on_packet(answer), vec!["lan"]);
        let uni = ssdp.unicast.lock().unwrap();
        assert_eq!(uni[0].1, "10.0.0.7:50000".parse::<SocketAddr>().unwrap());
        assert_eq!(r.counters.proxied_answers, 1);
        // A stray 200 OK on the multicast socket is not forwarded.
        assert!(r
            .on_packet(ssdp_packet(11, "10.1.0.40", 1900, SEARCH_ANSWER))
            .is_empty());
        // A LOCATION on a link-local address is dropped before the rules.
        let ll = ALIVE_RENDERER.replace("http://10.1.0.40:1400", "http://169.254.7.7:1400");
        assert!(r
            .on_packet(ssdp_packet(11, "10.1.0.40", 1900, &ll))
            .is_empty());
        assert_eq!(r.counters.dropped_link_local, 1);
    }

    #[test]
    fn missing_rules_file_means_drop_everything() {
        let sock = FakeSocket::new(Family::V4);
        let cfg: Config = toml::from_str(
            "interfaces = [\"a\", \"b\"]\nrules_file = \"/nonexistent/discowall/rules.toml\"",
        )
        .unwrap();
        let (tx, _rx) = mpsc::channel(1);
        let mut r = Reflector::new(&cfg, "test".into(), vec![sock], no_factory(), tx).unwrap();
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
