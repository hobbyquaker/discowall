//! The control protocol between `discowalld` and `discoctl`: JSON lines over a unix socket, one
//! request per line, one response per line.

use serde::{Deserialize, Serialize};
use std::net::IpAddr;

/// The default path of the control socket.
pub const DEFAULT_SOCKET: &str = "/run/discowall/control.sock";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Ping,
    Status,
    RuleList,
    /// Append, or insert before the 1-based position `at`.
    RuleAdd {
        rule: RuleSpec,
        at: Option<usize>,
    },
    RuleDel {
        index: usize,
    },
    RuleFlush,
    /// Write the chain to the rules file.
    RuleSave,
    /// Reload the chain from the rules file.
    RuleApply,
    RuleTest(TestSpec),
    Hosts,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum Response {
    Pong,
    Status(Status),
    Rules(RuleList),
    Ok,
    Test(TestResult),
    Hosts { hosts: Vec<HostView> },
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Status {
    pub version: String,
    pub uptime_s: u64,
    pub config_path: String,
    pub interfaces: Vec<InterfaceStatus>,
    pub counters: Counters,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct InterfaceStatus {
    pub name: String,
    /// `None` while the interface does not exist.
    pub index: Option<u32>,
    pub up: bool,
    pub joined_v4: bool,
    pub joined_v6: bool,
    pub addresses: Vec<IpAddr>,
    pub rx: u64,
    pub tx: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Counters {
    pub rx: u64,
    pub tx: u64,
    pub dropped_own_source: u64,
    pub dropped_duplicate: u64,
    pub dropped_unknown_interface: u64,
    pub dropped_unparsable: u64,
    /// Packets of which nothing was allowed to cross to any interface.
    pub dropped_by_rules: u64,
    /// Questions and records removed from packets that still crossed.
    pub records_dropped: u64,
    pub send_errors: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trip() {
        let line = serde_json::to_string(&Request::Status).unwrap();
        assert_eq!(line, r#"{"cmd":"status"}"#);
        assert_eq!(
            serde_json::from_str::<Request>(&line).unwrap(),
            Request::Status
        );
    }

    #[test]
    fn rule_spec_display_and_toml() {
        let r = RuleSpec {
            in_iface: Some("iot".into()),
            out: Some("lan".into()),
            kind: vec![PacketKind::Response, PacketKind::Announce],
            host: vec!["bridge".into(), "living room*".into()],
            action: Action::Accept,
            log: true,
            ..Default::default()
        };
        assert_eq!(
            r.to_string(),
            r#"in iot out lan kind response,announce host bridge,"living room*" accept log"#
        );
        let set = RuleSet {
            default: Action::Drop,
            rules: vec![r.clone()],
        };
        let text = toml::to_string(&set).unwrap();
        assert!(text.contains("[[rule]]"), "{text}");
        assert!(text.contains(r#"in = "iot""#), "{text}");
        assert_eq!(toml::from_str::<RuleSet>(&text).unwrap(), set);
        assert_eq!(toml::from_str::<RuleSet>("").unwrap(), RuleSet::default());
    }

    #[test]
    fn response_round_trip() {
        let r = Response::Error {
            message: "x".into(),
        };
        let line = serde_json::to_string(&r).unwrap();
        assert_eq!(line, r#"{"result":"error","message":"x"}"#);
        assert_eq!(serde_json::from_str::<Response>(&line).unwrap(), r);
    }
}

// ---- rules (P1) ----------------------------------------------------------------------------------

/// What a rule does with what it matches.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Accept,
    #[default]
    Drop,
}

impl std::fmt::Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Action::Accept => "accept",
            Action::Drop => "drop",
        })
    }
}

/// The kind of packet a rule applies to.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum PacketKind {
    #[default]
    Query,
    Probe,
    Response,
    Announce,
}

impl std::fmt::Display for PacketKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            PacketKind::Query => "query",
            PacketKind::Probe => "probe",
            PacketKind::Response => "response",
            PacketKind::Announce => "announce",
        })
    }
}

impl std::str::FromStr for PacketKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "query" => Ok(Self::Query),
            "probe" => Ok(Self::Probe),
            "response" => Ok(Self::Response),
            "announce" => Ok(Self::Announce),
            _ => Err(format!(
                "not a packet kind: {s} (query, probe, response, announce)"
            )),
        }
    }
}

/// One rule as written in `rules.toml`, sent over the control socket and typed on the command line.
/// Every match field is optional; an absent field matches everything. Lists are "any of".
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct RuleSpec {
    /// Arriving interface.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "in")]
    pub in_iface: Option<String>,
    /// Target interface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub out: Option<String>,
    /// Original source address or prefix (`192.168.30.17`, `192.168.30.0/24`, `fd00::/8`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub src: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kind: Vec<PacketKind>,
    /// Service types, glob (`_hap._tcp`, `_air*`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub service: Vec<String>,
    /// Instance names, glob (`living room*`); matched without the `._service._proto.local` tail.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instance: Vec<String>,
    /// Host names, glob (`bridge`, `printer*`); matched without `.local`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub host: Vec<String>,
    /// Record types (`A`, `AAAA`, `PTR`, `SRV`, `TXT`, …); for a query, the question's type.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rrtype: Vec<String>,
    pub action: Action,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub log: bool,
    /// A free comment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

impl std::fmt::Display for RuleSpec {
    /// The command-line form: `in lan out iot kind query service _hap._tcp accept`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut parts: Vec<String> = Vec::new();
        if let Some(i) = &self.in_iface {
            parts.push(format!("in {i}"));
        }
        if let Some(o) = &self.out {
            parts.push(format!("out {o}"));
        }
        if !self.src.is_empty() {
            parts.push(format!("src {}", self.src.join(",")));
        }
        if !self.kind.is_empty() {
            parts.push(format!(
                "kind {}",
                self.kind
                    .iter()
                    .map(|k| k.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        for (key, list) in [
            ("service", &self.service),
            ("instance", &self.instance),
            ("host", &self.host),
            ("rrtype", &self.rrtype),
        ] {
            if !list.is_empty() {
                parts.push(format!(
                    "{key} {}",
                    list.iter().map(|s| quote(s)).collect::<Vec<_>>().join(",")
                ));
            }
        }
        parts.push(self.action.to_string());
        if self.log {
            parts.push("log".into());
        }
        if let Some(c) = &self.comment {
            parts.push(format!("comment {}", quote(c)));
        }
        f.write_str(&parts.join(" "))
    }
}

fn quote(s: &str) -> String {
    if s.is_empty() || s.contains([' ', ',', '"']) {
        format!("{:?}", s)
    } else {
        s.to_string()
    }
}

/// The rules file and the chain as the daemon reports it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RuleSet {
    /// What happens to a packet no rule matches.
    #[serde(default)]
    pub default: Action,
    #[serde(default, rename = "rule")]
    pub rules: Vec<RuleSpec>,
}

impl Default for RuleSet {
    fn default() -> Self {
        Self {
            default: Action::Drop,
            rules: vec![],
        }
    }
}

/// A rule with its position and hit counter, as `rule list` shows it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuleView {
    /// 1-based position in the chain.
    pub index: usize,
    pub hits: u64,
    #[serde(flatten)]
    pub spec: RuleSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RuleList {
    pub default: Action,
    pub rules: Vec<RuleView>,
    /// True when the chain differs from the rules file on disk (edited, not saved).
    pub unsaved: bool,
}

/// `rule test`: what would happen to such a packet, record by record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct TestSpec {
    #[serde(rename = "in")]
    pub in_iface: String,
    pub out: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub src: Option<IpAddr>,
    pub kind: PacketKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rrtype: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TestResult {
    pub action: Action,
    /// The 1-based index of the rule that decided, or `None` for the default policy.
    pub rule: Option<usize>,
}

/// One entry of the host table.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostView {
    pub iface: String,
    pub host: String,
    pub addresses: Vec<IpAddr>,
    pub instances: Vec<InstanceView>,
    pub last_seen_s: u64,
    pub expires_in_s: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstanceView {
    pub instance: String,
    pub service: String,
    pub port: Option<u16>,
}
