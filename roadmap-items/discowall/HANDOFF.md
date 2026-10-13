# Handoff — discowall

The newest five dated entries; older ones move to `handoff-archive/<date>.md`.

**Last session 2026-10-13** (the maintainer: "continue with our roadmap. you may create lxc containers on my proxmox
for testing"; later "file task: lucy gui for openwrt package. do research for pfsense ui and file another task for
that"). Everything is committed locally; **nothing pushed** (D-9: the maintainer pushes). The lab containers are
running on the node and stay (see *Cold start*).

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
- **The lab exists** (task 11): four unprivileged Debian 13 LXC containers on the maintainer's Proxmox node —
  router (discowalld between `lan`/`iot`/`guest`, fresh VLAN tags 110/120/130), a host with avahi per network, minidlna
  on the iot host as a real UPnP device, an IGMP querier on the router. Built by `tests/lab/lab-create.sh` on the node,
  driven by `tests/lab/lab.py` over SSH + `pct exec` (nine cases, `lab.py list`), config outside the repo in
  `~/.config/discowall-lab/lab.toml` (the node's name, ids 900–903 — never written into the repo, D-9), packages in
  `~/.config/discowall-lab/assets/` (the v0.0.1 deb, locally built `0.0.2~dev*` debs). `tests/lab/README.md` is the
  manual. The lan host container also runs the namespace tests with real avahi (push the glibc test binary to the same
  absolute paths; this WSL has no avahi and no passwordless sudo).
- **State of the items:** 2–6 (P0–P4) and 8 (SSDP) are 🕵️ — v0.0.1 released, and now exercised end to end in the lab
  by the 0.0.2-dev build (install from the deb, avahi both ways, coexistence, legacy/QU, SSDP with minidlna and
  gssdp-discover, link/reboot/hot-plug dynamics, 306 rules + 50 services soak, IPv6 only, upgrade 0.0.1 → dev);
  never on a real router. 10 ✅ (coexistence measured). 11 ✅ (nine cases green on 2026-10-13). 12 🕵️ (avahi inside
  the namespace test; passes in the lab container, CI run pending the push). B-1, B-2, B-3 ✅ (found and fixed in
  the lab, see below). 13 (LuCI app), 14 (pfSense package + UI), 15 (IGMP snooping without querier) 🤷 for the
  maintainer; 7, 9 💤.
- **Unreleased changes since v0.0.1** (all on `master`, local): B-1 no ANSI codes off a terminal; B-2 dedup keyed per
  address family (avahi's dual-family queries); B-3 `discoctl save` writes the rules file itself (`RuleDump` +
  `RuleSaved` in the protocol; the daemon's `/etc` stays read-only under the hardened unit); the avahi namespace test;
  `ci.yml` installs avahi. The workspace version is still 0.0.1 — a release needs the maintainer's word and a bump.
- **Open for the maintainer:** OQ-10 (default policy), OQ-11 (which real router); task 13/14 questions (web-ui
  research note); task 15 (document IGMP snooping, or mitigate); push `master` so CI runs the avahi test; a release
  with the three fixes (0.0.2) once that is green; try it on a real router (tasks 2–6, 8 → ✅).
- **Where to continue without the maintainer:** nothing big — the lab's remaining ideas are an OpenWrt VM (needs the
  OpenWrt package built first) and an hour-long soak (`lab.py run --soak 3600 scale`); task 12 waits for CI.
- **Known gaps worth knowing:** `records_dropped`/rule hits count per target interface, not per packet; the proxy
  opens one ephemeral socket per legacy query; `cargo test -p discowalld` does not rebuild `discoctl` (`cargo build
  --all` first); after a directory rename `touch crates/discowalld/tests/netns.rs` (the test binary embeds absolute
  paths — cost an hour on 2026-10-13); the README's Roadmap list is hand-maintained in both languages; the OpenWrt
  package is untested; SSDP over IPv6 is implemented but only v4-tested; the `discowalld --version` string is the
  workspace version, so dev debs (`--deb-version 0.0.2~dev2`) still say 0.0.1.

## 2026-10-13 — the Proxmox lab built and run, four bugs found and fixed, task 10 measured, tasks 12–15

- **Lab** (task 11): created the four containers with `lab-create.sh` (one fix on the way: no `ip6=auto` on the
  management interface, ifupdown2 waits forever for DHCPv6), provisioned avahi/gupnp-tools/miniupnpc/minidlna/
  python-zeroconf, wrote `lab.py` with the nine cases of the research and a lab README. Decisions taken by the session
  (changeable): fresh VLAN tags, `pct exec` through the node instead of SSH into containers, a fourth container for
  the guest network.
- **Found in the lab, fixed, verified there:** B-1 ANSI codes in the journal; B-2 the dedup dropped avahi's IPv4
  query because its IPv6 twin had the same bytes (the namespace test sends the families 500 ms apart and never saw it);
  B-3 `discoctl save` impossible under the shipped unit (`ProtectSystem=strict`) — discoctl writes the file now.
  Fourth finding, not a bug of ours: **task 15** — the node's bridge snoops IGMP without a querier, SSDP memberships
  expired after 260 s and the `ssdp` case failed whenever it ran a few minutes after a daemon start, while mDNS
  (224.0.0.0/24, flooded by Linux bridges) never noticed. The lab now runs an IGMP querier on the router.
- **Task 10 ✅:** coexistence measured (multicast reaches both daemons; unicast to 5353 reaches the socket bound last;
  avahi's reflector beside discowall causes no storm), research 3.1 marked [measured].
- **Task 12 🕵️:** `avahi_between_namespaces` — avahi with its own D-Bus in each leaf and beside the daemon in the
  router namespace; needed a bind-mounted passwd/group with `avahi` as uid 0 (avahi insists its runtime dir belongs to
  its user). Passes in the lab container; CI installs avahi; waits for the maintainer's push.
- **Tasks 13, 14, 15 filed** with [notes/web-ui-research.md](notes/web-ui-research.md): LuCI app (client-side JS +
  a ucode rpcd plugin over `discoctl --json`), pfSense package + UI (two FreeBSD ports, packagegui descriptor, prior
  art `pfSense-pkg-mDNS-Bridge`; OPNsense plugin as the alternative; blocked by task 9), IGMP snooping.

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
