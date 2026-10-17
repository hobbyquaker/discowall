# Handoff — discowall

The newest five dated entries; older ones move to `handoff-archive/<date>.md`.

**Last session 2026-10-13** (the maintainer: "continue with our roadmap. you may create lxc containers on my proxmox
for testing"; later "file task: lucy gui for openwrt package. do research for pfsense ui and file another task for
that"). Everything is committed locally; **nothing pushed** (D-9: the maintainer pushes). The lab containers (900–904) and the
OpenWrt VM (905) are on the node and stay (see *Cold start*).

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
  never on a real router. 10 ✅ (coexistence measured). 11 ✅ (nine cases green on 2026-10-13). 16 (OpenWrt packages on the release) 🕵️. 12 ✅ (avahi inside the namespace
  test, green in CI). B-1, B-2, B-3 ✅ (found and fixed in
  the lab, see below). 13 (LuCI app) 🕵️: built and tested on an OpenWrt 24.10.8 VM in the lab (36/36, with the pages in a browser), see its item; 14 (pfSense package + UI), 15 (IGMP snooping without querier) 🤷 for the
  maintainer; 7, 9 💤.
- **Released as v0.0.2 on 2026-10-13:** B-1, B-2, B-3; `discoctl rule add|insert --json`; UCI-rendered OpenWrt config;
  `luci-app-discowall`; the avahi namespace test, run by CI. Since task 16 the release also carries the OpenWrt
  packages (`.ipk` 24.10, `.apk` 25.x, the D-14 architectures, unsigned), built from the release binaries.
- **Open for the maintainer:** OQ-10 (default policy), OQ-11 (which real router); task 14 questions (web-ui
  research note); task 15 (document IGMP snooping, or mitigate); try v0.0.2 on a real router (tasks 2–6, 8 → ✅); task 13's two questions
  (OpenWrt 25.12, firewall rules in the package).
- **Where to continue without the maintainer:** nothing big — the OpenWrt VM exists now (task 13); an hour-long soak (`lab.py run --soak 3600 scale`); task 12 waits for CI.
- **Known gaps worth knowing:** `records_dropped`/rule hits count per target interface, not per packet; the proxy
  opens one ephemeral socket per legacy query; `cargo test -p discowalld` does not rebuild `discoctl` (`cargo build
  --all` first); after a directory rename `touch crates/discowalld/tests/netns.rs` (the test binary embeds absolute
  paths — cost an hour on 2026-10-13); the README's Roadmap list is hand-maintained in both languages; SSDP over IPv6 is implemented but only v4-tested; the `discowalld --version` string is the
  workspace version, so dev debs (`--deb-version 0.0.2~dev2`) still say 0.0.1.

## 2026-10-17 — first real-router run of 0.0.2; B-4, B-5, tasks 17 and 18 filed

- **Real router:** the maintainer installed the 0.0.2 `.deb` on a Debian 12 x86_64 router and replaced avahi's
  reflector with it (avahi stays as the responder, `enable-reflector=no`): five interfaces (untagged LAN, three VLANs,
  a VXLAN), IPv4 + IPv6, `default = "accept"`, SSDP off, the shipped unit. Interfaces joined on both families,
  Matter and HomeKit services from other segments in `discoctl hosts`, no send errors. Details in task 18.
- **B-4 (🤷):** a TXT record with RDLENGTH 0 makes hickory-proto 0.26.3 fail the whole packet (`InvalidEmptyRecord`,
  reproduced against the captured packet); RFC 6763 §6.1 says to treat it as an empty string. The fix shape is open.
- **B-5 (☑️):** `discowalld --check` never reads the rules file (a `default = "acept"` passes).
- **Task 17 (☑️):** the reason for `unparsable` is only in the debug log; show it in `watch` and a rate-limited info line.
- **Task 18 (☑️):** drop the README's "not run on a real router yet" caution — after the router has run a few days.
- Nothing committed in code; nothing pushed.

## 2026-10-13 (Opus lane) — task 16: OpenWrt .ipk/.apk on the v0.0.2 release, release job for future tags

- **On v0.0.2** (`gh release upload`, the maintainer's word): `luci-app-discowall_0.0.2-r1_all.{ipk,apk}` and
  `discowall_0.0.2-r1_<arch>.{ipk,apk}` for the ten D-14 architectures — 22 files, built in the lab's SDK container
  with `contrib/openwrt/build-packages.sh` from the release's own tarballs (binaries byte-identical). The x86_64 ipk
  and the luci ipk passed `lab.py run --ui … openwrt` 36/36 on the 24.10.8 VM after a clean removal; the other
  architectures and all `.apk` checked by control data and ELF only (D-15).
- **New:** `contrib/openwrt/discowall-prebuilt` (the package from the static binary, no Rust), `build-packages.sh`,
  and the release workflow's `openwrt` job (24.10.8 and 25.12.5 × ten SDK targets) plus `workflow_dispatch` with a
  `tag`. **Not yet run on GitHub** — it needs the push; a dispatch with `tag: v0.0.2` afterwards would re-upload the
  same files with `--clobber` and is the first real test.
- **For the maintainer:** `arm_cortex-a9` (bcm53xx) is in D-14 but its SDK has no `HAS_FPU` and the armv7 binary is
  hard-float VFPv3 — expected not to run on BCM4708/4709 (README says so; drop it or keep it?). opkg has no
  `--force-architecture`; the README documents `--add-arch` (a plain foreign-arch install segfaulted opkg, measured).
  The OpenWrt versions in the matrix are pinned (bump per point release). Asset names use the SDK's `-r1`.

## 2026-10-13 (release) — v0.0.2 on GitHub, task 13's LuCI app built and lab-tested, task 12 green in CI

- **Release `v0.0.2`** on the maintainer's word ("release as 0.0.2"): version bumped everywhere (workspace, the two path
  dependencies, both OpenWrt Makefiles, the lab docs), CI run 38411440694 green on master including the avahi
  namespace test (task 12 → ✅), tag `v0.0.2` on commit 8fd31cc, release workflow run 38411727663 (see the release
  for its outcome; notes from the session's draft via `gh release edit`). Fixes B-1, B-2, B-3; new: `discoctl rule
  add|insert --json`, UCI-rendered OpenWrt config, `luci-app-discowall`, the avahi namespace test.
- **Task 13 (Opus lane, 🕵️):** discoctl `--json` rules (D-11), UCI → TOML in the procd init (D-10), the LuCI app
  (D-12, D-13) with a ucode rpcd plugin; tested end to end on an OpenWrt 24.10.8 VM in the lab (case `openwrt`,
  36/36, packages built by the OpenWrt SDK in CT 904, VM 905). Found: OpenWrt zones that reject input also reject the
  unicast answers to the proxies — allow UDP from source ports 1900 and 5353 (README, OpenWrt section); the VM gets
  no DHCP lease on the management network (reached over IPv6 link-local via the node). Open for the maintainer:
  OpenWrt 25.12 is the newest stable (D-12 says latest stable; the lab is on 24.10), and whether the package ships the
  firewall rules. Details in `tests/lab/openwrt.md` and the item.
- **Lab guests now 900–905** (router, three hosts, SDK container, OpenWrt VM), all running; the SDK container has
  30 GB and 4 GB RAM (10 GB during a Rust toolchain build, ~65 min).

## 2026-10-13 (Opus lane) — task 13 built: discoctl --json rules, UCI on OpenWrt, luci-app-discowall, OpenWrt VM in the lab

- **Built and committed** (`a2ccfbd`, `f3befb1`, `d490aa2`, item commits): `discoctl rule add|insert --json
  '<RuleSpec>'` (D-11); the OpenWrt package with `/etc/config/discowall` (uci-defaults) rendered to
  `discowall.toml` by the procd init script (D-10); `contrib/openwrt/luci-app-discowall` — ucode rpcd object
  `luci.discowall`, ACL, menu, views Status / Rules (modal editor, live, Save, Apply, unsaved banner) / Hosts and
  services / Settings (D-12, D-13); `lab.py` case `openwrt` (+ `--ipk`, `--ui`, `--keep-openwrt`),
  `tests/lab/luci-ui.js` (headless Firefox), `tests/lab/openwrt.md`.
- **Lab:** two new guests on the node: CT 904 `discowall-sdk` (OpenWrt SDK 24.10.8; 10 GB RAM after an OOM in the
  LLVM build) and VM 905 `discowall-openwrt` (OpenWrt 24.10.8, VLANs 110/120/130 at .2, management over IPv6
  link-local through the node — the network's DHCP never answered it). `lab.py run --ui --ipk … openwrt`: **36/36**, with
  both packages built in the SDK (the feed's Rust 1.90.0; Rust host build about 65 min + an OOM'd first try).
  CT 900's daemon is stopped during the case and started again after it.
- **Found:** OpenWrt zones with input REJECT drop the unicast answers to the proxies (allow UDP from source ports
  1900/5353 — READMEs); the lab's IGMP querier died for iot after the dynamics case (fixed).
- **For the maintainer:** OpenWrt **25.12.5** is the latest stable today, D-12 says target it (the lab ran 24.10.8
  as the Q&A named). Details and the open points in [task 13](roadmap/task-13.md).

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
