# discowall

[:de: Deutsches README](README.de.md)

[![CI](https://github.com/hobbyquaker/discowall/actions/workflows/ci.yml/badge.svg)](https://github.com/hobbyquaker/discowall/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

> [!CAUTION]
> discowall is under development. The reflector with its rule chain and CLI exists (P0, P1) but has not run on a real router yet.

discowall is a **discovery firewall** for a Linux router: it forwards mDNS packets (Bonjour, Zeroconf, RFC 6762)
from one network interface to others, as avahi-daemon's reflector does, but under **rules**: which hosts and
which services from which network become visible in which other network, as a whitelist or a blacklist, by
source, target and direction. A simple form of iptables for `.local`. Plus a CLI that shows which announcements
arrive on which interface and which rule lets them pass, and that is usable from scripts. SSDP (UPnP) is to
follow later.

## Why

mDNS is confined to the local network segment. Whoever separates devices into VLANs (IoT, guests, media) loses
AirPlay, HomeKit, Chromecast, printers and everything else found through Bonjour. The usual way out is avahi's
reflector: it mirrors **everything** into **every** network, and its only filter is a flat list of service types
that applies to all interfaces alike. No direction ("the HomeKit bridge in the IoT network may be seen from the
LAN, but nothing from the LAN in the IoT network"), no rule by source address, no view of what is being forwarded
right now and why. The smaller reflector tools do not filter at all.

discowall turns the reflector into a firewall: an ordered rule chain, first match wins, a default policy, and a
rule for a host or a service applies to the whole bundle of PTR, SRV, TXT and A/AAAA records that a service is.

## What it is meant to do

- **Forward** between configured interfaces, IPv4 and IPv6, without loops (never back onto the arriving
  interface, own packets dropped, deduplication against a second reflector).
- **Rules** by arriving and target interface, original source address or prefix, kind of packet (query, response,
  announcement, probe), service type, instance name, host name and record type; action `accept` or `drop`,
  optionally with `log`.
- **A CLI** `discoctl`: `rule list|add|insert|del|flush`, `save`, `apply`, `test`; `watch` for the live stream
  of packets with decision and rule, `hosts` and `services` for the learned table, `stats`, `status`. Every
  command with `--json`, fixed exit codes, nothing interactive.
- **Beside avahi:** discowall is neither an mDNS responder nor a resolver. avahi may keep running on the same host
  and announce the router as `router.local`; discowall only does the forwarding.
- **One-shot queries** (for example `dig @224.0.0.251 -p 5353`) are asked on the querier's behalf and the
  answers returned by unicast, which avahi's reflector cannot do.
- **One daemon** `discowalld`, a static binary in Rust, without root after start, without raw sockets, with a
  systemd unit; packages for Debian/Ubuntu/Armbian, Arch, Fedora and OpenWrt, later FreeBSD/pfSense.

Example: make HomeKit from the IoT network visible in the LAN, and nothing else.

```sh
discoctl default drop
discoctl rule add in lan out iot kind query    service _hap._tcp  accept   # the LAN may ask the IoT network for HomeKit
discoctl rule add in iot out lan kind response host bridge         accept   # only the bridge's answers come back
discoctl watch --iface iot
```

Discovery is not the data path: the connection to the device found (TCP 51826 for HomeKit, 631 for IPP, 7000 for
AirPlay) still needs a firewall rule. A later extension is to maintain the addresses of allowed hosts in
nftables sets, so that exactly one such rule per network pair is enough.

## Status

The research of 10 October 2026 (survey of avahi and the existing reflectors, design, assessment of Rust, effort
estimate, record of the decisions) is in [mdns-router-research.md](mdns-router-research.md). The decisions are in
[roadmap-items/discowall/decisions.md](roadmap-items/discowall/decisions.md), the tasks one by one in
[roadmap-readmes/discowall.md](roadmap-readmes/discowall.md), the open questions in
[roadmap-items/discowall/overview.md](roadmap-items/discowall/overview.md). The estimate for the first version is
20 to 30 person-days.

## Roadmap

- 🕵️ **P0:** Cargo project, sockets, interface tracking over netlink, plain reflector with loop prevention,
  systemd unit
- ☑️ **Lab:** measure coexistence with avahi-daemon on port 5353
- 🕵️ **P1:** parser, host table and record bundles, rule chain, control socket, `discoctl rule …`
- 🕵️ **P2:** `discoctl watch`, `hosts`, `services`, `stats`, `status`; per-rule counters, `log`
- 🏗️ **P3:** proxy for one-shot queries and the QU bit, link-local AAAA stripping, IPv6 end to end,
  deduplication, fuzzing, unit hardening
- ☑️ **P4:** packages (tarball, deb, rpm, OpenWrt), cross builds, release workflow, README and man pages
- 💤 nftables sets with the addresses of allowed hosts
- 💤 SSDP (UPnP) as a second protocol
- 💤 FreeBSD backend and pfSense package
- ✅ Concept: rule model, CLI and runtime discussed with the maintainer

🏗️ in progress · 🕵️ built, under test · ☑️ ready for implementation · 🤷 to be clarified · 💤 later · ✅ done

## Credits and licences

- This repository: MIT, Sebastian Raff.
- [avahi](https://avahi.org) (LGPL): the reflector discowall complements; its man page is the source for what it
  cannot do.
- [mdns-repeater](https://github.com/kennylevinsen/mdns-repeater) (GPL) and
  [mdns-reflector](https://github.com/vfreex/mdns-reflector) (GPL-3.0): the small reflectors, read as a model for
  the socket part, no code taken.
- RFC 6762 (Multicast DNS) and RFC 6763 (DNS-Based Service Discovery): the specification.
