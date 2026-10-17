//! discowalld: forwards mDNS between a router's interfaces under rules (P0: without rules yet).

mod config;
mod control;
#[allow(dead_code)]
mod hosts;
mod ifaces;
#[allow(dead_code)]
mod packet;
mod proxy;
mod reflector;
#[allow(dead_code)]
mod rules;
mod socket;
mod ssdp;

use anyhow::{Context, Result};
use clap::Parser;
use config::Config;
use control::CoreQuery;
use socket::{Family, MdnsSocket, Proto, SocketFactory};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info};

#[derive(Parser, Debug)]
#[command(
    version,
    about = "discovery firewall: forwards mDNS between interfaces under rules"
)]
struct Args {
    /// The configuration file.
    #[arg(short, long, default_value = "/etc/discowall/discowall.toml")]
    config: PathBuf,
    /// Check the configuration and the rules file it names (as start and `apply` load them), and exit.
    #[arg(long)]
    check: bool,
    /// With --check: check this rules file instead of the one the configuration names (the
    /// configuration is then not read), e.g. a new rules.toml before installing it.
    #[arg(long, value_name = "FILE", requires = "check")]
    rules: Option<PathBuf>,
    /// Log level or filter (RUST_LOG syntax).
    #[arg(long, default_value = "info", env = "DISCOWALL_LOG")]
    log: String,
    /// Write the man page into this directory and exit (packaging).
    #[arg(long, hide = true, value_name = "DIR")]
    generate_man: Option<PathBuf>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if let Some(dir) = &args.generate_man {
        use clap::CommandFactory;
        std::fs::create_dir_all(dir)?;
        clap_mangen::generate_to(Args::command(), dir)?;
        return Ok(());
    }
    let under_journal = std::env::var_os("JOURNAL_STREAM").is_some();
    let filter = tracing_subscriber::EnvFilter::try_new(&args.log).context("--log")?;
    // Colours only on a terminal: systemd's journal, a pipe or a file would keep the escape codes.
    let fmt = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stdout()));
    if under_journal {
        fmt.without_time().init();
    } else {
        fmt.init();
    }

    if let Some(rules) = &args.rules {
        println!("{}", reflector::check_rules(rules)?);
        return Ok(());
    }
    let config = Config::load(&args.config)?;
    if args.check {
        println!(
            "{}: ok ({} interfaces)",
            args.config.display(),
            config.interfaces.len()
        );
        println!("{}", reflector::check_rules(&config.rules_file)?);
        return Ok(());
    }
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(run(config, args.config.display().to_string()))
}

async fn run(config: Config, config_path: String) -> Result<()> {
    let mut sockets: Vec<Arc<dyn MdnsSocket>> = Vec::new();
    let protos: Vec<Proto> = if config.ssdp {
        vec![Proto::Mdns, Proto::Ssdp]
    } else {
        vec![Proto::Mdns]
    };
    for proto in protos {
        if config.ipv4 {
            sockets.push(Arc::new(socket::linux::LinuxSocket::open(
                proto,
                Family::V4,
            )?));
        }
        if config.ipv6 {
            sockets.push(Arc::new(socket::linux::LinuxSocket::open(
                proto,
                Family::V6,
            )?));
        }
    }
    info!(interfaces = ?config.interfaces, ipv4 = config.ipv4, ipv6 = config.ipv6, ssdp = config.ssdp, "discowalld {} starting", env!("CARGO_PKG_VERSION"));

    let listener = control::bind(&config.control_socket)?;
    let (packet_tx, mut packet_rx) = mpsc::channel(1024);
    for s in &sockets {
        let s = s.clone();
        let tx = packet_tx.clone();
        std::thread::Builder::new()
            .name(format!("recv-{:?}", s.family()))
            .spawn(move || loop {
                match s.recv() {
                    Ok(pkt) => {
                        if tx.blocking_send(pkt).is_err() {
                            return;
                        }
                    }
                    Err(e) => error!(family = ?s.family(), "recv: {e:#}"),
                }
            })?;
    }
    let (iface_tx, mut iface_rx) = mpsc::channel(256);
    let (query_tx, mut query_rx) = mpsc::channel(16);
    tokio::spawn(async move {
        if let Err(e) = ifaces::watch(iface_tx).await {
            error!("interface watcher ended: {e:#}");
        }
    });
    let factory: SocketFactory = Arc::new(|proto, family, ephemeral| {
        let s = if ephemeral {
            socket::linux::LinuxSocket::open_ephemeral(proto, family)?
        } else {
            socket::linux::LinuxSocket::open(proto, family)?
        };
        Ok(Arc::new(s) as Arc<dyn MdnsSocket>)
    });
    let mut core =
        reflector::Reflector::new(&config, config_path, sockets, factory, packet_tx.clone())?;
    tokio::spawn(control::serve(listener, query_tx, core.events.clone()));
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut sighup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(5));
    loop {
        tokio::select! {
            Some(pkt) = packet_rx.recv() => { core.on_packet(pkt); }
            Some(ev) = iface_rx.recv() => core.on_iface_event(ev),
            Some(CoreQuery { request, reply }) = query_rx.recv() => { reply.send(core.handle(request)).ok(); }
            _ = tick.tick() => core.tick(),
            _ = sighup.recv() => core.reload_rules(),
            _ = tokio::signal::ctrl_c() => break,
            _ = sigterm.recv() => break,
        }
    }
    info!("stopping");
    let _ = std::fs::remove_file(&config.control_socket);
    Ok(())
}
