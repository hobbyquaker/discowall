//! The mDNS sockets behind a trait (D-4: a FreeBSD backend is an addition, not a rewrite).
//!
//! One socket per address family, bound to the wildcard address on 5353 with `SO_REUSEADDR` and
//! `SO_REUSEPORT` so that avahi may hold the port as well (D-3), `IP_PKTINFO` / `IPV6_RECVPKTINFO`
//! so that every packet says which interface it arrived on, multicast loop off, TTL / hop limit 255
//! (RFC 6762 §11). The egress interface of a send is chosen per packet through the same control
//! message, so one socket serves every interface.

use anyhow::{Context, Result};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

pub use discowall_proto::Proto;

pub const MDNS_PORT: u16 = 5353;
pub const MDNS_V4: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
pub const MDNS_V6: Ipv6Addr = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 0xfb);
pub const SSDP_PORT: u16 = 1900;
pub const SSDP_V4: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);
pub const SSDP_V6: Ipv6Addr = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 0xc);

pub fn port_of(proto: Proto) -> u16 {
    match proto {
        Proto::Mdns => MDNS_PORT,
        Proto::Ssdp => SSDP_PORT,
    }
}

/// mDNS wants TTL 255 (RFC 6762 §11); UPnP recommends a small TTL (2 in the spec, 4 is common).
fn ttl_of(proto: Proto) -> u32 {
    match proto {
        Proto::Mdns => 255,
        Proto::Ssdp => 4,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Family {
    V4,
    V6,
}

/// A packet as it arrived: the payload, the original source, the interface index.
#[derive(Debug, Clone)]
pub struct Received {
    pub proto: Proto,
    pub family: Family,
    pub data: Vec<u8>,
    pub src: SocketAddr,
    pub ifindex: u32,
    /// Set on an answer the legacy-query proxy collected: the one-shot querier it is for and the
    /// interface it sits on.
    pub reply_to: Option<ReplyTo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplyTo {
    pub addr: SocketAddr,
    pub ifindex: u32,
}

pub fn group_addr(proto: Proto, family: Family) -> SocketAddr {
    match (proto, family) {
        (Proto::Mdns, Family::V4) => SocketAddr::new(IpAddr::V4(MDNS_V4), MDNS_PORT),
        (Proto::Mdns, Family::V6) => SocketAddr::new(IpAddr::V6(MDNS_V6), MDNS_PORT),
        (Proto::Ssdp, Family::V4) => SocketAddr::new(IpAddr::V4(SSDP_V4), SSDP_PORT),
        (Proto::Ssdp, Family::V6) => SocketAddr::new(IpAddr::V6(SSDP_V6), SSDP_PORT),
    }
}

/// One mDNS socket of one address family.
pub trait MdnsSocket: Send + Sync {
    fn proto(&self) -> Proto;
    fn family(&self) -> Family;
    /// Blocks until a packet arrives.
    fn recv(&self) -> Result<Received>;
    /// Sends `data` to the protocol's group on the interface `ifindex`.
    fn send_multicast(&self, data: &[u8], ifindex: u32) -> Result<()>;
    /// Sends `data` to one host on the interface `ifindex` (a legacy response, RFC 6762 §6.7).
    fn send_unicast(&self, data: &[u8], dst: SocketAddr, ifindex: u32) -> Result<()>;
    fn join(&self, ifindex: u32) -> Result<()>;
    fn leave(&self, ifindex: u32) -> Result<()>;
    /// `recv` returns an error after this long without a packet (`None`: blocks forever).
    fn set_read_timeout(&self, timeout: Option<std::time::Duration>) -> Result<()>;
}

/// Opens sockets: the two long-lived ones at start, an ephemeral one per proxied legacy query.
pub type SocketFactory = std::sync::Arc<
    dyn Fn(Proto, Family, bool) -> Result<std::sync::Arc<dyn MdnsSocket>> + Send + Sync,
>;

#[cfg(target_os = "linux")]
pub mod linux {
    use super::*;
    use nix::sys::socket::{
        recvmsg, sendmsg, ControlMessage, ControlMessageOwned, MsgFlags, SockaddrIn, SockaddrIn6,
        SockaddrStorage,
    };
    use socket2::{Domain, InterfaceIndexOrAddress, Protocol, Socket, Type};
    use std::io::{IoSlice, IoSliceMut};
    use std::os::fd::AsRawFd;

    pub struct LinuxSocket {
        proto: Proto,
        family: Family,
        sock: Socket,
    }

    fn setsockopt_int(
        sock: &Socket,
        level: libc::c_int,
        name: libc::c_int,
        value: libc::c_int,
    ) -> Result<()> {
        // SAFETY: a plain integer option on a socket we own; the kernel copies `value`.
        let rc = unsafe {
            libc::setsockopt(
                sock.as_raw_fd(),
                level,
                name,
                &value as *const libc::c_int as *const libc::c_void,
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            )
        };
        if rc != 0 {
            return Err(std::io::Error::last_os_error())
                .with_context(|| format!("setsockopt({level}, {name})"));
        }
        Ok(())
    }

    impl LinuxSocket {
        /// The long-lived socket on the protocol's port.
        pub fn open(proto: Proto, family: Family) -> Result<Self> {
            Self::open_on(proto, family, port_of(proto))
        }

        /// A short-lived socket on an ephemeral port: the proxy asks from it on a one-shot querier's
        /// behalf, and responders answer it by unicast (they treat it as a legacy querier).
        pub fn open_ephemeral(proto: Proto, family: Family) -> Result<Self> {
            Self::open_on(proto, family, 0)
        }

        fn open_on(proto: Proto, family: Family, port: u16) -> Result<Self> {
            let domain = match family {
                Family::V4 => Domain::IPV4,
                Family::V6 => Domain::IPV6,
            };
            let sock = Socket::new(domain, Type::DGRAM, Some(Protocol::UDP)).context("socket()")?;
            sock.set_reuse_address(true).context("SO_REUSEADDR")?;
            sock.set_reuse_port(true).context("SO_REUSEPORT")?;
            match family {
                Family::V4 => {
                    setsockopt_int(&sock, libc::IPPROTO_IP, libc::IP_PKTINFO, 1)?;
                    // Without this, a wildcard-bound socket also gets every group any other socket
                    // on the host joined, on every interface. We want only our own joins.
                    setsockopt_int(&sock, libc::IPPROTO_IP, libc::IP_MULTICAST_ALL, 0)?;
                    sock.set_multicast_loop_v4(false)
                        .context("IP_MULTICAST_LOOP")?;
                    sock.set_multicast_ttl_v4(ttl_of(proto))
                        .context("IP_MULTICAST_TTL")?;
                    sock.bind(&SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port).into())
                        .with_context(|| format!("bind 0.0.0.0:{port}"))?;
                }
                Family::V6 => {
                    sock.set_only_v6(true).context("IPV6_V6ONLY")?;
                    setsockopt_int(&sock, libc::IPPROTO_IPV6, libc::IPV6_RECVPKTINFO, 1)?;
                    // IPV6_MULTICAST_ALL exists since Linux 4.20; older kernels refuse it.
                    let _ = setsockopt_int(&sock, libc::IPPROTO_IPV6, libc::IPV6_MULTICAST_ALL, 0);
                    sock.set_multicast_loop_v6(false)
                        .context("IPV6_MULTICAST_LOOP")?;
                    sock.set_multicast_hops_v6(ttl_of(proto))
                        .context("IPV6_MULTICAST_HOPS")?;
                    sock.bind(&SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), port).into())
                        .with_context(|| format!("bind [::]:{port}"))?;
                }
            }
            Ok(Self {
                proto,
                family,
                sock,
            })
        }
    }

    impl LinuxSocket {
        fn group_v4(&self) -> Ipv4Addr {
            match self.proto {
                Proto::Mdns => MDNS_V4,
                Proto::Ssdp => SSDP_V4,
            }
        }

        fn group_v6(&self) -> Ipv6Addr {
            match self.proto {
                Proto::Mdns => MDNS_V6,
                Proto::Ssdp => SSDP_V6,
            }
        }

        /// One send with the egress interface chosen through the pktinfo control message.
        fn send_to(&self, data: &[u8], dst: SocketAddr, ifindex: u32) -> Result<()> {
            let iov = [IoSlice::new(data)];
            let fd = self.sock.as_raw_fd();
            match (self.family, dst) {
                (Family::V4, SocketAddr::V4(dst)) => {
                    let pi = libc::in_pktinfo {
                        ipi_ifindex: ifindex as i32,
                        ipi_spec_dst: libc::in_addr { s_addr: 0 },
                        ipi_addr: libc::in_addr { s_addr: 0 },
                    };
                    let addr = SockaddrIn::from(dst);
                    sendmsg(
                        fd,
                        &iov,
                        &[ControlMessage::Ipv4PacketInfo(&pi)],
                        MsgFlags::empty(),
                        Some(&addr),
                    )
                    .context("sendmsg v4")?;
                }
                (Family::V6, SocketAddr::V6(dst)) => {
                    let pi = libc::in6_pktinfo {
                        ipi6_addr: libc::in6_addr { s6_addr: [0; 16] },
                        ipi6_ifindex: ifindex,
                    };
                    let mut dst = dst;
                    if dst.ip().is_multicast() || dst.ip().segments()[0] & 0xffc0 == 0xfe80 {
                        dst.set_scope_id(ifindex);
                    }
                    let addr = SockaddrIn6::from(dst);
                    sendmsg(
                        fd,
                        &iov,
                        &[ControlMessage::Ipv6PacketInfo(&pi)],
                        MsgFlags::empty(),
                        Some(&addr),
                    )
                    .context("sendmsg v6")?;
                }
                _ => anyhow::bail!("address family mismatch"),
            }
            Ok(())
        }
    }

    impl MdnsSocket for LinuxSocket {
        fn proto(&self) -> Proto {
            self.proto
        }

        fn family(&self) -> Family {
            self.family
        }

        fn recv(&self) -> Result<Received> {
            // 9000 covers any jumbo frame; mDNS packets must fit one IP packet anyway.
            let mut buf = vec![0u8; 9000];
            let mut cmsg = nix::cmsg_space!(libc::in6_pktinfo, libc::in_pktinfo);
            let mut iov = [IoSliceMut::new(&mut buf)];
            let r = recvmsg::<SockaddrStorage>(
                self.sock.as_raw_fd(),
                &mut iov,
                Some(&mut cmsg),
                MsgFlags::empty(),
            )
            .context("recvmsg")?;
            let n = r.bytes;
            let mut ifindex = 0u32;
            for c in r.cmsgs().context("cmsgs")? {
                match c {
                    ControlMessageOwned::Ipv4PacketInfo(pi) => ifindex = pi.ipi_ifindex as u32,
                    ControlMessageOwned::Ipv6PacketInfo(pi) => ifindex = pi.ipi6_ifindex,
                    _ => {}
                }
            }
            let src = match r.address {
                Some(a) => {
                    if let Some(v4) = a.as_sockaddr_in() {
                        SocketAddr::new(IpAddr::V4(v4.ip()), v4.port())
                    } else if let Some(v6) = a.as_sockaddr_in6() {
                        SocketAddr::new(IpAddr::V6(v6.ip()), v6.port())
                    } else {
                        anyhow::bail!("recvmsg: source address of an unknown family")
                    }
                }
                None => anyhow::bail!("recvmsg: no source address"),
            };
            buf.truncate(n);
            Ok(Received {
                proto: self.proto,
                family: self.family,
                data: buf,
                src,
                ifindex,
                reply_to: None,
            })
        }

        fn send_multicast(&self, data: &[u8], ifindex: u32) -> Result<()> {
            self.send_to(data, group_addr(self.proto, self.family), ifindex)
        }

        fn send_unicast(&self, data: &[u8], dst: SocketAddr, ifindex: u32) -> Result<()> {
            self.send_to(data, dst, ifindex)
        }

        fn set_read_timeout(&self, timeout: Option<std::time::Duration>) -> Result<()> {
            self.sock.set_read_timeout(timeout).context("SO_RCVTIMEO")
        }

        fn join(&self, ifindex: u32) -> Result<()> {
            match self.family {
                Family::V4 => self
                    .sock
                    .join_multicast_v4_n(&self.group_v4(), &InterfaceIndexOrAddress::Index(ifindex))
                    .with_context(|| format!("IP_ADD_MEMBERSHIP on ifindex {ifindex}")),
                Family::V6 => self
                    .sock
                    .join_multicast_v6(&self.group_v6(), ifindex)
                    .with_context(|| format!("IPV6_JOIN_GROUP on ifindex {ifindex}")),
            }
        }

        fn leave(&self, ifindex: u32) -> Result<()> {
            match self.family {
                Family::V4 => self
                    .sock
                    .leave_multicast_v4_n(
                        &self.group_v4(),
                        &InterfaceIndexOrAddress::Index(ifindex),
                    )
                    .with_context(|| format!("IP_DROP_MEMBERSHIP on ifindex {ifindex}")),
                Family::V6 => self
                    .sock
                    .leave_multicast_v6(&self.group_v6(), ifindex)
                    .with_context(|| format!("IPV6_LEAVE_GROUP on ifindex {ifindex}")),
            }
        }
    }
}
