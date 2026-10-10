## Decisions

Decisions are made by the maintainer only; a change is recorded here with its date. D-1…D-9 come from the Q&A of
2026-10-10 on [../../mdns-router-research.md](../../mdns-router-research.md) (its section 9 has the record).

| | Decision (2026-10-10) |
| --- | --- |
| D-1 | **Rust.** Stable toolchain, static musl binaries for aarch64/armv7/x86_64; the reasons are research section 4 (a long-running, allocation-shy network daemon on routers, a parser for hostile input, OpenWrt's Rust support). Go was weighed and would have done the job; Rust is the better fit for this kind of program and the maintainer's wish. |
| D-2 | **The name is `discowall`** (discovery firewall): daemon `discowalld`, CLI `discoctl`, GitHub `hobbyquaker/discowall`, **MIT**. Chosen over mdns-router because SSDP joins later (D-3) and over discofw/discogw/disco. The local directory `~/repos/mdns-router` is renamed by the maintainer. |
| D-3 | **Scope: a discovery forwarder with rules, nothing more.** mDNS first, **SSDP (UPnP, 239.255.255.250:1900) later** as its own phase with the socket and rule layers written so it fits. Never an mDNS responder or resolver, no D-Bus API, no `.local` lookup for the host: that is avahi's job, and **avahi may run on the same host** — coexistence on port 5353 is a requirement, verified in the lab (task 10), and the legacy-query proxy never relies on unicast to 5353. |
| D-4 | **Platforms: Linux first** — Debian/Ubuntu/Armbian, Arch, Fedora, OpenWrt; a static musl tarball with a systemd unit covers the first four from day one, deb/rpm/OpenWrt packages are P4. **FreeBSD/pfSense later** (task 9): the socket layer sits behind a trait from the first commit so that a kqueue/`IP_RECVIF` backend is an addition, not a rewrite. |
| D-5 | **The rule model** is an ordered chain, first match wins, a default policy (`drop` or `accept`), matches on in/out interface, original source address or prefix, kind (query, response, announce, probe), service type, instance, host, record type; actions accept/drop with a `log` flag; a name rule applies to the record **bundle** (research 3.5). **Edited through an iptables-like CLI over a TOML file** (`discoctl rule add|insert|del|list|flush`, `save`, `apply`, `test`), reloaded atomically. |
| D-6 | **Legacy one-shot queries (ephemeral source port) and the QU bit are proxied in v1** (research 3.7): the daemon re-asks on the target interfaces from an ephemeral port and relays the answers unicast to the original source. |
| D-7 | **No MAC-based rules.** Interface, IP address/prefix and names are the match keys; plain UDP sockets, no `AF_PACKET`, no `NET_RAW`, the daemon runs as its own user. |
| D-8 | **The nftables-set feed** (addresses of allowed hosts per interface pair, research 3.8) is a **later phase** (task 7), not v1. |
| D-9 | **The agent working documents stay in this repository and go public with it.** Nothing lab-specific is written into them. Agents commit locally; **push, tags and releases are the maintainer's.** |
