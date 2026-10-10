//! discowalld: forwards mDNS between a router's interfaces under rules (P0: without rules yet).

mod config;
mod control;
#[allow(dead_code)]
mod hosts;
mod ifaces;
#[allow(dead_code)]
mod packet;
mod reflector;
#[allow(dead_code)]
mod rules;
mod socket;

use anyhow::{Context, Result};
use clap::Parser;
use config::Config;
use control::CoreQuery;
use socket::{Family, MdnsSocket};
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
    /// Check the configuration and exit.
    #[arg(long)]
    check: bool,
    /// Log level or filter (RUST_LOG syntax).
    #[arg(long, default_value = "info", env = "DISCOWALL_LOG")]
    log: String,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let under_journal = std::env::var_os("JOURNAL_STREAM").is_some();
    let filter = tracing_subscriber::EnvFilter::try_new(&args.log).context("--log")?;
    let fmt = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false);
    if under_journal {
        fmt.without_time().init();
    } else {
        fmt.init();
    }

    let config = Config::load(&args.config)?;
    if args.check {
        println!(
            "{}: ok ({} interfaces)",
            args.config.display(),
            config.interfaces.len()
        );
        return Ok(());
    }
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(run(config, args.config.display().to_string()))
}

async fn run(config: Config, config_path: String) -> Result<()> {
    let mut sockets: Vec<Arc<dyn MdnsSocket>> = Vec::new();
    if config.ipv4 {
        sockets.push(Arc::new(socket::linux::LinuxSocket::open(Family::V4)?));
    }
    if config.ipv6 {
        sockets.push(Arc::new(socket::linux::LinuxSocket::open(Family::V6)?));
    }
    info!(interfaces = ?config.interfaces, ipv4 = config.ipv4, ipv6 = config.ipv6, "discowalld {} starting", env!("CARGO_PKG_VERSION"));

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
    let mut core = reflector::Reflector::new(&config, config_path, sockets)?;
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
