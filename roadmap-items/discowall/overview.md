# discowall overview

A **discovery firewall**: forwards mDNS (later also SSDP) between a router's interfaces under **rules** — which hosts
and services on which interface may be seen from which other interface, whitelist or blacklist, source- and
direction-aware — plus a CLI that shows what arrives where and is usable from scripts. The reflector of avahi-daemon
with control, beside avahi, never instead of it. The survey, the design, the Rust assessment, the effort estimate and
the Q&A record are in [../../mdns-router-research.md](../../mdns-router-research.md) (Fable, 2026-10-10). Decisions
D-1…D-9 in [decisions.md](decisions.md).

- **Code:** [github.com/hobbyquaker/discowall](https://github.com/hobbyquaker/discowall) (public, `master`, MIT, pushed
  2026-10-10); local `~/repos/mdns-router` until the maintainer renames the directory to `discowall`. P0 and P1 are
  built (tasks 2, 3); Rust stable via rustup on the WSL machine.
- **Runtime:** Rust (D-1). **Shape:** `discowalld` owns the 5353 sockets, parses every packet, applies the rule chain
  per (in-interface, out-interface) and re-sends; `discoctl` talks to it over a unix socket (rules, live watch, host
  table, statistics, every command with `--json`).
- **Milestones** (the items): P0 plain reflector (task 2) with the avahi-coexistence lab check (task 10); P1 parser,
  host table and bundles, rule chain, control socket, `discoctl rule …` (task 3); P2 observability (task 4); P3
  legacy-query proxy, QU, IPv6, dedup, fuzzing, hardening (task 5); P4 packaging and CI (task 6); later: nftables-set
  feed (task 7), SSDP (task 8), FreeBSD/pfSense (task 9).
- **v1 = tasks 2–6**: about 20–30 person-days (research section 6), MVP = tasks 2 and 3 plus `watch`.

## Open questions

| | Question |
| --- | --- |
| OQ-1 | ~~Agent documents in the repository?~~ Yes, public (D-9). |
| OQ-2 | ~~Name?~~ discowall / discowalld / discoctl (D-2). |
| OQ-3 | ~~Licence and remote?~~ MIT, GitHub hobbyquaker/discowall, the maintainer pushes (D-2, D-9). |
| OQ-4 | ~~First platform?~~ Linux broadly, FreeBSD later (D-4). |
| OQ-5 | ~~Beside or instead of avahi?~~ Beside, required (D-3). |
| OQ-6 | ~~Rule syntax?~~ iptables-like CLI over TOML (D-5). |
| OQ-7 | ~~Legacy queries in v1?~~ Yes (D-6). |
| OQ-8 | ~~MAC rules?~~ No (D-7). |
| OQ-9 | ~~nftables set feed?~~ Later phase (D-8). |
| OQ-10 | Default policy of a fresh install: `drop` (nothing crosses until a rule says so) or `accept` (a drop-in avahi reflector replacement that one then restricts)? The research assumes `drop`; the maintainer decides when task 3 starts. |
| OQ-11 | Which router and VLANs are the lab for task 10 (avahi coexistence) and for the packet captures that become parser fixtures? Nothing lab-specific is written into the repository (D-9). |
