//! Interface tracking over rtnetlink: which configured interface has which index, whether it is
//! up, and every address of the host (the loop check drops packets from our own addresses).

use anyhow::{Context, Result};
use futures_util::{StreamExt, TryStreamExt};
use netlink_packet_core::{NetlinkMessage, NetlinkPayload};
use netlink_packet_route::address::{AddressAttribute, AddressMessage};
use netlink_packet_route::link::{LinkAttribute, LinkFlags, LinkMessage};
use netlink_packet_route::RouteNetlinkMessage;
use rtnetlink::sys::{AsyncSocket, SocketAddr as NlAddr};
use std::net::IpAddr;
use tokio::sync::mpsc;

/// What the interface watcher tells the core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IfaceEvent {
    /// A link exists (new or changed): its index, name and whether it is usable.
    Link {
        index: u32,
        name: String,
        up: bool,
    },
    LinkGone {
        index: u32,
    },
    AddrAdded {
        index: u32,
        addr: IpAddr,
    },
    AddrRemoved {
        index: u32,
        addr: IpAddr,
    },
    /// The initial dump is complete; everything before was the current state.
    Synced,
}

fn link_event(msg: &LinkMessage) -> Option<IfaceEvent> {
    let index = msg.header.index;
    let name = msg.attributes.iter().find_map(|a| match a {
        LinkAttribute::IfName(n) => Some(n.clone()),
        _ => None,
    })?;
    let flags = msg.header.flags;
    // Up and with a carrier: a veth without its peer up, or an unplugged port, has Up but not LowerUp.
    let up = flags.contains(LinkFlags::Up) && flags.contains(LinkFlags::LowerUp);
    Some(IfaceEvent::Link { index, name, up })
}

fn addr_of(msg: &AddressMessage) -> Option<IpAddr> {
    // IFA_LOCAL is the host's address on point-to-point links; IFA_ADDRESS otherwise. Prefer local.
    let mut address = None;
    for a in &msg.attributes {
        match a {
            AddressAttribute::Local(ip) => return Some(*ip),
            AddressAttribute::Address(ip) => address = Some(*ip),
            _ => {}
        }
    }
    address
}

/// Runs forever: dumps links and addresses, then follows the kernel's notifications.
pub async fn watch(tx: mpsc::Sender<IfaceEvent>) -> Result<()> {
    let (mut conn, handle, mut messages) =
        rtnetlink::new_connection().context("rtnetlink socket")?;
    let groups = rtnetlink::constants::RTMGRP_LINK
        | rtnetlink::constants::RTMGRP_IPV4_IFADDR
        | rtnetlink::constants::RTMGRP_IPV6_IFADDR;
    conn.socket_mut()
        .socket_mut()
        .bind(&NlAddr::new(0, groups))
        .context("binding the netlink socket")?;
    tokio::spawn(conn);

    let mut links = handle.link().get().execute();
    while let Some(msg) = links.try_next().await.context("link dump")? {
        if let Some(ev) = link_event(&msg) {
            tx.send(ev).await.ok();
        }
    }
    let mut addrs = handle.address().get().execute();
    while let Some(msg) = addrs.try_next().await.context("address dump")? {
        if let Some(addr) = addr_of(&msg) {
            tx.send(IfaceEvent::AddrAdded {
                index: msg.header.index,
                addr,
            })
            .await
            .ok();
        }
    }
    tx.send(IfaceEvent::Synced).await.ok();

    while let Some((msg, _)) = messages.next().await {
        let NetlinkMessage { payload, .. } = msg;
        let ev =
            match payload {
                NetlinkPayload::InnerMessage(RouteNetlinkMessage::NewLink(m)) => link_event(&m),
                NetlinkPayload::InnerMessage(RouteNetlinkMessage::DelLink(m)) => {
                    Some(IfaceEvent::LinkGone {
                        index: m.header.index,
                    })
                }
                NetlinkPayload::InnerMessage(RouteNetlinkMessage::NewAddress(m)) => addr_of(&m)
                    .map(|addr| IfaceEvent::AddrAdded {
                        index: m.header.index,
                        addr,
                    }),
                NetlinkPayload::InnerMessage(RouteNetlinkMessage::DelAddress(m)) => addr_of(&m)
                    .map(|addr| IfaceEvent::AddrRemoved {
                        index: m.header.index,
                        addr,
                    }),
                _ => None,
            };
        if let Some(ev) = ev {
            if tx.send(ev).await.is_err() {
                break;
            }
        }
    }
    Ok(())
}
