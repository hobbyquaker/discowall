# mdns-router: mDNS forwarding between a router's interfaces, under rules — research

**Written** 2026-10-10 by Fable for the maintainer. Research only: nothing implemented, no toolchain installed, no
remote created, nothing decided. First document for discussion; the open questions are in section 7 and in
`roadmap-items/mdns-router/overview.md` (OQ-1…OQ-9).

**The ask** (maintainer, 2026-10-10, verbatim): *"this should become a tool that sits on a linux router and forwards
mdns messages from on interface to another (as avahi-daemons reflection feature does). but with more control: it should
be configurable through rules which hosts may be forwarded to which interface. it should have possiblity of
whitelisting/blacklisting and src/target based rules. maybe like a simple form of iptables. with cli tools to list
and set rules, see which mdns announcements arrive on which iface and so on but also a good cli to use it in scripts
or smth. i think it should be implemented in rust. what do you think about that? effort? feasibility? create a first
research markdown for later discussions with me"*

## 0. In short

- **Feasible, and not large.** An mDNS reflector is a UDP socket per address family on port 5353, a multicast group
  join per interface, "which interface did this arrive on" from the kernel (`IP_PKTINFO`), and a re-send on the other
  interfaces with the sender's own address. mdns-repeater does that in about 500 lines of C. What the ask adds — a
  parser for the DNS wire format, a rule chain, a control socket and a CLI — is ordinary, well-bounded work. No raw
  sockets, no root after the start, no kernel module.
- **The control is the product.** Every existing tool does one of two things: reflect everything (mdns-repeater,
  mdns-reflector, avahi without filters) or filter by a flat list of service types or host names on all interfaces
  alike (avahi's `reflect-filters`). None has **direction** (host X on *iot* may be seen from *lan*, not the other way),
  none has **source rules** (only this address may announce), none shows what arrives where, none has a scriptable
  CLI. Cisco's "Service Discovery Gateway" (service lists permitted per interface, in and out) is the closest
  conceptual precedent, and that is a commercial router feature, not a Linux tool. Section 2 has the survey.
- **The one genuinely hard part is record consistency**, not sockets: a browse for `_hap._tcp` is answered by a PTR, an
  SRV, a TXT and an A/AAAA, often in different packets, and an allowed service is useless when its host's A record is
  dropped. A rule on "host X" or "service Y" must therefore apply to a *bundle* — the daemon keeps a small cache
  that maps instance ↔ host ↔ addresses and decides on the bundle. Avahi has that cache because it is a full stack;
  the plain reflectors avoid the problem by not filtering. Section 3.5 designs it; it is the part to prototype first.
- **Rust fits.** A long-running, small, allocation-shy network daemon on a router — a single static musl binary of a
  few MiB, 3–6 MiB RSS, cross-compiled for aarch64/armv7/x86_64, packaged for Debian and OpenWrt (OpenWrt builds Rust
  packages since 23.05) — is what the language is good at, and the untrusted-input parser is where its guarantees
  pay. The crates needed exist (section 4). The cost is the toolchain, which is not on this machine, and that the
  maintainer's other projects are Go and Node: no shared code, a second language to keep warm. Go would do the job
  as well; Rust is the better fit for *this* kind of program and the maintainer's wish. Recommendation: Rust.
- **Effort:** roughly **20–30 person-days** for a complete v1 (reflector, parser, rule engine with bundles, config,
  CLI with `--json`, live watch, host table, dedup, IPv6, legacy unicast queries, deb and OpenWrt packages, CI with
  network-namespace tests), an **MVP in 8–11** (reflect under direction and host/service rules, CLI list/add/del/watch,
  systemd unit). Phases in section 6.
- **What needs the maintainer's word before anything is filed:** the target router (Debian/nftables, OpenWrt,
  container?), beside or instead of avahi on the same host, the rule syntax (an iptables-like CLI over a TOML file is
  proposed), whether legacy unicast queries are proxied in v1, MAC-based rules, the name, the licence. Section 7.

## 1. What mDNS is, as far as a reflector must care (RFC 6762 / 6763)

Facts that constrain the design. Where a fact comes from the RFC it is marked **[rfc]**; where it is from the survey
or general implementation knowledge it is marked **[impl]** and is to be verified in the lab before it is relied on.

- **Transport.** UDP, multicast group `224.0.0.251` / `ff02::fb`, port 5353, link-local scope by definition: routers
  do not forward it, which is the whole reason for a reflector. **[rfc]** The IP TTL / hop limit of mDNS packets
  **should be 255**, and some responders check it; a reflector re-sends with TTL 255, not the default 1. **[rfc]**
- **Two kinds of queriers.** A *fully compliant* querier sends from source port 5353 and expects multicast responses;
  a *one-shot* (legacy) querier sends from an ephemeral port and expects a **unicast** response to that address and
  port — `dig @224.0.0.251 -p 5353`, many small libraries, some IoT firmware. A response to a legacy query cannot be
  "reflected": it goes unicast to a host on another segment and must be *proxied* — the daemon asks on the target
  segment itself and relays the answer back to the original source. Avahi's man page says the reflector loses "the
  unicast reply bit"; the plain reflectors ignore the case entirely. **[rfc, impl]**
- **The QU bit.** A question can request a unicast response (the "QU" bit) even from port 5353. A reflected QU
  question would make responders unicast to the *router's* address on their segment (the packet's source after
  reflection), not to the querier. The daemon either clears the bit before reflecting or accepts the unicast answer
  itself and multicasts it on the querier's segment. Clearing is what to do first. **[rfc]**
- **Record data is not rewritten.** A/AAAA records carry the host's real addresses, SRV the host name and port.
  Across segments that means: **the data path must be routable and firewalled separately** (nftables rules for TCP
  631, 7000, 51826, …) — discovery working while the connection fails is the standard complaint in every forum thread
  surveyed. The research proposes an optional nftables-set feed for exactly that (3.8). IPv6 **link-local AAAA**
  (`fe80::/10`) are meaningless on another segment and are known to make clients prefer a dead address; the daemon
  strips them (an option, default on). IPv4/IPv6 cross-reflection (avahi's `reflect-ipv`) is not done. **[impl]**
- **Loops and duplicates.** Never re-send on the receiving interface; drop packets whose source is one of the daemon's
  own addresses (its own reflections arrive on its own socket — `IP_MULTICAST_LOOP` off helps but is not enough with
  several sockets); keep a short dedup cache (packet hash, ~1 s) so that two reflectors on overlapping segments do not
  "play ping-pong" (avahi's man page warns of this). **[impl]**
- **Probing and conflicts.** A host that announces a name probes for it first; if its probe is reflected to a segment
  where another host owns the same name, both see a conflict and rename (`Printer (2)`). That is correct and desired
  when the two segments are meant to be one name space, and a reason to make reflection of *probes and announcements*
  (responses with the cache-flush bit) a rule-controlled thing like everything else, not special. **[rfc, impl]**
- **Known-answer suppression and TTLs.** Responders suppress answers the querier already listed; a reflector that
  forwards the question unchanged keeps that working. Record TTLs are the responder's and are forwarded unchanged;
  the daemon's own host table (3.6) expires on them. **[rfc]**
- **Source address after reflection.** The reflected packet's IP source is the router's address on the target
  interface. Rules on "source" therefore mean the *original* source as seen on the in-interface — the daemon knows it,
  downstream hosts never do. **[impl]**

## 2. Existing tools

| Tool | Language, licence | Filtering | Direction | Observability / CLI | State |
| --- | --- | --- | --- | --- | --- |
| **avahi-daemon reflector** (`enable-reflector`, `reflect-filters`, `reflect-ipv`) | C, LGPL | one flat list of service types *or* host names, applied on all interfaces alike; everything else reflected | none | none for the reflector; `avahi-browse` shows what avahi itself sees | the de-facto standard; `reflect-filters` came from a 2014 community patch that waited years for review; needs the whole daemon with D-Bus; man page admits the lost unicast-reply bit and the ping-pong risk |
| **mdns-repeater** (kennylevinsen) | C, GPL | none | none | none; command line lists the interfaces | the original, ~500 lines, IPv4 only, discontinued; packaged in FreeBSD and Fedora |
| **mdns-reflector** (vfreex) | C, GPL-3.0 | none | none ("unidirectional announcement planned") | none | 249 stars, 21 commits, 11 open issues; IPv4+IPv6, epoll, "zones" = sets of interfaces per instance |
| **mdns-relay** (engel0) | Rust, ? | ? | uni- or bidirectional per interface pair | ? | described on Docker Hub (TOML config, TTL-based dedup cache, tokio, "needs NET_RAW"); the GitHub URL answered 404 on 2026-10-10 — **unverified**, treat as a hint that someone wrote the socket part in Rust |
| **mdns-repeater** (flumeware) | Go, BSD-3 | substring match on names → proxies the query by unicast to a configured target | implicit (targets) | none | 2 commits, 1 star; a different model (a proxy with a static target list), interesting only as a shape for the legacy-query proxy |
| a Python reflector "like avahi-reflector but with fine-grained control" (GitHub topic listing) | Python | yes, per the description | ? | ? | not inspected |
| **umdns** (OpenWrt) | C | — | — | `ubus` interface, shows hosts and services per interface | an mDNS responder and browser, **no reflector**; its ubus view is a good model for `mdnsctl hosts` |
| **Cisco Service Discovery Gateway**, UniFi "mDNS reflector", pfSense/OPNsense (= avahi) | commercial / avahi | Cisco: service lists permitted per interface, in and out ("mdns service-policy"); UniFi: on/off per network | Cisco: yes | Cisco: `show mdns cache` | the model for the rule language is Cisco's, not avahi's |

**What is missing everywhere** (the ask, in the survey's words): direction, source rules, host-level allow/deny with
record consistency, an inspectable live view, a scriptable CLI, a daemon that runs without a full mDNS stack beside
it. That is the product.

## 3. Design proposal

### 3.1 Process shape and sockets

One daemon, working name **`mdns-routerd`**, and one CLI, working name **`mdnsctl`**, over a unix socket
(`/run/mdns-router/control.sock`, JSON lines, one request/one response, plus a streaming mode for `watch`). Names are
OQ-2.

- **Sockets:** one `AF_INET` and one `AF_INET6` UDP socket, bound to `0.0.0.0:5353` / `[::]:5353` with `SO_REUSEADDR`
  (and `SO_REUSEPORT`, see below), `IP_PKTINFO` / `IPV6_RECVPKTINFO` so that every `recvmsg` carries the arriving
  interface index and destination address, `IP_ADD_MEMBERSHIP` / `IPV6_JOIN_GROUP` **once per configured interface**,
  `IP_MULTICAST_LOOP` off, TTL/hop limit 255. Sending: `IP_MULTICAST_IF` / `IPV6_MULTICAST_IF` chosen per packet via
  `sendmsg` control data, so one socket serves all interfaces. No `AF_PACKET`, no `NET_RAW`: port 5353 is unprivileged,
  multicast joins are unprivileged, so the daemon runs as its own user from the start, under a systemd unit with
  `ProtectSystem=strict`, `PrivateTmp`, `NoNewPrivileges` and `RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
  AF_NETLINK`.
- **Beside avahi** (OQ-5): on Linux, multicast *delivery* to several sockets bound with `SO_REUSEADDR` on the same port
  works (that is how avahi and systemd-resolved coexist); **unicast** to 5353 (legacy-query responses, QU answers)
  reaches only one of them. If the daemon shares a host with avahi, it must not depend on receiving unicast on 5353 —
  the legacy-query proxy (3.7) then uses an ephemeral port for its own questions, which is correct anyway. Avahi's
  `disallow-other-stacks=yes` would block the bind; it defaults to `no`. The
  FreeBSD report that `SO_REUSEPORT` is not enough is not Linux.
  **[measured 2026-10-13, task 10, Proxmox lab: Debian 13 LXC, kernel 7.0 of the PVE 9.2 host, avahi 0.8, discowalld
  0.0.2-dev; `tests/lab/lab.py run coexistence`]** Both daemons bind `0.0.0.0:5353` and `[::]:5353` side by side.
  (1) Multicast reaches both: avahi on the router answers `router.local` while discowall forwards the browse of a
  printer from the other interface. (2) Unicast to port 5353 reaches **the socket bound last** — with discowalld
  restarted after avahi, a unicast query from an ephemeral port and one from port 5353 both show up in `discoctl
  watch` and avahi does not answer; with avahi restarted after discowalld, avahi answers and discowall sees nothing.
  So the proxy's ephemeral socket is the right design (3.7); a reboot order must never matter, and it does not.
  (3) avahi's own reflector on as well (`enable-reflector=yes`): no storm — 8 packets on the lan interface in 10 s
  around a fresh announcement and a browse; avahi's copies carry the router's source address and fall to
  discowall's own-source drop, the rest to the dedup window; the printer still browses.
- **Interfaces** are named in the config (`lan`, `iot`, `guest`); the daemon watches **rtnetlink** for link and address
  changes, joins the groups when an interface appears or comes up, leaves when it goes, and keeps its own address
  list current (needed for the loop check). An interface that does not exist yet is fine: configured, waiting.

### 3.2 Packet pipeline

```
recvmsg ─► own-source? drop ─► dedup cache ─► parse (DNS wire format, tolerant)
        ─► classify: query | response, questions[], records[] (name, type, class, cache-flush, data)
        ─► derive: service types, instance names, host names, addresses; source iface, source addr
        ─► for each candidate out-iface ≠ in-iface:
               evaluate rule chain (in, out, src, kind, names) → accept | drop, possibly per record
               rebuild the packet with the surviving records (strip fe80 AAAA, clear QU, fix counts)
               sendmsg on out-iface, TTL 255
        ─► host table update, counters, watch stream event (iface, src, kind, names, decision, rule id)
```

A packet whose every record is dropped is not sent. A query is forwarded as a whole or not (questions are not
filtered record by record in v1; a question for a denied name is simply not forwarded, see 3.4 on direction).

### 3.3 Parser

mDNS packets are plain DNS wire format (RFC 1035 header, questions, RRs, name compression) with two mDNS bits
(QU in the question's class, cache-flush in the RR's class). Two options:

- **hickory-proto** (the former trust-dns, 0.26.x, `mdns` feature): complete, fuzzed, "zero panics, only safe Rust",
  handles compression, every record type, and can serialize a rebuilt `Message`. Cost: a large dependency for a tiny
  need, and a *rebuilt* message is not byte-identical to the original (compression pointers differ) — harmless for
  mDNS, but worth knowing.
- **An own parser** (~400–600 lines with tests): tolerant of the malformed packets IoT devices send (forward unparsed
  bytes untouched when the rule needs no record detail), byte-identical re-send when nothing is filtered, exact
  control over what is rebuilt. More code to own, less dependency.

Proposal: start with hickory-proto behind a small internal trait (`parse → Packet`, `Packet → bytes`), so that an own
parser can replace it later if size or tolerance demands it. Fuzz the boundary either way (`cargo fuzz`), because
the input is anything a device on the guest VLAN sends.

### 3.4 The rule model — "a simple form of iptables"

An **ordered chain**, first match wins, a **default policy** (`drop` — nothing crosses that is not allowed; the
maintainer may prefer `accept` for a blacklist setup, so it is a setting), and one rule = matches + action:

| Match | Meaning | Example |
| --- | --- | --- |
| `in` | arriving interface (name, or `any`) | `in iot` |
| `out` | target interface | `out lan` |
| `src` | original source address or prefix, v4 or v6 | `src 192.168.30.0/24`, `src 192.168.30.17` |
| `kind` | `query`, `response`, `announce` (response with cache-flush), `probe` | `kind response` |
| `service` | service type, glob | `service _hap._tcp`, `service _airplay._tcp,_raop._tcp` |
| `instance` | service instance name, glob | `instance "Living Room*"` |
| `host` | host name (`.local` implied), glob | `host printer*` |
| `rrtype` | record type | `rrtype AAAA` |
| action | `accept`, `drop`, with `log` as a flag | `accept log` |

Direction falls out of `in`/`out` plus `kind`: *"host X on iot may be seen from lan"* is

```
rule add in lan out iot kind query  service _hap._tcp  accept   # lan may ask iot for HomeKit
rule add in iot out lan kind response host x            accept   # only X's answers come back
default drop
```

Whitelist = default `drop` plus accept rules; blacklist = default `accept` plus drop rules; both are just the chain.
A rule on `service`/`instance`/`host` is applied to the **bundle** (3.5), so "host x accept" lets X's PTR, SRV, TXT, A
and AAAA through, and nothing of any other host in the same packet.

**Where the rules live:** a TOML file (`/etc/mdns-router/rules.toml`, one `[[rule]]` per line, numbered on load) that
the CLI edits and the daemon reloads atomically on `SIGHUP` or `mdnsctl apply`. The CLI mirrors iptables' verbs
(`rule list`, `rule add`, `rule insert N`, `rule del N`, `rule flush`, `save`), and `rule list --json` and
`rule add --dry-run` make it scriptable. An nftables-like text format loaded as a whole is the alternative (OQ-6);
the proposal is the CLI-over-TOML because the ask says "list and set rules".

### 3.5 Record bundles (the hard part)

Service discovery is a graph: `_hap._tcp.local PTR "Living Room._hap._tcp.local"` → `SRV bridge.local:51826` + `TXT`
→ `bridge.local A 192.168.30.17`. The PTR, SRV/TXT and A may arrive in one packet (announcements) or three (answers
to three questions, each a different responder decision). A rule "host bridge accept" must pass all of them, and a
rule "service _hap._tcp accept" must pass the A record of a host it has never seen in a `_hap` context *if* that
host is known to carry `_hap` instances.

The daemon therefore keeps a **host table** per in-interface, learned passively from everything it sees (allowed or
not): instance → service type, instance → host, host → addresses, with the records' TTLs. The rule engine resolves a
name to its bundle through the table: an A record for `bridge.local` is attributed to every instance on `bridge`,
and passes if any of those is accepted (or if the host itself is). Cold-start gap: before the table knows the SRV,
an A record cannot be attributed — acceptable, because a client that got the PTR asks for SRV and A again within a
second, and announcements carry the whole bundle in one packet. The table is also what `mdnsctl hosts` shows.

Prototype this first (phase 1), with recorded packet captures from the lab as fixtures.

### 3.6 Observability and the CLI

- `mdnsctl watch [--iface X] [--json]` — a live line per packet: time, in-iface, source, kind, names, the decision
  and the rule that made it, the out-ifaces it went to. The thing that is missing from every tool surveyed.
- `mdnsctl hosts [--iface X] [--json]` — the host table: hosts, addresses, instances, service types, first/last seen,
  TTL left, per in-interface (umdns' ubus view is the model).
- `mdnsctl services`, `mdnsctl stats` (per interface and per rule: packets, bytes, hits, drops), `mdnsctl status`
  (interfaces, groups joined, addresses, config loaded at, errors), `mdnsctl rule …` (3.4), `mdnsctl test <packet
  spec>` (which rule would match: `test --in iot --out lan --kind response --host x`).
- Every command has `--json`; exit codes are meaningful (0 ok, 1 denied/not found, 2 usage, 3 daemon unreachable);
  nothing interactive. Logging via `tracing` to the journal; `log` on a rule emits a structured line per hit.

### 3.7 Legacy unicast queries and QU

v1: clear QU bits before reflecting; forward legacy queries (ephemeral source port) *as questions from the daemon
itself* on the target interfaces from an ephemeral port, collect the unicast answers for a short window
(RFC: responders answer within ~500 ms) and send them unicast back to the original source. That is a small proxy,
about a day, and it is what avahi does not do. OQ-7 asks whether it is in v1 or v2; the proposal is v1, because
"`dig` works" is how people test.

### 3.8 Optional: feeding the firewall

Discovery alone does not make a connection work (1.). A later phase can have the daemon maintain **nftables sets**
(`@mdns_allowed_iot_to_lan` = the addresses of hosts whose services were accepted from *iot* to *lan*), so one static
nft rule per interface pair opens the data path for exactly the hosts the mDNS rules allow. That turns the tool from
a reflector into the "simple form of iptables" of the ask in both senses. Not v1 (OQ-9).

### 3.9 Out of scope

Being an mDNS responder or resolver (no announcing the router's own services, no `.local` lookup for the router's
resolver), DNS-SD to unicast-DNS gateway (RFC 6763 wide-area), IPv4/IPv6 cross-reflection, Windows, BSD (the socket
code is Linux-specific by design; a BSD port is possible, not planned).

## 4. Rust: the assessment

**What fits.** The program is a long-running network daemon on a small box: steady memory, no GC, a parser for
untrusted input, a few threads or an async loop, a static binary. Rust gives a 2–4 MiB stripped musl binary, 3–6 MiB
RSS, no runtime, memory safety in the parser, and a cross-compile story that is a one-liner once set up. OpenWrt's
packages feed builds Rust packages since 23.05 (`rust-lang` host tool, `cargo` packaging macros), Debian packaging
via `cargo-deb` is trivial, and `cross` or `cargo-zigbuild` builds aarch64/armv7/x86_64 from this WSL box.

**Crates** (all stable, widely used; versions to pin at project start):

| Need | Crate | Note |
| --- | --- | --- |
| sockets with `IP_PKTINFO`, `recvmsg`/`sendmsg`, multicast joins per ifindex | `socket2` + `nix` (or `libc`) | tokio's `UdpSocket` has no `recvmsg` with control data; wrap a `socket2` socket in tokio's `AsyncFd`, or run the two receive loops as plain threads — the packet rate is tiny |
| async runtime, unix socket server, timers | `tokio` | optional; two blocking threads and a channel would do as well |
| interface/address events | `rtnetlink` (+ `netlink-packet-route`) | or `nix`' `getifaddrs` polled every few seconds for v1 |
| DNS wire format | `hickory-proto` (`mdns` feature) | or an own parser, 3.3 |
| CLI | `clap` (derive) | `--json` everywhere |
| config | `serde` + `toml` | atomic reload |
| logging | `tracing`, `tracing-journald` | |
| hashing for dedup | `ahash` / std | |
| fuzzing | `cargo-fuzz` | the parser boundary |
| tests | std `#[test]`; integration tests spawn `ip netns` + veth | root or `unshare -rn` in CI |

**What it costs.** No toolchain on this machine today (`rustup` installs one in minutes). The maintainer's other
projects are Go (occulited) and Node (everything else): no shared libraries, no shared idioms, a language to keep
warm. Build times are longer than Go's; the first `cross` setup costs an afternoon. Against Go honestly: Go would
deliver the same tool in about the same time with a 10–15 MiB binary and 10–15 MiB RSS, `golang.org/x/net/ipv4` has
`ControlMessage` with the ifindex built in, and the openccu-lite decision D-15 chose Go for reasons that partly apply
here too (one language, cross-compiles, no hot loop). What differs here: this *is* a packet path, the binary goes
on routers where every MiB counts (OpenWrt), the input is hostile, there is no Homematic ecosystem to reuse, and the
maintainer wants Rust. **Recommendation: Rust**, as a decision to be written down with these reasons (D-1).

## 5. Platforms, packaging, tests

- **First target** (OQ-4): a Debian router with nftables is the easiest (systemd unit, `.deb`, the maintainer's
  tooling); OpenWrt second (musl, procd init script, `uci` config bridge optional, feed package); a container with
  `--network host` works without any change.
- **CI:** GitHub Actions, `cargo fmt --check`, `clippy -D warnings`, `cargo test`, the netns integration tests under
  `sudo` on the ubuntu runner (allowed there), cross builds for three targets, release artifacts. The integration
  tests are the valuable part: two or three namespaces joined by veth pairs to a "router" namespace running the
  daemon, a small mDNS responder/querier in the test (hickory-proto can build the packets) on each side, assertions
  on what crosses under which rules. Cheap, realistic, no hardware.
- **Lab:** the maintainer's real router and VLANs for the avahi-coexistence check (3.1), the IoT devices for parser
  tolerance (capture with `tcpdump -i X udp port 5353 -w` and keep the captures as fixtures, anonymised).

## 6. Effort

Person-days of focused agent work, including tests and docs, excluding the maintainer's reviews and lab time.

| Phase | Content | Days |
| --- | --- | --- |
| P0 | Cargo project, sockets per 3.1, netlink interface tracking, reflect everything between configured interfaces with loop prevention and dedup, TOML config, systemd unit, journald logging | 2–3 |
| P1 | parser (hickory behind a trait), classification, host table and bundles (3.5), rule chain (3.4), control socket, `mdnsctl rule list/add/insert/del/flush/save/apply/test`, `--json`, unit tests with captured fixtures, first netns integration test | 5–8 |
| P2 | `watch`, `hosts`, `services`, `stats`, `status`; per-rule counters and `log` | 3–4 |
| P3 | legacy-query proxy and QU handling (3.7), fe80 stripping, IPv6 end to end, dedup against a second reflector, fuzzing of the parser, hardening of the unit | 4–6 |
| P4 | `cargo-deb`, OpenWrt package and procd script, cross builds and release workflow, README with the rule language and recipes (HomeKit, AirPlay, printers, Chromecast), man pages from clap | 3–4 |
| P5 (optional) | nftables set feed (3.8) | 2–3 |
| **v1 (P0–P4)** | | **17–25**, plus 3–5 for the unforeseen: **20–30** |
| **MVP (P0+P1, `watch` from P2)** | | **8–11** |

Where the estimate is soft: the bundle logic against real devices (Sonos, Chromecast and HomeKit bridges each have
their own habits), and the avahi coexistence if OQ-5 is "beside".

## 7. Open questions for the maintainer

The list is also OQ-1…OQ-9 in `roadmap-items/mdns-router/overview.md`; in the order they block work:

1. **Target router and packaging first** (OQ-4): Debian/nftables, OpenWrt, container?
2. **Beside or instead of avahi** on that host (OQ-5)?
3. **Rust** — yes as proposed in section 4 (D-1)?
4. **Rule syntax** (OQ-6): iptables-like CLI over TOML as in 3.4, or an nft-like text file?
5. **Legacy unicast queries** in v1 (OQ-7)? Proposal: yes.
6. **MAC-based rules** (OQ-8)? Not in v1 unless wanted; they need `AF_PACKET` or a neighbour-table lookup per packet.
7. **nftables set feed** (OQ-9): a later phase, or not at all?
8. **Name, licence, remote** (OQ-2, OQ-3), and whether the agent documents stay in the code repository (OQ-1).
9. **Anything to reflect that is not mDNS?** SSDP (UPnP, 239.255.255.250:1900) is the sibling problem with the same
   shape; the socket and rule layers would carry it with a second parser. Out of scope unless the maintainer wants it
   on the list.

## 8. Sources

Surveyed on 2026-10-10. Forum threads are evidence of symptoms, not of causes.

- avahi-daemon.conf(5): [Debian manpage](https://manpages.debian.org/avahi-daemon.conf(5)),
  [Arch manpage](https://man.archlinux.org/man/avahi-daemon.conf.5) — `enable-reflector`, `reflect-ipv`,
  `reflect-filters`, `disallow-other-stacks`, the unicast-reply-bit and ping-pong notes
- the `reflect-filters` patch discussion: [lathiat/avahi PR 62](https://github.com/lathiat/avahi/pull/62)
- ASTLinux's reflector guide with the printer example: [doc.astlinux-project.org](https://doc.astlinux-project.org/userdoc:tt_avahi_mdns)
- mdns-repeater (kennylevinsen): [FreshPorts](https://www.freshports.org/net/mdns-repeater),
  [Fedora spec](https://download.copr.fedorainfracloud.org/results/jakub/gcc-13-test/fedora-41-i386/05173089-mdns-repeater/mdns-repeater.spec)
- mdns-reflector (vfreex): [github.com/vfreex/mdns-reflector](https://github.com/vfreex/mdns-reflector)
- mdns-repeater (flumeware, Go): [github.com/flumeware/mdns-repeater](https://github.com/flumeware/mdns-repeater)
- mdns-relay (Rust, unverified): [Docker Hub listing](https://hub.docker.com/r/engeloded/mdns-relay)
- hickory-proto 0.26.1 features: [Debian codegraph](https://codegraph.jelmer.uk/rust-hickory-proto/0.26.1-1/);
  Rust mDNS crates for reference: [mdns-sd](https://docs.rs/crate/mdns-sd/0.20.3/source/README.md),
  [multicast-socket](https://github.com/PhysicalGraph/multicast-socket)
- symptoms across VLANs: [OpenWrt: mDNS reflector problems](https://forum.openwrt.org/t/mdns-reflector-problems/191228),
  [IPFire: reflection not working](https://community.ipfire.org/t/help-avahi-mdns-reflection-between-vlans-not-working/15435),
  [Netgate: IPv6 link-local source](https://forum.netgate.com/post/788974),
  [Zima: same name resolves differently](https://shop.zimaspace.com/blogs/support-tips/same-mdns-name-resolves-differently-across-home-vlans)
- SO_REUSEPORT beside avahi, FreeBSD report: [ports commit](https://git.disroot.org/kpriv/freebsd-ports/commit/0248b2740959f9400189e87c4fa5b73b7774e5cc);
  avahi's own TODO on it: [docs/TODO](https://nest-open-source.googlesource.com/manifest_repos/avahi/+/refs/heads/main/docs/TODO)
- RFC 6762 (Multicast DNS) and RFC 6763 (DNS-SD): read from memory for section 1; the **[rfc]** marks are to be
  checked against the text before they become decisions.

## 9. Q&A outcome (maintainer and Fable, 2026-10-10, after the research above)

The decisions are D-1…D-9 in `roadmap-items/discowall/decisions.md`; this is the record of the discussion.

- **Platform:** *"debian (ubuntu, armbian), arch, fedora, openwrt, pfsense"* — broader than the research assumed, and
  pfSense is FreeBSD. Resolution: Linux first, a static musl tarball with a systemd unit covers the glibc distros at
  once, OpenWrt in P4, FreeBSD/pfSense later behind a socket trait (D-4).
- **Avahi:** the maintainer asked what else avahi does that users need and whether "instead of avahi" would grow the
  project beyond its purpose. Answer: avahi is a full stack — responder (the host's own `router.local`, SSH, CUPS,
  Samba), resolver (`nss-mdns`), D-Bus/C API for CUPS, GNOME, KDE, PulseAudio — and the reflector is a side feature.
  discowall covers only the reflector and must not grow into the rest; "instead" was the wrong framing, **coexistence
  on the same host is the requirement** (D-3), verified in the lab (task 10).
- **Rules:** iptables-like CLI over TOML, as proposed in 3.4 (D-5). **Rust:** yes (D-1). **Legacy queries:** proxied
  in v1 (D-6). **MAC rules:** no (D-7). **nftables feed:** later phase (D-8). **Agent documents:** stay here, public
  (D-9). **Licence:** MIT; **remote:** GitHub `hobbyquaker/…`, the maintainer pushes (D-2, D-9).
- **SSDP:** wanted — *"but then our project name should change, shouldn't it?"* Yes. Suggestions went from
  zeroconf-router / sdgate / discovery-router over the maintainer's discogw / disrouter / discovrules to discofw /
  discogw / discowall / disco; chosen: **discowall** — discovery firewall — with `discowalld` and `discoctl` (D-2).
  The repository is renamed by the maintainer; this file keeps its name as the record of where it started.
- **Open after the Q&A:** the default policy of a fresh install (OQ-10) and the lab for task 10 (OQ-11).
