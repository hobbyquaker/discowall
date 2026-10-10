//! discoctl: the command line of discowalld.

use anyhow::{bail, Context, Result};
use clap::{Args as ClapArgs, Parser, Subcommand};
use discowall_proto::{
    Action, HostView, PacketKind, Request, Response, RuleList, RuleSpec, ServiceView, StatsView,
    Status, TestResult, TestSpec, WatchEvent,
};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser, Debug)]
#[command(version, about = "control discowalld", after_help = RULE_SYNTAX)]
struct Cli {
    /// The daemon's control socket.
    #[arg(short, long, default_value = discowall_proto::DEFAULT_SOCKET, env = "DISCOWALL_SOCKET", global = true)]
    socket: PathBuf,
    /// Print the daemon's answer as JSON.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

const RULE_SYNTAX: &str = "\
Rule syntax (rule add / rule insert / rule test):
  [in <iface>] [out <iface>] [src <addr|prefix>[,…]] [kind query|probe|response|announce[,…]]
  [service <glob>[,…]] [instance <glob>[,…]] [host <glob>[,…]] [rrtype <type>[,…]]
  accept|drop [log] [comment <text>]
Every match left out matches everything; a list is any-of. Names are matched without their
'.local' / '._service._proto.local' tail. Example:
  discoctl rule add in lan out iot kind query service _hap._tcp accept
  discoctl rule add in iot out lan kind response,announce host bridge accept log";

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Interfaces, their state and the counters.
    Status,
    /// Is the daemon there?
    Ping,
    /// The rule chain.
    Rule(RuleCmd),
    /// Write the chain to the rules file.
    Save,
    /// Reload the chain from the rules file.
    Apply,
    /// The host table: what each interface has announced.
    Hosts,
    /// The service instances each interface has announced.
    Services,
    /// Counters per interface and per rule.
    Stats,
    /// Live: one line per packet with its decisions, until Ctrl-C.
    Watch {
        /// Only packets arriving on this interface.
        #[arg(short, long)]
        iface: Option<String>,
    },
}

#[derive(ClapArgs, Debug)]
struct RuleCmd {
    #[command(subcommand)]
    cmd: RuleSub,
}

#[derive(Subcommand, Debug)]
enum RuleSub {
    /// List the rules with their hit counters.
    List,
    /// Append a rule.
    Add {
        #[arg(required = true, trailing_var_arg = true)]
        tokens: Vec<String>,
    },
    /// Insert a rule before position N (1-based).
    Insert {
        n: usize,
        #[arg(required = true, trailing_var_arg = true)]
        tokens: Vec<String>,
    },
    /// Delete the rule at position N.
    Del { n: usize },
    /// Remove every rule (the default policy stays).
    Flush,
    /// What would happen to such a packet: `in <iface> out <iface> kind <kind> [src a] [service s] [instance i] [host h] [rrtype t]`.
    Test {
        #[arg(required = true, trailing_var_arg = true)]
        tokens: Vec<String>,
    },
}

/// Exit codes: 0 ok, 1 the daemon refused, 2 usage, 3 the daemon is unreachable.
fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("discoctl: {e:#}");
            ExitCode::from(if e.downcast_ref::<Usage>().is_some() {
                2
            } else {
                3
            })
        }
    }
}

#[derive(Debug)]
struct Usage(String);
impl std::fmt::Display for Usage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Usage {}

fn usage(msg: impl Into<String>) -> anyhow::Error {
    Usage(msg.into()).into()
}

fn call(socket: &PathBuf, req: &Request) -> Result<Response> {
    let stream =
        UnixStream::connect(socket).with_context(|| format!("connecting {}", socket.display()))?;
    let mut wr = stream.try_clone()?;
    let mut line = serde_json::to_string(req)?;
    line.push('\n');
    wr.write_all(line.as_bytes())?;
    let mut rd = BufReader::new(stream);
    let mut answer = String::new();
    if rd.read_line(&mut answer)? == 0 {
        bail!("the daemon closed the connection without an answer");
    }
    serde_json::from_str(&answer).context("the daemon's answer")
}

fn list(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(str::to_string)
        .collect()
}

/// `in lan out iot kind query service _hap._tcp accept log` → a RuleSpec.
fn parse_rule(tokens: &[String]) -> Result<RuleSpec> {
    let mut spec = RuleSpec::default();
    let mut action = None;
    let mut it = tokens.iter();
    while let Some(t) = it.next() {
        let mut value = || {
            it.next()
                .ok_or_else(|| usage(format!("`{t}` needs a value")))
        };
        match t.as_str() {
            "in" => spec.in_iface = Some(value()?.clone()),
            "out" => spec.out = Some(value()?.clone()),
            "src" => spec.src = list(value()?),
            "kind" => {
                spec.kind = list(value()?)
                    .iter()
                    .map(|k| k.parse::<PacketKind>().map_err(usage))
                    .collect::<Result<_>>()?
            }
            "service" => spec.service = list(value()?),
            "instance" => spec.instance = list(value()?),
            "host" => spec.host = list(value()?),
            "rrtype" => spec.rrtype = list(value()?),
            "comment" => spec.comment = Some(value()?.clone()),
            "accept" => action = Some(Action::Accept),
            "drop" => action = Some(Action::Drop),
            "log" => spec.log = true,
            other => {
                return Err(usage(format!(
                    "unknown word `{other}` (see --help for the rule syntax)"
                )))
            }
        }
    }
    spec.action = action.ok_or_else(|| usage("a rule ends with `accept` or `drop`"))?;
    Ok(spec)
}

fn parse_test(tokens: &[String]) -> Result<TestSpec> {
    let mut t = TestSpec::default();
    let mut kind = None;
    let mut it = tokens.iter();
    while let Some(k) = it.next() {
        let v = it
            .next()
            .ok_or_else(|| usage(format!("`{k}` needs a value")))?;
        match k.as_str() {
            "in" => t.in_iface = v.clone(),
            "out" => t.out = v.clone(),
            "src" => {
                t.src = Some(
                    v.parse()
                        .map_err(|_| usage(format!("not an address: {v}")))?,
                )
            }
            "kind" => kind = Some(v.parse::<PacketKind>().map_err(usage)?),
            "service" => t.service = Some(v.clone()),
            "instance" => t.instance = Some(v.clone()),
            "host" => t.host = Some(v.clone()),
            "rrtype" => t.rrtype = Some(v.clone()),
            other => return Err(usage(format!("unknown word `{other}`"))),
        }
    }
    if t.in_iface.is_empty() || t.out.is_empty() {
        return Err(usage("`in` and `out` are required"));
    }
    t.kind = kind.ok_or_else(|| usage("`kind` is required"))?;
    Ok(t)
}

fn run(cli: &Cli) -> Result<ExitCode> {
    let req = match &cli.cmd {
        Cmd::Status => Request::Status,
        Cmd::Ping => Request::Ping,
        Cmd::Save => Request::RuleSave,
        Cmd::Apply => Request::RuleApply,
        Cmd::Hosts => Request::Hosts,
        Cmd::Services => Request::Services,
        Cmd::Stats => Request::Stats,
        Cmd::Watch { iface } => return watch(cli, iface.clone()),
        Cmd::Rule(r) => match &r.cmd {
            RuleSub::List => Request::RuleList,
            RuleSub::Add { tokens } => Request::RuleAdd {
                rule: parse_rule(tokens)?,
                at: None,
            },
            RuleSub::Insert { n, tokens } => Request::RuleAdd {
                rule: parse_rule(tokens)?,
                at: Some(*n),
            },
            RuleSub::Del { n } => Request::RuleDel { index: *n },
            RuleSub::Flush => Request::RuleFlush,
            RuleSub::Test { tokens } => Request::RuleTest(parse_test(tokens)?),
        },
    };
    let resp = call(&cli.socket, &req)?;
    if cli.json {
        println!("{}", serde_json::to_string_pretty(&resp)?);
        return Ok(match resp {
            Response::Error { .. } => ExitCode::from(1),
            _ => ExitCode::SUCCESS,
        });
    }
    match resp {
        Response::Pong => println!("pong"),
        Response::Ok => {}
        Response::Status(s) => print_status(&s),
        Response::Rules(l) => print_rules(&l),
        Response::Test(t) => print_test(&t),
        Response::Hosts { hosts } => print_hosts(&hosts),
        Response::Services { services } => print_services(&services),
        Response::Stats(st) => print_stats(&st),
        Response::Event(ev) => print_event(&ev),
        Response::Error { message } => {
            eprintln!("discoctl: {message}");
            return Ok(ExitCode::from(1));
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn watch(cli: &Cli, iface: Option<String>) -> Result<ExitCode> {
    let stream = UnixStream::connect(&cli.socket)
        .with_context(|| format!("connecting {}", cli.socket.display()))?;
    let mut wr = stream.try_clone()?;
    let mut line = serde_json::to_string(&Request::Watch { iface })?;
    line.push('\n');
    wr.write_all(line.as_bytes())?;
    let rd = BufReader::new(stream);
    let stdout = std::io::stdout();
    for line in rd.lines() {
        let line = line?;
        let resp: Response = serde_json::from_str(&line).context("the daemon's answer")?;
        let mut out = stdout.lock();
        let result = if cli.json {
            writeln!(out, "{line}")
        } else {
            match resp {
                Response::Event(ev) => writeln!(out, "{}", format_event(&ev)),
                Response::Error { message } => writeln!(out, "! {message}"),
                _ => Ok(()),
            }
        };
        // A closed pipe (`discoctl watch | head`) ends the watch quietly.
        if result.is_err() {
            return Ok(ExitCode::SUCCESS);
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn format_event(ev: &WatchEvent) -> String {
    let secs = ev.ts_ms / 1000;
    let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
    let mut names: Vec<String> = Vec::new();
    names.extend(ev.services.iter().cloned());
    names.extend(ev.instances.iter().map(|i| format!("\"{i}\"")));
    names.extend(ev.hosts.iter().cloned());
    let out = ev
        .out
        .iter()
        .map(|d| {
            let rules = if d.rules.is_empty() {
                String::new()
            } else {
                format!(
                    "({})",
                    d.rules
                        .iter()
                        .map(|r| r.to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                )
            };
            let dropped = if d.dropped > 0 && d.result != "dropped" {
                format!("-{}", d.dropped)
            } else {
                String::new()
            };
            format!("{} {}{}{}", d.iface, d.result, rules, dropped)
        })
        .collect::<Vec<_>>()
        .join("  ");
    format!(
        "{h:02}:{m:02}:{s:02}.{:03} {:<8} {:<21} {:<9} {:<40} -> {}",
        ev.ts_ms % 1000,
        ev.iface,
        ev.src,
        ev.kind,
        names.join(" "),
        out
    )
}

fn print_event(ev: &WatchEvent) {
    println!("{}", format_event(ev));
}

fn print_services(services: &[ServiceView]) {
    println!(
        "{:<10} {:<22} {:<36} {:<24} {:>5} {:>7}",
        "iface", "service", "instance", "host", "port", "expires"
    );
    for s in services {
        println!(
            "{:<10} {:<22} {:<36} {:<24} {:>5} {:>6}s",
            s.iface,
            s.service,
            s.instance,
            s.host.as_deref().unwrap_or("-"),
            s.port.map(|p| p.to_string()).unwrap_or_else(|| "-".into()),
            s.expires_in_s
        );
    }
}

fn print_stats(st: &StatsView) {
    let c = &st.counters;
    println!(
        "up {}s  rx {}  tx {}  hosts {}  instances {}",
        st.uptime_s, c.rx, c.tx, st.hosts, st.instances
    );
    println!(
        "dropped: own-source {}  duplicate {}  unparsable {}  by-rules {}  unknown-interface {}  records {}  send-errors {}",
        c.dropped_own_source, c.dropped_duplicate, c.dropped_unparsable, c.dropped_by_rules, c.dropped_unknown_interface, c.records_dropped, c.send_errors
    );
    println!("{:<12} {:>10} {:>10}", "interface", "rx", "tx");
    for i in &st.interfaces {
        println!("{:<12} {:>10} {:>10}", i.name, i.rx, i.tx);
    }
    println!("{:>4} {:>10}  rule", "#", "hits");
    for r in &st.rules {
        println!("{:>4} {:>10}  {}", r.index, r.hits, r.rule);
    }
    println!("{:>4} {:>10}  default", "-", st.default_hits);
}

fn print_status(s: &Status) {
    println!(
        "discowalld {}  up {}s  config {}",
        s.version, s.uptime_s, s.config_path
    );
    println!(
        "{:<12} {:>5} {:<6} {:<6} {:>8} {:>8}  addresses",
        "interface", "index", "state", "joined", "rx", "tx"
    );
    for i in &s.interfaces {
        let index = i.index.map(|x| x.to_string()).unwrap_or_else(|| "-".into());
        let state = if i.index.is_none() {
            "absent"
        } else if i.up {
            "up"
        } else {
            "down"
        };
        let joined = match (i.joined_v4, i.joined_v6) {
            (true, true) => "v4 v6",
            (true, false) => "v4",
            (false, true) => "v6",
            (false, false) => "-",
        };
        let addrs = i
            .addresses
            .iter()
            .map(|a| a.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        println!(
            "{:<12} {:>5} {:<6} {:<6} {:>8} {:>8}  {}",
            i.name, index, state, joined, i.rx, i.tx, addrs
        );
    }
    let c = &s.counters;
    println!(
        "rx {}  tx {}  dropped: own-source {}  duplicate {}  unparsable {}  by-rules {}  unknown-interface {}  records-dropped {}  send-errors {}",
        c.rx, c.tx, c.dropped_own_source, c.dropped_duplicate, c.dropped_unparsable, c.dropped_by_rules, c.dropped_unknown_interface, c.records_dropped, c.send_errors
    );
}

fn print_rules(l: &RuleList) {
    println!(
        "default {}{}",
        l.default,
        if l.unsaved {
            "   (unsaved changes: `discoctl save`)"
        } else {
            ""
        }
    );
    for r in &l.rules {
        println!("{:>3}  [{:>6}]  {}", r.index, r.hits, r.spec);
    }
}

fn print_test(t: &TestResult) {
    match t.rule {
        Some(n) => println!("{} (rule {n})", t.action),
        None => println!("{} (default)", t.action),
    }
}

fn print_hosts(hosts: &[HostView]) {
    println!(
        "{:<10} {:<28} {:<32} {:>5} {:>7}  services",
        "iface", "host", "addresses", "seen", "expires"
    );
    for h in hosts {
        let addrs = h
            .addresses
            .iter()
            .map(|a| a.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        let services = h
            .instances
            .iter()
            .map(|i| match i.port {
                Some(p) => format!("{} ({}:{p})", i.service, i.instance),
                None => format!("{} ({})", i.service, i.instance),
            })
            .collect::<Vec<_>>()
            .join(", ");
        println!(
            "{:<10} {:<28} {:<32} {:>4}s {:>6}s  {}",
            h.iface, h.host, addrs, h.last_seen_s, h.expires_in_s, services
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn parses_the_homekit_rules() {
        let r = parse_rule(&toks("in lan out iot kind query service _hap._tcp accept")).unwrap();
        assert_eq!(r.in_iface.as_deref(), Some("lan"));
        assert_eq!(r.out.as_deref(), Some("iot"));
        assert_eq!(r.kind, vec![PacketKind::Query]);
        assert_eq!(r.service, vec!["_hap._tcp"]);
        assert_eq!(r.action, Action::Accept);
        assert!(!r.log);
        let r = parse_rule(&toks(
            "in iot out lan kind response,announce host bridge,printer* accept log",
        ))
        .unwrap();
        assert_eq!(r.kind, vec![PacketKind::Response, PacketKind::Announce]);
        assert_eq!(r.host, vec!["bridge", "printer*"]);
        assert!(r.log);
        assert_eq!(
            r.to_string(),
            "in iot out lan kind response,announce host bridge,printer* accept log"
        );
        let r = parse_rule(&toks("src 192.168.30.0/24,fd00::/8 rrtype AAAA drop")).unwrap();
        assert_eq!(r.src, vec!["192.168.30.0/24", "fd00::/8"]);
        assert_eq!(r.rrtype, vec!["AAAA"]);
    }

    #[test]
    fn rejects_bad_rules() {
        assert!(parse_rule(&toks("in lan")).is_err());
        assert!(parse_rule(&toks("in lan bogus accept")).is_err());
        assert!(parse_rule(&toks("kind nope accept")).is_err());
        assert!(parse_rule(&toks("in")).is_err());
    }

    #[test]
    fn parses_a_test() {
        let t = parse_test(&toks(
            "in iot out lan kind announce host bridge src 10.0.0.1",
        ))
        .unwrap();
        assert_eq!(t.kind, PacketKind::Announce);
        assert_eq!(t.host.as_deref(), Some("bridge"));
        assert_eq!(t.src, Some("10.0.0.1".parse().unwrap()));
        assert!(parse_test(&toks("in iot kind query")).is_err());
        assert!(parse_test(&toks("in iot out lan")).is_err());
    }
}
