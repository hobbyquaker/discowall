//! The control socket: JSON lines over a unix socket (see `discowall-proto`).

use anyhow::{Context, Result};
use discowall_proto::{Request, Response, WatchEvent};
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::broadcast;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, warn};

/// A request handed to the core, with the channel for its answer.
pub struct CoreQuery {
    pub request: Request,
    pub reply: oneshot::Sender<Response>,
}

pub fn bind(path: &Path) -> Result<UnixListener> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    if path.exists() {
        std::fs::remove_file(path).with_context(|| format!("removing stale {}", path.display()))?;
    }
    let listener =
        UnixListener::bind(path).with_context(|| format!("binding {}", path.display()))?;
    Ok(listener)
}

pub async fn serve(
    listener: UnixListener,
    core: mpsc::Sender<CoreQuery>,
    events: broadcast::Sender<WatchEvent>,
) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let core = core.clone();
                let events = events.clone();
                tokio::spawn(async move {
                    if let Err(e) = connection(stream, core, events).await {
                        debug!("control connection: {e:#}");
                    }
                });
            }
            Err(e) => warn!("control accept: {e}"),
        }
    }
}

async fn connection(
    stream: UnixStream,
    core: mpsc::Sender<CoreQuery>,
    events: broadcast::Sender<WatchEvent>,
) -> Result<()> {
    let (rd, mut wr) = stream.into_split();
    let mut lines = BufReader::new(rd).lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Request>(&line) {
            Ok(Request::Ping) => Response::Pong,
            Ok(Request::Watch { iface }) => {
                // Stream until the client goes away; a slow client that lags behind misses events
                // (broadcast semantics) rather than stalling the daemon.
                let mut rx = events.subscribe();
                loop {
                    let ev = match rx.recv().await {
                        Ok(ev) => ev,
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            let mut out = serde_json::to_string(&Response::Error {
                                message: format!("{n} events missed"),
                            })?;
                            out.push('\n');
                            wr.write_all(out.as_bytes()).await?;
                            continue;
                        }
                        Err(broadcast::error::RecvError::Closed) => return Ok(()),
                    };
                    if iface.as_deref().is_some_and(|i| i != ev.iface) {
                        continue;
                    }
                    let mut out = serde_json::to_string(&Response::Event(ev))?;
                    out.push('\n');
                    if wr.write_all(out.as_bytes()).await.is_err() {
                        return Ok(());
                    }
                }
            }
            Ok(request) => {
                let (reply, rx) = oneshot::channel();
                core.send(CoreQuery { request, reply }).await.ok();
                rx.await.unwrap_or_else(|_| Response::Error {
                    message: "core gone".into(),
                })
            }
            Err(e) => Response::Error {
                message: format!("bad request: {e}"),
            },
        };
        let mut out = serde_json::to_string(&response)?;
        out.push('\n');
        wr.write_all(out.as_bytes()).await?;
    }
    Ok(())
}
