//! The legacy-query proxy (RFC 6762 §6.7, research 3.7): a query from an ephemeral port expects a
//! unicast answer, which a reflected packet can never bring back. So the daemon asks on the target
//! interfaces itself, from a short-lived socket, collects what answers within a window and hands
//! the answers to the core, which relays them to the querier by unicast after the rules had their
//! say.

use crate::socket::{Family, MdnsSocket, Proto, Received, ReplyTo, SocketFactory};
use anyhow::Result;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tracing::{debug, warn};

/// Sends `queries` (bytes per target interface index) from an ephemeral socket and collects answers
/// for `window`; every answer goes to `tx` tagged with `reply_to`. Returns at once; the collecting
/// runs on its own thread.
pub fn ask(
    factory: &SocketFactory,
    proto: Proto,
    family: Family,
    queries: Vec<(Vec<u8>, u32)>,
    reply_to: ReplyTo,
    window: Duration,
    tx: mpsc::Sender<Received>,
) -> Result<()> {
    let sock: Arc<dyn MdnsSocket> = factory(proto, family, true)?;
    for (bytes, ifindex) in &queries {
        if let Err(e) = sock.send_multicast(bytes, *ifindex) {
            warn!(ifindex, "proxy query not sent: {e:#}");
        }
    }
    std::thread::Builder::new()
        .name("legacy-proxy".into())
        .spawn(move || {
            let deadline = Instant::now() + window;
            loop {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    break;
                }
                if sock.set_read_timeout(Some(left)).is_err() {
                    break;
                }
                match sock.recv() {
                    Ok(mut pkt) => {
                        pkt.reply_to = Some(reply_to);
                        if tx.blocking_send(pkt).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        debug!("proxy window closed: {e:#}");
                        break;
                    }
                }
            }
        })?;
    Ok(())
}
