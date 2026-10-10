//! discoctl: the command line of discowalld.

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use discowall_proto::{Request, Response, Status};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser, Debug)]
#[command(version, about = "control discowalld")]
struct Args {
    /// The daemon's control socket.
    #[arg(short, long, default_value = discowall_proto::DEFAULT_SOCKET, env = "DISCOWALL_SOCKET")]
    socket: PathBuf,
    /// Print the daemon's answer as JSON.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Interfaces, their state and the counters.
    Status,
    /// Is the daemon there?
    Ping,
}

/// Exit codes: 0 ok, 1 the daemon refused, 2 usage (clap), 3 the daemon is unreachable.
fn main() -> ExitCode {
    let args = Args::parse();
    match run(&args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("discoctl: {e:#}");
            ExitCode::from(3)
        }
    }
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

fn run(args: &Args) -> Result<ExitCode> {
    let req = match args.cmd {
        Cmd::Status => Request::Status,
        Cmd::Ping => Request::Ping,
    };
    let resp = call(&args.socket, &req)?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&resp)?);
        return Ok(match resp {
            Response::Error { .. } => ExitCode::from(1),
            _ => ExitCode::SUCCESS,
        });
    }
    match resp {
        Response::Pong => println!("pong"),
        Response::Status(s) => print_status(&s),
        Response::Error { message } => {
            eprintln!("discoctl: {message}");
            return Ok(ExitCode::from(1));
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn print_status(s: &Status) {
    println!(
        "discowalld {}  up {}s  config {}",
        s.version, s.uptime_s, s.config_path
    );
    println!(
        "{:<12} {:>5} {:<5} {:<6} {:>8} {:>8}  addresses",
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
            "{:<12} {:>5} {:<5} {:<6} {:>8} {:>8}  {}",
            i.name, index, state, joined, i.rx, i.tx, addrs
        );
    }
    let c = &s.counters;
    println!(
        "rx {}  tx {}  dropped: own-source {}  duplicate {}  unknown-interface {}  send-errors {}",
        c.rx,
        c.tx,
        c.dropped_own_source,
        c.dropped_duplicate,
        c.dropped_unknown_interface,
        c.send_errors
    );
}
