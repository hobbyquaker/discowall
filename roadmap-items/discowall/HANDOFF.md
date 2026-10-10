# Handoff — discowall

The newest five dated entries; older ones move to `handoff-archive/<date>.md`.

## Cold start (read this first)

- **What it is:** a discovery firewall in Rust — `discowalld` forwards mDNS between a router's interfaces under a
  rule chain, `discoctl` is its CLI. Read `AGENTS.md`, then [overview.md](overview.md) (what and why, OQ-n),
  [decisions.md](decisions.md) (D-1…D-9), the index [roadmap-readmes/discowall.md](../../roadmap-readmes/discowall.md)
  and the item you were given. The research with the design is [mdns-router-research.md](../../mdns-router-research.md)
  (sections 3 design, 6 phases, 9 the Q&A record).
- **Where:** GitHub `hobbyquaker/discowall` (public, branch `master`, pushed 2026-10-10 with Fable's commits up to
  P1), local working copy `~/repos/mdns-router` until the maintainer renames it to `~/repos/discowall`. Agents commit
  locally with named paths; **push only on the maintainer's word** (D-9) — the first push was his.
- **Toolchain:** Rust stable via rustup on the WSL machine; `export PATH=$HOME/.cargo/bin:$PATH` in non-login shells.
  Gate: `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo build --all`, `cargo test --all`
  (the last builds `discoctl` for the namespace test). The namespace test needs `unshare -rn`, `ip`, `nsenter`,
  `python3`; it skips with a message where it cannot run. CI (`.github/workflows/ci.yml`) runs the same.
- **Layout:** `crates/discowall-proto` (JSON-lines control protocol, `RuleSpec`, views), `crates/discowalld`
  (`config.rs`, `socket.rs` trait + Linux backend, `ifaces.rs` rtnetlink, `packet.rs` parser/classification on
  hickory-proto, `hosts.rs` host table and bundles, `rules.rs` chain/globs/prefixes, `reflector.rs` the core and the
  control handling, `control.rs` unix socket, `tests/netns.rs` + `tests/mdns_probe.py`), `crates/discoctl`,
  `contrib/` (config example, rules example, systemd unit).
- **State of the items:** 2 (P0) and 3 (P1) are 🕵️ — green locally and in CI, never on a real router. 4 (P2: `watch`, `services`, `stats`, `status` details, per-rule log), 5 (P3: legacy one-shot queries and the
  QU bit, fe80 stripping, IPv6 end to end, dedup against a second reflector, fuzzing, unit hardening), 6 (P4:
  packaging, cross builds, release workflow, docs) and 10 (lab: avahi coexistence on 5353) are ☑️; 7, 8, 9 are 💤.
- **Open for the maintainer:** OQ-10 the default policy of a fresh install (the code assumes `drop`); OQ-11 which
  router and VLANs are the lab for task 10 and for packet captures as parser fixtures (nothing lab-specific goes
  into the repository). The directory rename. A try of P0/P1 on a router: `discowalld --config`, `discoctl status`,
  a browse from one side, `discoctl hosts`.
- **Known gaps worth knowing before the next item:** the QU bit is forwarded unchanged (P3 clears it); a query from
  an ephemeral port is forwarded like any other and its unicast answer never comes back (P3 proxies it); fe80 AAAA
  records cross (P3 strips them); IPv6 is implemented but only the v4 path is covered by the namespace test;
  `records_dropped` counts per target interface, not per packet; the README's Roadmap list is hand-maintained in both
  languages and must be updated with every `roadmap.py state`.

## 2026-10-10 (evening) — pushed to GitHub

- The maintainer had the repository created and pushed: `github.com/hobbyquaker/discowall`, public, `master`, six
  commits (research and scaffold, README, P0, P1). **First CI run green** (run 38060324264: fmt, clippy, build, tests
  including the namespace test on the ubuntu runner, release build). Tasks 2 and 3 stay 🕵️ until a real router has
  seen them; after that they are ✅.
- Session ends here; everything above in *Cold start* is current.

## 2026-10-10 (later) — P0 built: the plain reflector works in network namespaces

- Task 2 is 🕵️: workspace with `discowalld`, `discoctl` and the protocol crate; sockets behind the `MdnsSocket`
  trait; rtnetlink interface tracking; forward-everything with own-source drop and dedup; control socket with
  `status`/`ping`; systemd unit; config example; 8 unit tests and the netns integration test (details in the item).
  Gate green locally; `.github/workflows/ci.yml` written, never run (no remote).
- **Rust 1.99 is installed** via rustup (`~/.cargo/bin`, not on PATH in non-login shells: `export PATH=$HOME/.cargo/bin:$PATH`).
- **Assumed, not decided:** OQ-10 default policy `drop` (not used before P1). Nothing else was decided.
- **Task 3 (P1) is 🕵️:** parser, host table with bundles, rule chain, packet rebuild, rules file with save/apply,
  `discoctl rule …`/`hosts`/`save`/`apply`, the netns test drives the chain through discoctl and proves the
  HomeKit scenario (camera dropped, bridge forwarded). 31 tests green, clippy clean. Details in the item.
- Next: the maintainer creates the GitHub remote and pushes (CI), tries P0 on a router; the P1 lane continues with
  the host table (research 3.5).

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
