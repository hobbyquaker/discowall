#!/usr/bin/env python3
"""A minimal IGMPv2 querier for the lab: sends a General Membership Query to 224.0.0.1 on each given
interface every INTERVAL seconds (default 60). Standard library only, needs CAP_NET_RAW (root).

Why: the Proxmox host's Linux bridge has multicast snooping on and no querier (measured 2026-10-13:
multicast_snooping=1, multicast_querier=0, multicast_membership_interval=26000 cs). Without queries no
host refreshes its membership, and the bridge stops forwarding a group to a port 260 s after the join.
224.0.0.0/24 (mDNS) is exempt — Linux bridges flood the local network control block — so mDNS kept
working while SSDP (239.255.255.250) died a few minutes after every daemon start. A real network has a
querier (the router or a switch); this script is that querier for the lab, run on the router container.

  igmp_querier.py [-i INTERVAL] IFACE [IFACE ...]
"""
import argparse
import socket
import struct
import sys
import time


def checksum(data):
    if len(data) % 2:
        data += b"\0"
    s = sum(struct.unpack("!%dH" % (len(data) // 2), data))
    while s >> 16:
        s = (s & 0xFFFF) + (s >> 16)
    return (~s) & 0xFFFF


def query_packet(max_resp_ds=100):
    # IGMPv2 Membership Query: type 0x11, max response time in 1/10 s, checksum, group 0.0.0.0.
    head = struct.pack("!BBH4s", 0x11, max_resp_ds, 0, b"\0\0\0\0")
    return struct.pack("!BBH4s", 0x11, max_resp_ds, checksum(head), b"\0\0\0\0")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("-i", "--interval", type=float, default=60.0)
    ap.add_argument("ifaces", nargs="+")
    args = ap.parse_args()
    pkt = query_packet()
    print(f"querying every {args.interval:g} s on {', '.join(args.ifaces)}", flush=True)
    while True:
        # A fresh socket per round and interface: the interface may have been re-created since (the
        # lab's dynamics case hot-unplugs the router's iot interface; a socket bound to the old index
        # failed with ENODEV from then on and the iot memberships expired, measured 2026-10-13).
        for name in args.ifaces:
            try:
                with socket.socket(socket.AF_INET, socket.SOCK_RAW, socket.IPPROTO_IGMP) as s:
                    idx = socket.if_nametoindex(name)
                    s.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_TTL, 1)
                    # ip_mreqn: group (unused), local address (any), interface index.
                    s.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_IF,
                                 struct.pack("4s4si", b"\0\0\0\0", b"\0\0\0\0", idx))
                    # Router Alert option, as IGMP queries carry it (RFC 2236 §2; the kernel builds the
                    # IP header).
                    s.setsockopt(socket.IPPROTO_IP, socket.IP_OPTIONS, b"\x94\x04\x00\x00")
                    s.sendto(pkt, ("224.0.0.1", 0))
            except OSError as e:
                print(f"{name}: {e}", file=sys.stderr, flush=True)
        time.sleep(args.interval)

if __name__ == "__main__":
    main()
