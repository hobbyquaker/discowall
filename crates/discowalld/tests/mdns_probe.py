#!/usr/bin/env python3
"""A tiny mDNS sender/receiver for the network-namespace tests. Standard library only.

  mdns_probe.py send <iface-addr> query <name>              multicast a PTR question from port 5353
  mdns_probe.py send <iface-addr> announce <inst> <svc> <host> <ip>
                                                            multicast PTR+SRV+TXT+A with cache-flush
  mdns_probe.py recv <iface-addr> <timeout-s> [count]       print "src:port kind names" per packet
  mdns_probe.py legacy <iface-addr> <name> <timeout-s>      ask from an ephemeral port, print the unicast answer
  mdns_probe.py respond <iface-addr> <timeout-s> <inst> <svc> <host> <ip>
                                                            answer the first PTR query by unicast (legacy style)
  An <iface-addr> of the form v6:<ifname> uses IPv6 (ff02::fb) on that interface.
  mdns_probe.py ssdp-notify <iface-addr> <uuid> <nt> <location>  multicast an ssdp:alive on 1900
  mdns_probe.py ssdp-recv <iface-addr> <timeout-s>                print the first SSDP packet on 1900
  mdns_probe.py ssdp-search <iface-addr> <st> <timeout-s>         M-SEARCH from an ephemeral port, print the answer
  mdns_probe.py ssdp-respond <iface-addr> <timeout-s> <uuid> <st> <location>
                                                                  answer the first M-SEARCH by unicast
"""
SSDP_GROUP = "239.255.255.250"
SSDP_PORT = 1900
import socket
import struct
import sys
import time

GROUP = "224.0.0.251"
PORT = 5353


GROUP6 = "ff02::fb"


def sock6(ifname, port=PORT):
    idx = socket.if_nametoindex(ifname)
    s = socket.socket(socket.AF_INET6, socket.SOCK_DGRAM)
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEPORT, 1)
    s.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_MULTICAST_HOPS, 255)
    s.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_MULTICAST_LOOP, 0)
    s.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_MULTICAST_IF, idx)
    s.bind(("::", port))
    s.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_JOIN_GROUP,
                 socket.inet_pton(socket.AF_INET6, GROUP6) + struct.pack("@I", idx))
    return s, (GROUP6, PORT, 0, idx)


def open_sock(addr, port=PORT):
    if addr.startswith("v6:"):
        return sock6(addr[3:], port)
    return sock(addr, port), (GROUP, PORT)


def ssdp_sock(addr, port=SSDP_PORT):
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEPORT, 1)
    s.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_TTL, 4)
    s.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_LOOP, 0)
    s.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_IF, socket.inet_aton(addr))
    s.bind(("0.0.0.0", port))
    s.setsockopt(socket.IPPROTO_IP, socket.IP_ADD_MEMBERSHIP,
                 struct.pack("4s4s", socket.inet_aton(SSDP_GROUP), socket.inet_aton(addr)))
    return s


def ssdp_alive(uuid, nt, location):
    return (f"NOTIFY * HTTP/1.1\r\nHOST: {SSDP_GROUP}:{SSDP_PORT}\r\nCACHE-CONTROL: max-age=1800\r\n"
            f"LOCATION: {location}\r\nNT: {nt}\r\nNTS: ssdp:alive\r\nSERVER: probe/1\r\nUSN: {uuid}::{nt}\r\n\r\n").encode()


def ssdp_search(st):
    return (f"M-SEARCH * HTTP/1.1\r\nHOST: {SSDP_GROUP}:{SSDP_PORT}\r\nMAN: \"ssdp:discover\"\r\nMX: 1\r\nST: {st}\r\n\r\n").encode()


def ssdp_describe(data):
    lines = data.decode(errors="replace").split("\r\n")
    headers = {}
    for line in lines[1:]:
        if ":" in line:
            k, v = line.split(":", 1)
            headers[k.strip().upper()] = v.strip()
    keys = ["NTS", "NT", "ST", "USN", "LOCATION"]
    return lines[0] + " " + " ".join(f"{k}={headers[k]}" for k in keys if k in headers)


def sock(addr, port=PORT):
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEPORT, 1)
    s.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_TTL, 255)
    s.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_LOOP, 0)
    s.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_IF, socket.inet_aton(addr))
    s.bind(("0.0.0.0", port))
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
    if mode == "ssdp-notify":
        ssdp_sock(addr).sendto(ssdp_alive(*sys.argv[3:6]), (SSDP_GROUP, SSDP_PORT))
        return
    if mode == "ssdp-recv":
        s = ssdp_sock(addr)
        s.settimeout(float(sys.argv[3]))
        try:
            data, src = s.recvfrom(9000)
            print(f"{src[0]}:{src[1]} {ssdp_describe(data)}", flush=True)
        except socket.timeout:
            print("timeout", flush=True)
        return
    if mode == "ssdp-search":
        s = ssdp_sock(addr, 0)
        s.sendto(ssdp_search(sys.argv[3]), (SSDP_GROUP, SSDP_PORT))
        s.settimeout(float(sys.argv[4]))
        try:
            data, src = s.recvfrom(9000)
            print(f"{src[0]}:{src[1]} {ssdp_describe(data)}", flush=True)
        except socket.timeout:
            print("timeout", flush=True)
        return
    if mode == "ssdp-respond":
        s = ssdp_sock(addr)
        s.settimeout(float(sys.argv[3]))
        uuid, st, location = sys.argv[4:7]
        try:
            while True:
                data, src = s.recvfrom(9000)
                if not data.startswith(b"M-SEARCH"):
                    continue
                reply = (f"HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=1800\r\nEXT:\r\nLOCATION: {location}\r\n"
                         f"SERVER: probe/1\r\nST: {st}\r\nUSN: {uuid}::{st}\r\n\r\n").encode()
                s.sendto(reply, src)
                print(f"answered {src[0]}:{src[1]}", flush=True)
                return
        except socket.timeout:
            print("timeout", flush=True)
        return
    if mode == "legacy":
        # An ephemeral port: the responder (or the proxy) answers by unicast.
        s, group = open_sock(addr, 0)
        s.sendto(query(sys.argv[3]), group)
        s.settimeout(float(sys.argv[4]))
        try:
            data, src = s.recvfrom(9000)
            print(f"{src[0]}:{src[1]} {describe(data)}", flush=True)
        except socket.timeout:
            print("timeout", flush=True)
        return
    s, group = open_sock(addr)
    if mode == "send":
        what = sys.argv[3]
        pkt = query(sys.argv[4]) if what == "query" else announce(*sys.argv[4:8])
        s.sendto(pkt, group)
        return
    if mode == "respond":
        s.settimeout(float(sys.argv[3]))
        try:
            while True:
                data, src = s.recvfrom(9000)
                _, flags, qd, *_ = struct.unpack("!HHHHHH", data[:12])
                if flags & 0x8000 or qd == 0:
                    continue
                # A legacy response: the query's id, the question repeated, answers appended.
                body = announce(*sys.argv[4:8])
                reply = data[:2] + struct.pack("!HHHHH", 0x8400, 1, 3, 0, 1) + data[12:] + body[12:]
                s.sendto(reply, src)
                print(f"answered {src[0]}:{src[1]}", flush=True)
                return
        except socket.timeout:
            print("timeout", flush=True)
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
