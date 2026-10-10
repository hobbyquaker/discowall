# Handoff — discowall

The newest five dated entries; older ones move to `handoff-archive/<date>.md`.

**Paused 2026-10-10 after the release of v0.0.1** (maintainer: "wir machen hier erstmal pause"). Everything is
committed and pushed; the working tree is clean; the next session starts from *Cold start* below.

## Cold start (read this first)

- **What it is:** a discovery firewall in Rust — `discowalld` forwards mDNS between a router's interfaces under a
  rule chain, `discoctl` is its CLI. Read `AGENTS.md`, then [overview.md](overview.md) (what and why, OQ-n),
  [decisions.md](decisions.md) (D-1…D-9), the index [roadmap-readmes/discowall.md](../../roadmap-readmes/discowall.md)
  and the item you were given. The research with the design is [mdns-router-research.md](../../mdns-router-research.md)
  (sections 3 design, 6 phases, 9 the Q&A record).
- **Where:** GitHub `hobbyquaker/discowall` (public, branch `master`, release `v0.0.1` of 2026-10-10), local
  working copy `~/repos/discowall` (renamed from `mdns-router` by the maintainer when the session of 2026-10-10
  ended; if a memory or a note still says `mdns-router`, that is why). Agents commit locally with named paths;
  **push, tags and releases only on the maintainer's word** (D-9).
- **Toolchain:** Rust stable via rustup on the WSL machine; `export PATH=$HOME/.cargo/bin:$PATH` in non-login shells.
  Gate: `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo build --all`, `cargo test --all`
  (the last builds `discoctl` for the namespace test). The namespace test needs `unshare -rn`, `ip`, `nsenter`,
  `python3`; it skips with a message where it cannot run. CI (`.github/workflows/ci.yml`) runs the same.
- **Layout:** `crates/discowall-proto` (JSON-lines control protocol, `RuleSpec`, views), `crates/discowalld`
  (`config.rs`, `socket.rs` trait + Linux backend, `ifaces.rs` rtnetlink, `packet.rs` parser/classification on
  hickory-proto, `hosts.rs` host table and bundles, `rules.rs` chain/globs/prefixes, `reflector.rs` the core and the
  control handling, `control.rs` unix socket, `tests/netns.rs` + `tests/mdns_probe.py`), `crates/discoctl`,
  `contrib/` (config example, rules example, systemd unit).
- **State of the items:** 2–6 (P0–P4, the whole v1) and 8 (SSDP) are 🕵️ — green locally and in CI, released as
  v0.0.1, never on a real router; the release workflow is proven. 10 (lab: avahi coexistence) is ☑️ and needs
  the lab, 11 (Proxmox lab) 🤷, 12 (avahi in the namespace test) ☑️; 7, 9 are 💤.
- **Open for the maintainer:** OQ-10 the default policy of a fresh install (the code assumes `drop`); OQ-11 which
  router and VLANs are the lab; task 11's questions (fresh or real VLAN tags for the Proxmox lab, SSH for
  sessions to `pct` and the containers, an OpenWrt VM later). A try of v0.0.1 on a router: install the `.deb`,
  `discoctl status`, rules, a browse from one side, `discoctl watch` — then tasks 2–6 and 8 → ✅.
- **Where to continue without the maintainer:** task 12 (avahi inside the namespace test, ☑️); then nothing
  until the lab exists — tasks 7 (nftables sets) and 9 (FreeBSD) are 💤 by decision, not by readiness.
- **Known gaps worth knowing before the next item:** `records_dropped` and `dropped_link_local` count per target
  interface, not per packet; the proxy opens one ephemeral socket per legacy query (fine for `dig`, a flood would
  need a cap); `cargo test -p discowalld` does not rebuild `discoctl` for the namespace test (`cargo build --all`
  first); the README's Roadmap list is hand-maintained in both languages and must be updated with every
  `roadmap.py state`; the OpenWrt package is untested; SSDP over IPv6 (ff02::c) is implemented but only v4-tested.

## 2026-10-10 (release) — v0.0.1 on GitHub, end-to-end testing proposed

- **Release `v0.0.1`** tagged and published on the maintainer's word; the release workflow succeeded on its first
  run (three musl targets, tarballs, deb, rpm, nine assets). CI green on `master`.
- **End-to-end testing:** [notes/e2e-testing-research.md](notes/e2e-testing-research.md) proposes a Proxmox lab
  of three LXC containers on a VLAN-aware bridge driven by `tests/lab/lab.py` over SSH (task 11, 🤷, needs the
  maintainer's answers on VLAN tags and SSH access), avahi inside the namespace test on GitHub (task 12, ☑️), and
  a manual per-release checklist on the real router.
- **Next:** the maintainer answers task 11's questions and OQ-10/OQ-11; tries v0.0.1 on a router (tasks 2–6 and
  8 → ✅); a session can take task 12 without the lab.

## 2026-10-10 (late night) — SSDP in, version set to 0.0.1 for the first release

- **Task 8 (SSDP)** 🕵️ on the maintainer's word: second socket pair on 1900, parser, notifications under the rules,
  searches proxied with unicast relay, `proto` match, UPnP recipe in the READMEs, namespace test end to end. 44
  tests. The maintainer also asked for a release as **0.0.1** once CI is green (the workspace version is 0.0.1
  now; the tag `v0.0.1` runs the release workflow for the first time) and for research on end-to-end testing in
  his Proxmox environment.

## 2026-10-10 (night) — P2, P3, P4 built; v1 is code-complete, untested on a router

- **Task 4 (P2)** 🕵️: `discoctl watch` (broadcast of per-packet decisions, streamed as JSON lines), `services`,
  `stats`. **Task 5 (P3)** 🕵️: the legacy-query proxy (ephemeral socket, answers relayed by unicast from 5353),
  QU bit cleared, link-local address records stripped, IPv6 end to end in the namespace test, a 20 000-packet
  robustness test of the parser. **Task 6 (P4)** 🕵️: man pages from clap, `.deb`/`.rpm` metadata (built locally
  from the x86_64 musl binaries), the release workflow (three musl targets with `cross`, tarballs, deb, rpm on a
  `v*` tag — never run), the OpenWrt package (never built), the READMEs with install, configuration, rules,
  recipes, discoctl and how-it-works sections. 36 tests, clippy clean, CI green on every push.
- **The README is English** (`README.md`), German is `README.de.md` (maintainer, 2026-10-10).
- **The maintainer's next moves:** try it on a router (tasks 2–6 → ✅), decide OQ-10/OQ-11, tag `v0.1.0` to run
  the release workflow for the first time (expect a fix or two in `cross`/`cargo-generate-rpm` paths), the
  avahi-coexistence lab check (task 10), the directory rename.
- **Left for later sessions:** tasks 7 (nftables sets), 8 (SSDP), 9 (FreeBSD/pfSense) are 💤; the dedup against
  a second reflector and `systemd-analyze security` need a box with avahi and systemd.

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

