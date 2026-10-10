//! The rule chain (D-5): ordered, first match wins, a default policy. A rule is compiled once from
//! its `RuleSpec` (globs, address prefixes, record types) and evaluated per question or per record
//! with the names the packet and the host table attribute to it (research 3.4, 3.5).

use anyhow::{bail, Context, Result};
use discowall_proto::{Action, PacketKind, Proto, RuleList, RuleSet, RuleSpec, RuleView};
use hickory_proto::rr::RecordType;
use std::net::IpAddr;
use std::str::FromStr;

/// A `*`/`?` glob, matched case-insensitively.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Glob(Vec<char>);

impl Glob {
    pub fn new(pattern: &str) -> Self {
        Self(pattern.to_lowercase().chars().collect())
    }

    pub fn matches(&self, text: &str) -> bool {
        let text: Vec<char> = text.to_lowercase().chars().collect();
        glob_match(&self.0, &text)
    }
}

fn glob_match(p: &[char], t: &[char]) -> bool {
    match (p.first(), t.first()) {
        (None, None) => true,
        (None, Some(_)) => false,
        (Some('*'), _) => glob_match(&p[1..], t) || (!t.is_empty() && glob_match(p, &t[1..])),
        (Some('?'), Some(_)) => glob_match(&p[1..], &t[1..]),
        (Some(a), Some(b)) => a == b && glob_match(&p[1..], &t[1..]),
        (Some(_), None) => false,
    }
}

/// An address or a prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Prefix {
    addr: IpAddr,
    len: u8,
}

impl FromStr for Prefix {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        let (addr, len) = match s.split_once('/') {
            Some((a, l)) => (a, Some(l)),
            None => (s, None),
        };
        let addr: IpAddr = addr
            .parse()
            .with_context(|| format!("not an address: {addr}"))?;
        let max = if addr.is_ipv4() { 32 } else { 128 };
        let len = match len {
            Some(l) => l
                .parse::<u8>()
                .ok()
                .filter(|l| *l <= max)
                .with_context(|| format!("not a prefix length: /{l}"))?,
            None => max,
        };
        Ok(Self { addr, len })
    }
}

impl Prefix {
    pub fn contains(&self, ip: IpAddr) -> bool {
        match (self.addr, ip) {
            (IpAddr::V4(p), IpAddr::V4(a)) => {
                let mask = if self.len == 0 {
                    0
                } else {
                    u32::MAX << (32 - self.len)
                };
                (u32::from(p) & mask) == (u32::from(a) & mask)
            }
            (IpAddr::V6(p), IpAddr::V6(a)) => {
                let mask = if self.len == 0 {
                    0
                } else {
                    u128::MAX << (128 - self.len)
                };
                (u128::from(p) & mask) == (u128::from(a) & mask)
            }
            _ => false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub spec: RuleSpec,
    src: Vec<Prefix>,
    service: Vec<Glob>,
    instance: Vec<Glob>,
    host: Vec<Glob>,
    rrtype: Vec<RecordType>,
    pub hits: u64,
}

/// What one question or record looks like to the chain.
#[derive(Debug, Clone, Default)]
pub struct Subject<'a> {
    pub proto: Proto,
    pub in_iface: &'a str,
    pub out_iface: &'a str,
    pub src: Option<IpAddr>,
    pub kind: PacketKind,
    /// Service types attributed to the subject (`_hap._tcp`).
    pub services: Vec<String>,
    /// Instance names without their service tail (`living room`).
    pub instances: Vec<String>,
    /// Host names without `.local` (`bridge`).
    pub hosts: Vec<String>,
    pub rrtype: Option<RecordType>,
}

impl Rule {
    pub fn compile(spec: RuleSpec) -> Result<Self> {
        let src = spec
            .src
            .iter()
            .map(|s| s.parse())
            .collect::<Result<Vec<Prefix>>>()?;
        let rrtype = spec
            .rrtype
            .iter()
            .map(|t| {
                RecordType::from_str(&t.to_uppercase())
                    .map_err(|_| anyhow::anyhow!("not a record type: {t}"))
            })
            .collect::<Result<Vec<_>>>()?;
        if let Some(i) = &spec.in_iface {
            if i.is_empty() || i == "any" {
                bail!("`in` must be an interface name (leave it out for any)");
            }
        }
        if let Some(o) = &spec.out {
            if o.is_empty() || o == "any" {
                bail!("`out` must be an interface name (leave it out for any)");
            }
        }
        Ok(Self {
            src,
            service: spec.service.iter().map(|s| Glob::new(s)).collect(),
            instance: spec.instance.iter().map(|s| Glob::new(s)).collect(),
            host: spec.host.iter().map(|s| Glob::new(s)).collect(),
            rrtype,
            spec,
            hits: 0,
        })
    }

    pub fn matches(&self, s: &Subject) -> bool {
        if !self.spec.proto.is_empty() && !self.spec.proto.contains(&s.proto) {
            return false;
        }
        if self
            .spec
            .in_iface
            .as_deref()
            .is_some_and(|i| i != s.in_iface)
        {
            return false;
        }
        if self.spec.out.as_deref().is_some_and(|o| o != s.out_iface) {
            return false;
        }
        if !self.src.is_empty() {
            match s.src {
                Some(ip) if self.src.iter().any(|p| p.contains(ip)) => {}
                _ => return false,
            }
        }
        if !self.spec.kind.is_empty() && !self.spec.kind.contains(&s.kind) {
            return false;
        }
        if !self.rrtype.is_empty() && !s.rrtype.is_some_and(|t| self.rrtype.contains(&t)) {
            return false;
        }
        let any = |globs: &[Glob], names: &[String]| {
            globs.is_empty() || names.iter().any(|n| globs.iter().any(|g| g.matches(n)))
        };
        any(&self.service, &s.services)
            && any(&self.instance, &s.instances)
            && any(&self.host, &s.hosts)
    }
}

#[derive(Debug, Clone)]
pub struct Chain {
    pub default: Action,
    pub rules: Vec<Rule>,
    pub default_hits: u64,
}

/// The outcome for one subject: the action and the 1-based rule index, `None` for the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Verdict {
    pub action: Action,
    pub rule: Option<usize>,
    pub log: bool,
}

impl Chain {
    pub fn compile(set: &RuleSet) -> Result<Self> {
        let rules = set
            .rules
            .iter()
            .enumerate()
            .map(|(i, r)| Rule::compile(r.clone()).with_context(|| format!("rule {}", i + 1)))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            default: set.default,
            rules,
            default_hits: 0,
        })
    }

    pub fn empty(default: Action) -> Self {
        Self {
            default,
            rules: vec![],
            default_hits: 0,
        }
    }

    /// Evaluates and counts.
    pub fn decide(&mut self, s: &Subject) -> Verdict {
        for (i, r) in self.rules.iter_mut().enumerate() {
            if r.matches(s) {
                r.hits += 1;
                return Verdict {
                    action: r.spec.action,
                    rule: Some(i + 1),
                    log: r.spec.log,
                };
            }
        }
        self.default_hits += 1;
        Verdict {
            action: self.default,
            rule: None,
            log: false,
        }
    }

    /// Evaluates without counting (`rule test`).
    pub fn peek(&self, s: &Subject) -> Verdict {
        for (i, r) in self.rules.iter().enumerate() {
            if r.matches(s) {
                return Verdict {
                    action: r.spec.action,
                    rule: Some(i + 1),
                    log: r.spec.log,
                };
            }
        }
        Verdict {
            action: self.default,
            rule: None,
            log: false,
        }
    }

    pub fn insert(&mut self, spec: RuleSpec, at: Option<usize>) -> Result<usize> {
        let rule = Rule::compile(spec)?;
        let pos = match at {
            Some(0) => bail!("positions are 1-based"),
            Some(n) if n > self.rules.len() + 1 => bail!(
                "position {n} is beyond the end ({} rules)",
                self.rules.len()
            ),
            Some(n) => n - 1,
            None => self.rules.len(),
        };
        self.rules.insert(pos, rule);
        Ok(pos + 1)
    }

    pub fn remove(&mut self, index: usize) -> Result<RuleSpec> {
        if index == 0 || index > self.rules.len() {
            bail!("no rule {index} ({} rules)", self.rules.len());
        }
        Ok(self.rules.remove(index - 1).spec)
    }

    pub fn to_set(&self) -> RuleSet {
        RuleSet {
            default: self.default,
            rules: self.rules.iter().map(|r| r.spec.clone()).collect(),
        }
    }

    pub fn view(&self, unsaved: bool) -> RuleList {
        RuleList {
            default: self.default,
            rules: self
                .rules
                .iter()
                .enumerate()
                .map(|(i, r)| RuleView {
                    index: i + 1,
                    hits: r.hits,
                    spec: r.spec.clone(),
                })
                .collect(),
            unsaved,
        }
    }
}

/// `living room._hap._tcp.local` → `living room`; a host `bridge.local` → `bridge`.
pub fn short_instance(instance: &str) -> String {
    match crate::packet::service_type_of(instance) {
        Some(st) => instance
            .strip_suffix(&format!(".{st}.local"))
            .map(str::to_string)
            .unwrap_or_else(|| {
                // Another domain than .local: strip the service type and whatever follows.
                instance
                    .split(&format!(".{st}."))
                    .next()
                    .unwrap_or(instance)
                    .to_string()
            }),
        None => instance.to_string(),
    }
}

pub fn short_host(host: &str) -> String {
    host.strip_suffix(".local").unwrap_or(host).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs() {
        assert!(Glob::new("_hap._tcp").matches("_HAP._tcp"));
        assert!(Glob::new("_air*").matches("_airplay._tcp"));
        assert!(Glob::new("*").matches(""));
        assert!(Glob::new("printer?").matches("printer1"));
        assert!(!Glob::new("printer?").matches("printer"));
        assert!(!Glob::new("bridge").matches("bridge2"));
        assert!(Glob::new("*room*").matches("living room"));
    }

    #[test]
    fn prefixes() {
        let p: Prefix = "192.168.30.0/24".parse().unwrap();
        assert!(p.contains("192.168.30.17".parse().unwrap()));
        assert!(!p.contains("192.168.31.17".parse().unwrap()));
        assert!(!p.contains("fd00::1".parse().unwrap()));
        let h: Prefix = "192.168.30.17".parse().unwrap();
        assert!(h.contains("192.168.30.17".parse().unwrap()));
        assert!(!h.contains("192.168.30.18".parse().unwrap()));
        let v6: Prefix = "fd00::/8".parse().unwrap();
        assert!(v6.contains("fd12:3456::1".parse().unwrap()));
        assert!(!v6.contains("fe80::1".parse().unwrap()));
        let all: Prefix = "0.0.0.0/0".parse().unwrap();
        assert!(all.contains("1.2.3.4".parse().unwrap()));
        assert!("192.168.30.0/33".parse::<Prefix>().is_err());
        assert!("nope".parse::<Prefix>().is_err());
    }

    fn spec(text: &str) -> RuleSpec {
        toml::from_str(text).unwrap()
    }

    fn subject<'a>(in_iface: &'a str, out: &'a str, kind: PacketKind) -> Subject<'a> {
        Subject {
            in_iface,
            out_iface: out,
            kind,
            ..Default::default()
        }
    }

    #[test]
    fn homekit_example() {
        let set: RuleSet = toml::from_str(
            r#"
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
            log = true
            "#,
        )
        .unwrap();
        let mut chain = Chain::compile(&set).unwrap();
        let mut q = subject("lan", "iot", PacketKind::Query);
        q.services = vec!["_hap._tcp".into()];
        assert_eq!(
            chain.decide(&q),
            Verdict {
                action: Action::Accept,
                rule: Some(1),
                log: false
            }
        );
        q.services = vec!["_airplay._tcp".into()];
        assert_eq!(
            chain.decide(&q),
            Verdict {
                action: Action::Drop,
                rule: None,
                log: false
            }
        );
        let mut a = subject("iot", "lan", PacketKind::Announce);
        a.hosts = vec!["bridge".into()];
        a.services = vec!["_hap._tcp".into()];
        assert_eq!(
            chain.decide(&a),
            Verdict {
                action: Action::Accept,
                rule: Some(2),
                log: true
            }
        );
        a.hosts = vec!["camera".into()];
        assert_eq!(chain.decide(&a).action, Action::Drop);
        // The wrong direction: iot asking lan.
        let mut back = subject("iot", "lan", PacketKind::Query);
        back.services = vec!["_hap._tcp".into()];
        assert_eq!(chain.decide(&back).action, Action::Drop);
        assert_eq!(chain.rules[0].hits, 1);
        assert_eq!(chain.rules[1].hits, 1);
        assert_eq!(chain.default_hits, 3);
        let view = chain.view(true);
        assert_eq!(view.rules[1].index, 2);
        assert!(view.unsaved);
    }

    #[test]
    fn src_kind_rrtype_and_blacklist() {
        let set: RuleSet = toml::from_str(
            r#"
            default = "accept"
            [[rule]]
            src = ["192.168.30.0/24"]
            action = "drop"
            [[rule]]
            rrtype = ["AAAA"]
            action = "drop"
            "#,
        )
        .unwrap();
        let mut chain = Chain::compile(&set).unwrap();
        let mut s = subject("iot", "lan", PacketKind::Response);
        s.src = Some("192.168.30.5".parse().unwrap());
        assert_eq!(chain.decide(&s).rule, Some(1));
        s.src = Some("10.0.0.5".parse().unwrap());
        assert_eq!(
            chain.decide(&s),
            Verdict {
                action: Action::Accept,
                rule: None,
                log: false
            }
        );
        s.rrtype = Some(RecordType::AAAA);
        assert_eq!(chain.decide(&s).rule, Some(2));
        // A rule with a src list never matches a subject without a source.
        s.src = None;
        s.rrtype = None;
        assert_eq!(chain.decide(&s).rule, None);
    }

    #[test]
    fn proto_match() {
        let set: RuleSet =
            toml::from_str("default = \"drop\"\n[[rule]]\nproto = [\"ssdp\"]\naction = \"accept\"")
                .unwrap();
        let mut chain = Chain::compile(&set).unwrap();
        let mut s = subject("iot", "lan", PacketKind::Announce);
        assert_eq!(chain.decide(&s).action, Action::Drop);
        s.proto = Proto::Ssdp;
        assert_eq!(chain.decide(&s).action, Action::Accept);
        assert_eq!(chain.rules[0].spec.to_string(), "proto ssdp accept");
    }

    #[test]
    fn insert_remove_and_errors() {
        let mut chain = Chain::empty(Action::Drop);
        assert_eq!(chain.insert(spec("action = \"accept\""), None).unwrap(), 1);
        assert_eq!(
            chain
                .insert(spec("action = \"drop\"\nhost = [\"x\"]"), Some(1))
                .unwrap(),
            1
        );
        assert_eq!(chain.rules[0].spec.host, vec!["x"]);
        assert!(chain.insert(spec("action = \"drop\""), Some(0)).is_err());
        assert!(chain.insert(spec("action = \"drop\""), Some(4)).is_err());
        assert!(chain.remove(3).is_err());
        assert_eq!(chain.remove(1).unwrap().host, vec!["x"]);
        assert_eq!(chain.rules.len(), 1);
        assert!(Rule::compile(spec("action = \"drop\"\nsrc = [\"bogus\"]")).is_err());
        assert!(Rule::compile(spec("action = \"drop\"\nrrtype = [\"NOPE\"]")).is_err());
        assert!(Rule::compile(spec("action = \"drop\"\nin = \"any\"")).is_err());
        assert_eq!(chain.to_set().rules.len(), 1);
    }

    #[test]
    fn short_names() {
        assert_eq!(short_instance("living room._hap._tcp.local"), "living room");
        assert_eq!(short_instance("_hap._tcp.local"), "_hap._tcp.local");
        assert_eq!(short_host("bridge.local"), "bridge");
        assert_eq!(short_host("bridge"), "bridge");
    }
}
