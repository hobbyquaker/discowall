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
arrive on which interface and which rule lets them pass, and that is usable from scripts. SSDP (UPnP) is
forwarded under the same rules.

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

## Install

Releases on GitHub carry static binaries for x86_64, aarch64 and armv7 (musl, no dependencies), as a tarball with
the man pages, the systemd unit and the examples, and as `.deb` and `.rpm` packages. On OpenWrt use the static
tarball or build the two packages with the SDK (see *OpenWrt* below).

```sh
# Debian, Ubuntu, Armbian
sudo dpkg -i discowall_<version>_<arch>.deb
# Fedora
sudo rpm -i discowall-<version>-1.<arch>.rpm
# any Linux: the tarball
tar xzf discowall-<version>-<target>.tar.gz && cd discowall-*
sudo install -m 755 discowalld discoctl /usr/local/bin/
sudo install -d /etc/discowall && sudo install -m 644 discowall.toml rules.toml /etc/discowall/
sudo install -m 644 discowalld.service /etc/systemd/system/ && sudo systemctl daemon-reload
# from source (Rust stable)
cargo install --git https://github.com/hobbyquaker/discowall discowalld discoctl
```

Then name the interfaces in `/etc/discowall/discowall.toml`, put rules into `/etc/discowall/rules.toml` (or add
them with `discoctl` and `discoctl save`) and start the unit:

```sh
sudo systemctl enable --now discowalld
discoctl status
```

The daemon runs as a dynamic user without capabilities: port 5353 and multicast joins need none. avahi-daemon may
keep running on the same host; discowall only forwards and never answers for the router itself.

### OpenWrt

There are no `.ipk`/`.apk` files on the release yet (that is task 16 on the roadmap); today there are two ways in.

**1. Static binaries from the release tarball** (x86_64, aarch64 and armv7 routers — the binaries are static musl
and run on any OpenWrt of that architecture; MIPS routers are not covered yet). Pick the tarball for your CPU
(`uname -m`: `x86_64`, `aarch64`, `armv7l`), then on the router:

```sh
cd /tmp && wget https://github.com/hobbyquaker/discowall/releases/download/v0.0.2/discowall-0.0.2-aarch64-unknown-linux-musl.tar.gz
tar xzf discowall-0.0.2-*.tar.gz && cd discowall-0.0.2-*
cp discowalld discoctl /usr/bin/ && mkdir -p /etc/discowall && cp rules.toml /etc/discowall/
# the init script and the UCI defaults from the repository, same version
wget -O /etc/init.d/discowall https://raw.githubusercontent.com/hobbyquaker/discowall/v0.0.2/contrib/openwrt/discowall/files/discowall.init
wget -O /tmp/90_discowall https://raw.githubusercontent.com/hobbyquaker/discowall/v0.0.2/contrib/openwrt/discowall/files/discowall.uci-defaults
chmod +x /etc/init.d/discowall && sh /tmp/90_discowall
```

**2. Build the packages with the OpenWrt SDK** (gives you `discowall_<version>_<arch>.ipk` and
`luci-app-discowall_<version>_all.ipk` — the LuCI web UI under *Services → discowall*: status, the rule chain
edited live with Save and Apply, hosts and services, settings). `contrib/openwrt/discowall` builds the daemon with
the packages feed's Rust (about an hour the first time), `contrib/openwrt/luci-app-discowall` the UI;
[tests/lab/openwrt.md](tests/lab/openwrt.md) has the exact steps, tested with OpenWrt 24.10. Install with
`opkg install ./discowall_*.ipk ./luci-app-discowall_*.ipk` (OpenWrt 25: `apk add --allow-untrusted …`).

Either way the settings live in UCI; name at least two interfaces and start:

```sh
uci add_list discowall.main.interface=br-lan
uci add_list discowall.main.interface=br-iot      # Linux device names, as the rules name them
uci commit discowall && /etc/init.d/discowall enable && /etc/init.d/discowall start
discoctl status
```

The init script renders `/etc/discowall/discowall.toml` from `/etc/config/discowall` (`interface`, `ipv4`,
`ipv6`, `ssdp`, `rules_file`); the rules stay in `/etc/discowall/rules.toml`, edited with `discoctl` or the LuCI
app. **Firewall:** a zone with input `REJECT` must accept UDP to ports 5353 and 1900, and UDP *from* source ports
5353 and 1900 — the answers to the searches and one-shot queries the daemon asks on a querier's behalf come back
by unicast to an ephemeral port.

## Configuration

`/etc/discowall/discowall.toml`:

```toml
interfaces = ["lan", "iot"]          # by their Linux names; one that does not exist yet is waited for
#ipv4 = true
#ipv6 = true
#ssdp = true                         # also forward SSDP (UPnP, 239.255.255.250:1900) under the same rules
#control_socket = "/run/discowall/control.sock"
#rules_file = "/etc/discowall/rules.toml"
#dedup_window_ms = 500               # an identical packet within this window is dropped (a second reflector)
#strip_link_local = true             # drop fe80::/10 and 169.254/16 address records: dead on another segment
#legacy_window_ms = 500              # one-shot queries (dig) are asked on the querier's behalf; 0 drops them
```

`discowalld --check` validates it. `SIGHUP` reloads the rules file, not the interfaces.

## Rules

An ordered chain, first match wins, a default policy. A rule has matches and an action; every match left out
matches everything, a list is any-of. Names are matched without their `.local` or `._service._proto.local` tail,
case-insensitively, with `*` and `?` globs.

| Match | Meaning | Example |
| --- | --- | --- |
| `proto` | `mdns`, `ssdp`; absent: both | `proto ssdp` |
| `in` | the interface the packet arrived on | `in iot` |
| `out` | the interface it would be sent to | `out lan` |
| `src` | the original source address or prefix | `src 192.168.30.0/24,fd00::/8` |
| `kind` | `query`, `probe`, `response`, `announce` | `kind response,announce` |
| `service` | service types | `service _hap._tcp,_airplay._tcp` |
| `instance` | service instance names | `instance "living room*"` |
| `host` | host names | `host bridge,printer?` |
| `rrtype` | record types, for a query the question's | `rrtype AAAA` |
| action | `accept` or `drop`, `log` writes a journal line per hit | `accept log` |

For SSDP, `service` is the NT/ST (`upnp:rootdevice`, `urn:schemas-upnp-org:device:MediaRenderer:1`, `ssdp:all`),
`instance` the device's `uuid:…` and `host` the host of its LOCATION URL; `kind` is `query` for M-SEARCH,
`announce` for alive/byebye/update and `response` for a search answer.

A rule on a host or a service applies to the whole bundle that a service is: its PTR, SRV, TXT and A/AAAA
records, which arrive in separate packets. The daemon keeps a table of what each interface announced (instance →
service and host, host → addresses) and attributes every record through it. A packet is rebuilt with only the
records that passed; a query with no question left, or a response with no answer left, is not sent.

The file form, `/etc/discowall/rules.toml`:

```toml
default = "drop"

[[rule]]
in = "lan"
out = "iot"
kind = ["query"]
service = ["_hap._tcp"]
action = "accept"

[[rule]]
in = "iot"
out = "lan"
kind = ["response", "announce"]
host = ["bridge"]
action = "accept"
log = true
```

The same through the CLI (the chain in the daemon changes at once; `save` writes the file, `apply` reloads it):

```sh
discoctl rule add in lan out iot kind query service _hap._tcp accept
discoctl rule add in iot out lan kind response,announce host bridge accept log
discoctl rule insert 1 src 192.168.40.0/24 drop comment "guests see nothing"
discoctl rule list
discoctl rule test in iot out lan kind announce host bridge      # → accept (rule 2)
discoctl rule del 1
discoctl save
```

Whitelist: `default = "drop"` plus accept rules. Blacklist: `default = "accept"` plus drop rules. A missing rules
file means `drop` and no rules: nothing crosses until you say so.

### Recipes

```sh
# HomeKit: the LAN may browse the IoT network, only the bridge answers
discoctl rule add in lan out iot kind query service _hap._tcp accept
discoctl rule add in iot out lan kind response,announce host bridge accept

# AirPlay and Spotify Connect speakers in "media" are visible from the LAN, nothing from the LAN in "media"
discoctl rule add in lan out media kind query service _airplay._tcp,_raop._tcp,_spotify-connect._tcp accept
discoctl rule add in media out lan kind response,announce service _airplay._tcp,_raop._tcp,_spotify-connect._tcp accept

# Printers everywhere
discoctl rule add kind query service _ipp._tcp,_ipps._tcp,_printer._tcp,_pdl-datastream._tcp accept
discoctl rule add kind response,announce service _ipp._tcp,_ipps._tcp,_printer._tcp,_pdl-datastream._tcp accept

# Chromecast / Google Cast from the LAN to the IoT network, but not from the guest network
discoctl rule add in guest drop
discoctl rule add in lan out iot kind query service _googlecast._tcp accept
discoctl rule add in iot out lan kind response,announce service _googlecast._tcp accept

# UPnP: the LAN may search the IoT network, the media renderer announces back
discoctl rule add proto ssdp in lan out iot kind query accept
discoctl rule add proto ssdp in iot out lan kind announce,response service urn:schemas-upnp-org:device:MediaRenderer:1,upnp:rootdevice accept

# Sonos: one host, by name
discoctl rule add in lan out media kind query service _sonos._tcp accept
discoctl rule add in media out lan kind response,announce host "sonos*" accept
```

Discovery is not the data path. Once a client has found a service it connects to the address in the A record,
and that connection needs its own firewall rule (TCP 51826 for HomeKit, 631 for IPP, 7000 and 7100 for AirPlay,
8008 and 8009 for Cast, 1400 for Sonos, and so on). A later version is to maintain the addresses of allowed hosts
in nftables sets so that one static rule per network pair is enough.

## discoctl

```
discoctl status                 interfaces, state, joined groups, addresses, counters
discoctl rule list|add|insert N|del N|flush|test …
discoctl save | apply           write the rules file | reload it
discoctl hosts                  the table: hosts, addresses, instances, per interface
discoctl services               the instances: service type, host, port, TTL left
discoctl stats                  counters per interface and per rule
discoctl watch [--iface X]      live: one line per packet with its decisions
```

Every command takes `--json` for scripts, and with it `rule add` and `rule insert N` also take the rule as one
JSON object (`discoctl rule add --json '{"in":"iot","out":"lan","host":["bridge"],"action":"accept"}'`); `--socket` or `DISCOWALL_SOCKET` names the control socket. Exit codes:
0 ok, 1 the daemon refused, 2 usage, 3 the daemon is unreachable. A `watch` line:

```
14:02:11.482 iot      10.1.0.5:5353         announce  _hap._tcp "living room" bridge.local   -> lan sent(2)  guest dropped
```

The result per target interface is `sent`, `partial` (some records dropped: the count follows), `dropped`, `down`
or, for one-shot queries, `asked` and `relayed`. The rules that decided are in parentheses.

## How it works

- One UDP socket per address family on port 5353, bound to the wildcard address with `SO_REUSEADDR` and
  `SO_REUSEPORT`, so avahi can hold the port too; `IP_PKTINFO` tells which interface a packet arrived on, and the
  same control message chooses the egress interface per send. The group is joined on every configured interface
  as rtnetlink reports it up. No raw sockets, no capabilities.
- Every packet is parsed (hickory-proto). The host table learns from every packet, forwarded or not. For each
  target interface the chain decides per question and per record; the packet is re-encoded if anything was dropped,
  the QU bit is cleared, link-local address records are removed, the TTL is 255.
- Loops: never back onto the arriving interface, packets from the router's own addresses are dropped, an identical
  packet within the dedup window is dropped.
- SSDP (UPnP, 239.255.255.250:1900 / ff02::c) uses a second pair of sockets. NOTIFY messages are forwarded under
  the rules; an M-SEARCH is always proxied, because UPnP answers go by unicast to the searcher's port: the daemon
  searches on the allowed interfaces from an ephemeral socket, collects the answers for MX seconds and relays them
  to the searcher from port 1900. A LOCATION on a link-local address is dropped.
- A query from an ephemeral port (`dig @224.0.0.251 -p 5353`, one-shot libraries) cannot be reflected: the
  daemon asks on the allowed interfaces from an ephemeral socket of its own, collects the unicast answers for a
  short window, runs them through the rules and relays them to the querier by unicast from port 5353.
- Tests run in Linux network namespaces: a router namespace with the daemon between two veth leaves, real
  multicast on both address families, rules driven through `discoctl`, the legacy proxy end to end.

## Status

The research of 10 October 2026 (survey of avahi and the existing reflectors, design, assessment of Rust, effort
estimate, record of the decisions) is in [mdns-router-research.md](mdns-router-research.md). The decisions are in
[roadmap-items/discowall/decisions.md](roadmap-items/discowall/decisions.md), the tasks one by one in
[roadmap-readmes/discowall.md](roadmap-readmes/discowall.md), the open questions in
[roadmap-items/discowall/overview.md](roadmap-items/discowall/overview.md).

## Roadmap

- 🕵️ **P0:** Cargo project, sockets, interface tracking over netlink, plain reflector with loop prevention,
  systemd unit
- ✅ **Lab:** measure coexistence with avahi-daemon on port 5353 (measured 2026-10-13: multicast to both, unicast to the socket bound last, no storm)
- 🕵️ **P1:** parser, host table and record bundles, rule chain, control socket, `discoctl rule …`
- 🕵️ **P2:** `discoctl watch`, `hosts`, `services`, `stats`, `status`; per-rule counters, `log`
- 🕵️ **P3:** proxy for one-shot queries and the QU bit, link-local AAAA stripping, IPv6 end to end,
  deduplication, fuzzing, unit hardening
- 🕵️ **P4:** packages (tarball, deb, rpm, OpenWrt), cross builds, release workflow, README and man pages
- 💤 nftables sets with the addresses of allowed hosts
- 🕵️ SSDP (UPnP) as a second protocol
- 💤 FreeBSD backend and pfSense package
- ✅ Proxmox lab for end-to-end tests (four LXC on a VLAN-aware bridge, lab.py with nine cases, all green 2026-10-13)
- ✅ avahi inside the namespace test on GitHub (green in CI 2026-10-13)
- ✅ B-1: log output carried ANSI colour codes in the journal (found and fixed in the lab)
- ✅ B-2: dedup dropped the IPv4 twin of a query avahi sends on both families at once (found and fixed in the lab)
- ✅ B-3: `discoctl save` failed under the shipped unit; discoctl writes the rules file now (found and fixed in the lab)
- 🕵️ LuCI app for the OpenWrt package (web UI for rules, hosts, services, status; tested on an OpenWrt 24.10 VM in the lab 2026-10-13)
- 🤷 pfSense package and web UI (FreeBSD port, packagegui, rules page; OPNsense plugin to be decided)
- 🤷 IGMP snooping without a querier kills SSDP minutes after start (measured on a Linux bridge): document, or refresh memberships, or be the querier
- 📥 OpenWrt packages as release assets (luci-app `all`, discowall `.ipk`/`.apk` per architecture from the musl binaries)
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
