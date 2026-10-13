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

Decisions from the Q&A on task 13 (LuCI app) of 2026-10-13; the questions and the maintainer's answers are in the item.

| | Decision (2026-10-13) |
| --- | --- |
| D-10 | **On OpenWrt the daemon's settings live in UCI** (`/etc/config/discowall`: interfaces, ipv4, ipv6, ssdp); the init script renders `/etc/discowall/discowall.toml` from it before start. **The rule chain stays in `rules.toml`** under `discoctl`/the daemon, on every platform. LuCI edits the settings through UCI and the rules through the daemon. |
| D-11 | **`discoctl rule add` and `rule insert` accept `--json <RuleSpec>`** besides the token syntax, so that UIs pass rule objects through instead of building tokens. No protocol change (the protocol already carries `RuleSpec`). |
| D-12 | **`luci-app-discowall` lives in this repository** under `contrib/openwrt/`, beside the package, built with the OpenWrt SDK; upstreaming to the `openwrt/luci` feed comes after the `discowall` package is in `openwrt/packages`. **Target: the latest stable OpenWrt** — 24.10 today; the target moves with each release. |
| D-13 | **The LuCI app's rule editor edits live** (every change goes to the daemon at once, as `discoctl` does; Save writes `rules.toml`, Apply reloads it, the unsaved flag is shown) — the iptables-like model of D-5 in the UI. First version: Status, Rules, Hosts and Services views; one ACL group `luci-app-discowall` (admin: read and write). |

Decisions from the Q&A on tasks 13/16 (OpenWrt packaging) of 2026-10-13.

| | Decision (2026-10-13) |
| --- | --- |
| D-14 | **OpenWrt packages on every release, unsigned, for local install:** `luci-app-discowall` (`all`) and `discowall` for the architectures the three static musl binaries cover — x86_64; aarch64_cortex-a53, aarch64_cortex-a72, aarch64_generic; arm_cortex-a7_neon-vfpv4, arm_cortex-a9_vfpv3-d16, arm_cortex-a15_neon-vfpv4, arm_cortex-a8_vfpv3, arm_cortex-a9, arm_cortex-a9_neon — one binary per family, `--force-architecture` for the rest, no MIPS (Rust has no tier-2 musl target). Users `opkg install ./file.ipk` (apk: `--allow-untrusted`); a signed package feed would be its own task. |
| D-15 | **Both package formats:** `.ipk` for OpenWrt 24.10 (opkg) and `.apk` for 25.x, built from the two SDKs in the release workflow. **The lab's OpenWrt VM stays on 24.10 for now**; a 25.12 VM comes with a later lab session (D-12's "latest stable" is the target the packages follow). |
| D-16 | **The OpenWrt package installs no firewall rules.** Zones are the administrator's; the README and the LuCI settings page say that a zone with input REJECT must accept UDP to and from ports 5353 and 1900 for the proxies' unicast answers. |
