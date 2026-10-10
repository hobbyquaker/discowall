#!/usr/bin/env python3
"""A tiny mDNS sender/receiver for the network-namespace tests. Standard library only.

  mdns_probe.py send <iface-addr> <payload>            multicast one packet from port 5353
  mdns_probe.py recv <iface-addr> <timeout-s> [count]  print "src:port payload" per packet received
"""
import socket
import struct
import sys
import time

GROUP = "224.0.0.251"
PORT = 5353


def sock(addr):
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEPORT, 1)
    s.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_TTL, 255)
    s.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_LOOP, 0)
    s.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_IF, socket.inet_aton(addr))
    s.bind(("0.0.0.0", PORT))
    s.setsockopt(socket.IPPROTO_IP, socket.IP_ADD_MEMBERSHIP,
                 struct.pack("4s4s", socket.inet_aton(GROUP), socket.inet_aton(addr)))
    return s


def main():
    mode, addr = sys.argv[1], sys.argv[2]
    s = sock(addr)
    if mode == "send":
        s.sendto(sys.argv[3].encode(), (GROUP, PORT))
        return
    deadline = time.monotonic() + float(sys.argv[3])
    want = int(sys.argv[4]) if len(sys.argv) > 4 else 1
    got = 0
    while got < want:
        left = deadline - time.monotonic()
        if left <= 0:
            break
        s.settimeout(left)
        try:
            data, src = s.recvfrom(9000)
        except socket.timeout:
            break
        print(f"{src[0]}:{src[1]} {data.decode(errors='replace')}", flush=True)
        got += 1


main()
