# Handoff — discowall

The newest five dated entries; older ones move to `handoff-archive/<date>.md`.

## 2026-10-10 (later) — P0 built: the plain reflector works in network namespaces

- Task 2 is 🕵️: workspace with `discowalld`, `discoctl` and the protocol crate; sockets behind the `MdnsSocket`
  trait; rtnetlink interface tracking; forward-everything with own-source drop and dedup; control socket with
  `status`/`ping`; systemd unit; config example; 8 unit tests and the netns integration test (details in the item).
  Gate green locally; `.github/workflows/ci.yml` written, never run (no remote).
- **Rust 1.99 is installed** via rustup (`~/.cargo/bin`, not on PATH in non-login shells: `export PATH=$HOME/.cargo/bin:$PATH`).
- **Assumed, not decided:** OQ-10 default policy `drop` (not used before P1). Nothing else was decided.
- Next: the maintainer creates the GitHub remote and pushes (CI), tries it on a router; then task 3 (P1).

## 2026-10-10 — research, Q&A, decisions D-1…D-9, phase items filed; no code

- The maintainer's ask (verbatim in the research): a tool on a Linux router that forwards mDNS between interfaces like
  avahi's reflector, with rules (which hosts to which interface, whitelist/blacklist, src/target based, a simple form
  of iptables), a CLI to list and set rules and to see which announcements arrive on which interface, scriptable;
  Rust; what do I think, effort, feasibility.
- Written: [mdns-router-research.md](../../mdns-router-research.md) (verdict: feasible, Rust fits, 20–30 person-days
  for v1, 8–11 for an MVP; section 9 is the Q&A record), this working-model scaffold (AGENTS.md, overview with
  OQ-1…OQ-11, decisions D-1…D-9, the roadmap script copied from openccu-lite-agents, 44 tests green).
- **Q&A outcome:** Rust (D-1); the name **discowall**, `discowalld`/`discoctl`, MIT, GitHub `hobbyquaker/discowall`
  (D-2); scope stays the forwarder, SSDP later, avahi coexistence required (D-3); Linux first across Debian/Ubuntu/
  Armbian/Arch/Fedora/OpenWrt, FreeBSD/pfSense later behind a socket trait (D-4); iptables-like CLI over TOML (D-5);
  legacy-query proxy in v1 (D-6); no MAC rules (D-7); nftables feed later (D-8); agent docs public here (D-9).
- **Items:** task 1 (the concept discussion) ✅; tasks 2–6 = v1 phases P0–P4 ☑️; task 10 the avahi-coexistence lab
  check ☑️ (needs the maintainer's lab, OQ-11); tasks 7 (nftables feed), 8 (SSDP), 9 (FreeBSD/pfSense) 💤.
- **Not done, the maintainer's:** rename `~/repos/mdns-router` to `~/repos/discowall` (the VS Code workspace points
  at the old name), create the GitHub remote, answer OQ-10 and OQ-11. Then task 2 can start: `rustup` on the WSL
  machine, the Cargo project, the plain reflector with netns tests.
