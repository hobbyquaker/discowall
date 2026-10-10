#!/usr/bin/env python3
"""A tiny mDNS sender/receiver for the network-namespace tests. Standard library only.

  mdns_probe.py send <iface-addr> query <name>              multicast a PTR question from port 5353
  mdns_probe.py send <iface-addr> announce <inst> <svc> <host> <ip>
                                                            multicast PTR+SRV+TXT+A with cache-flush
  mdns_probe.py recv <iface-addr> <timeout-s> [count]       print "src:port kind names" per packet
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


def name(n):
    out = b""
    for label in n.strip(".").split("."):
        b = label.encode()
        out += bytes([len(b)]) + b
    return out + b"\x00"


def rr(owner, rtype, rdata, flush, ttl=120):
    cls = 1 | (0x8000 if flush else 0)
    return name(owner) + struct.pack("!HHIH", rtype, cls, ttl, len(rdata)) + rdata


def query(n):
    return struct.pack("!HHHHHH", 0, 0, 1, 0, 0, 0) + name(n) + struct.pack("!HH", 12, 1)


def announce(inst, svc, host, ip):
    full = f"{inst}.{svc}.local"
    answers = rr(f"{svc}.local", 12, name(full), False)
    answers += rr(full, 33, struct.pack("!HHH", 0, 0, 1234) + name(f"{host}.local"), True)
    answers += rr(full, 16, b"\x03a=b", True)
    additional = rr(f"{host}.local", 1, socket.inet_aton(ip), True)
    return struct.pack("!HHHHHH", 0, 0x8400, 0, 3, 0, 1) + answers + additional


def read_name(data, off):
    labels = []
    while True:
        n = data[off]
        if n == 0:
            return ".".join(labels), off + 1
        if n & 0xC0 == 0xC0:
            ptr = struct.unpack("!H", data[off:off + 2])[0] & 0x3FFF
            tail, _ = read_name(data, ptr)
            return ".".join(labels + [tail]), off + 2
        labels.append(data[off + 1:off + 1 + n].decode(errors="replace"))
        off += 1 + n


def describe(data):
    _, flags, qd, an, ns, ar = struct.unpack("!HHHHHH", data[:12])
    kind = "response" if flags & 0x8000 else "query"
    off = 12
    names = []
    for _ in range(qd):
        n, off = read_name(data, off)
        off += 4
        names.append(n)
    for _ in range(an + ns + ar):
        n, off = read_name(data, off)
        rtype, _, _, rdlen = struct.unpack("!HHIH", data[off:off + 10])
        off += 10 + rdlen
        names.append(f"{n}/{rtype}")
    return f"{kind} " + " ".join(names)


def main():
    mode, addr = sys.argv[1], sys.argv[2]
    s = sock(addr)
    if mode == "send":
        what = sys.argv[3]
        pkt = query(sys.argv[4]) if what == "query" else announce(*sys.argv[4:8])
        s.sendto(pkt, (GROUP, PORT))
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
        print(f"{src[0]}:{src[1]} {describe(data)}", flush=True)
        got += 1


main()
