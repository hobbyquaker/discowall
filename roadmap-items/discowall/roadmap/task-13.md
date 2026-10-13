## 13. LuCI app for the OpenWrt package: rules, hosts, services and status in the web UI 🕵️

**Created** 2026-10-13.

**Created** 2026-10-13 (maintainer: "file task: lucy gui for openwrt package"). Research note with the pfSense/OPNsense
counterpart: [../notes/web-ui-research.md](../notes/web-ui-research.md).

**The ask:** a LuCI application (`luci-app-discowall`) so that the rule chain, the host and service tables, the
counters and the daemon's status can be seen and edited in OpenWrt's web UI, not only with `discoctl` over SSH.

**Shape (from the LuCI example app in `openwrt/luci`, `applications/luci-app-example`, read 2026-10-13):**
- Modern LuCI apps are **client-side JavaScript** (`htdocs/luci-static/resources/view/discowall/*.js`, `'require form'`,
  `'require rpc'`, `view.extend`), a menu file `root/usr/share/luci/menu.d/luci-app-discowall.json`, an ACL file
  `root/usr/share/rpcd/acl.d/luci-app-discowall.json`, and a `Makefile` with `LUCI_TITLE`, `LUCI_DEPENDS:=+luci-base
  +discowall`, `include ../../luci.mk`. The Lua server-side way is deprecated.
- **Backend = an rpcd plugin that calls `discoctl --json`**, not UCI: the chain lives in `rules.toml` (D-5) and every
  `discoctl` answer is already JSON. A ucode script `root/usr/share/rpcd/ucode/discowall.uc` exporting the object
  `luci.discowall` with methods `status`, `rules`, `rule_add`, `rule_del`, `rule_flush`, `save`, `apply`, `hosts`,
  `services`, `stats` (each a `discoctl --json …` call via `system()`/popen; `rule_add` passes the tokens). The
  ACL grants `read → ubus → luci.discowall → [status, rules, hosts, services, stats]` and `write → ubus →
  luci.discowall → [rule_add, rule_del, rule_flush, save, apply]`. The daemon's control socket must be reachable
  by rpcd (root on OpenWrt: fine).
- **Views:** *Status* (interfaces up/joined, counters, `discoctl status`), *Rules* (the chain as a table with hit
  counters; add/insert/delete with a token-field editor — in/out/src/kind/service/instance/host/rrtype/proto/action/log;
  "Save" and "Apply" buttons; the `unsaved` flag shown), *Hosts* and *Services* (read-only tables, auto-refresh),
  later *Watch* (a streaming view is not natural in LuCI's request model: poll `stats` instead, or skip).
- **Config:** `discowall.toml` (interfaces, ipv4/ipv6, ssdp) could be a UCI section rendered to TOML by the init
  script — only if the OpenWrt package itself moves to UCI (undecided; today the package ships the TOML files as
  conffiles). The LuCI app does not need it for the rule chain.

**Open (🤷):** UCI for `discowall.toml` or keep TOML; whether `discoctl` grows a `rule add --json <RuleSpec>` so the
UI does not build token lists; whether to place the app in the `openwrt/luci` feed (needs the discowall package in
`packages` first — task 6's OpenWrt package is still unbuilt) or ship it from this repository (`contrib/openwrt/
luci-app-discowall`). Needs an OpenWrt build host or VM (research section 2 of the e2e note: an OpenWrt VM in the
Proxmox lab) — the natural next step of task 11 once the package builds.

**Q&A 2026-10-13 (maintainer, answers recorded as D-10…D-13):**
1. Settings store on OpenWrt → **UCI rendered to TOML by the init script**; the rule chain stays in `rules.toml` (D-10).
2. `discoctl rule add --json <RuleSpec>` → **yes** (D-11).
3. Location → **`contrib/openwrt/luci-app-discowall` here**, upstream to the luci feed later (D-12).
4. Test bed → **yes: an SDK container (id 904) and an OpenWrt x86-64 VM (id 905) in the Proxmox lab**, same rules as
   for 900–903 (only these ids; nothing else on the node).
5. Views → **Status, Rules editor, Hosts and services**; live watch not in the first version (polling `stats` later).
6. OpenWrt release → **24.10 stable for now, "but we should always target the latest stable"** (D-12).
7. Rule editor → **table + modal form, edited live**, Save/Apply buttons, unsaved flag (D-13).
8. Access → **admin only**, one ACL group with read and write methods (D-13).

**Shape, ready to build (☑️):**
- **discoctl:** `rule add|insert --json '<RuleSpec>'` (serde from the proto crate; tokens stay the default; man page).
- **OpenWrt package** (`contrib/openwrt/discowall`): `/etc/config/discowall` (section `discowall 'main'`: `list interface`,
  `ipv4`, `ipv6`, `ssdp`, `rules_file`), `uci-defaults` creating it on first install, the procd init script renders
  `/etc/discowall/discowall.toml` from UCI and starts `discowalld`; `rules.toml` stays a conffile.
- **`contrib/openwrt/luci-app-discowall`:** `Makefile` (`LUCI_TITLE`, `LUCI_DEPENDS:=+luci-base +discowall`),
  `root/usr/share/rpcd/ucode/discowall.uc` (object `luci.discowall`: `status`, `rules`, `hosts`, `services`, `stats`
  read; `rule_add {rule, at}`, `rule_del {index}`, `rule_flush`, `save`, `apply` write — each a `discoctl --json` call,
  the result parsed and returned), ACL `root/usr/share/rpcd/acl.d/luci-app-discowall.json` (read/write split as
  above plus `uci: discowall`), menu `admin/services/discowall/{status,rules,hosts,settings}`, views in
  `htdocs/luci-static/resources/view/discowall/`: `status.js` (poll 5 s), `rules.js` (table from `rules`, modal form with
  the RuleSpec fields — interface dropdowns from `status`, kind/proto/action selects, free text for src/service/instance/
  host/rrtype/comment, `log` checkbox — live `rule_add`/`rule_del`, Save and Apply buttons, unsaved banner),
  `hosts.js` (hosts + services tables, poll), `settings.js` (`form.Map('discowall')`: interfaces multi-select from
  the device's interfaces, ipv4/ipv6/ssdp flags; saving restarts the service).
- **Lab:** CT 904 `discowall-sdk` (Debian 13, OpenWrt 24.10 x86-64 SDK, Rust in the SDK, `make package/discowall/compile`
  and `luci-app-discowall`), VM 905 `discowall-openwrt` (OpenWrt 24.10 x86-64 generic image, management NIC + the three
  lab VLANs, the packages installed with `opkg`/`apk`), `lab.py` gains an `openwrt` target or cases (install, status
  over LuCI's JSON-RPC, a rule added through the UI's rpcd method, browse from the lan host).
- **Effort:** discoctl half a day, UCI/init one day, app two days, SDK/VM/lab one to two days.

📥 2026-10-13: enqueued in an Opus lane by the coordinator (maintainer: "start a opus lane for task 13 and other ready-to-implement tasks"; task 13 is the only ☑️ item).

🏗️ 2026-10-13: in progress (Opus lane).

### Built (Opus lane, 2026-10-13)

- **discoctl** (`a2ccfbd`): with `--json`, `rule add` / `rule insert N` take one JSON object as the RuleSpec
  (`discoctl rule add --json '{"in":"iot","out":"lan","host":["bridge"],"action":"accept"}'`); the words stay the
  default; `--json` was already the global output flag, so one flag means "JSON in and out". Unit test
  `takes_a_rule_as_json` (insert at 2, round trip through the listed RuleSpec, words still work, unknown fields and
  a missing action refused, an object without `--json` refused). Help text and both READMEs.
- **OpenWrt package** (`f3befb1`): `files/discowall.uci-defaults` creates `/etc/config/discowall` (section
  `discowall 'main'`: `ipv4`, `ipv6`, `ssdp`, `rules_file`; `list interface` left to the user) unless it exists;
  `files/discowall.init` renders `/etc/discowall/discowall.toml` from UCI before every start (interfaces checked
  against `[A-Za-z0-9._@-]`, fewer than two → not started, logged), rewrites the file only when it changed (flash),
  `procd_set_param file` so that a reload (`reload_config`, LuCI's Save & Apply) restarts the daemon only when
  the rendered file changed — a reload that changes nothing keeps the live, unsaved chain. Conffiles:
  `/etc/config/discowall`, `/etc/discowall/rules.toml`. The Makefile now calls `Build/Compile/Cargo` once per
  crate: `cargo install --path` on the workspace root (what the old default did) cannot install a virtual
  manifest. Interfaces are Linux device names (`br-lan`, `eth2`), as the rules name them.
- **luci-app-discowall** (`f3befb1`): Makefile (`LUCI_TITLE`, `LUCI_DEPENDS:=+luci-base +discowall`,
  `LUCI_PKGARCH:=all`, `include $(TOPDIR)/feeds/luci/luci.mk` — inside the luci feed it would be `../../luci.mk`);
  `root/usr/share/rpcd/ucode/discowall.uc` (object `luci.discowall`; each method `discoctl --json …` through
  `popen`, the answer parsed; plain-text failures (daemon unreachable, exit 3) come back as
  `{"result":"error","message":…,"exit":3}`; `rule_add {rule, at}` passes the object shell-quoted to
  `rule add|insert --json`); ACL `luci-app-discowall` (read: status, rules, hosts, services, stats + uci
  discowall; write: rule_add, rule_del, rule_flush, save, apply + uci discowall); menu
  `admin/services/discowall/{status,rules,hosts,settings}` (depends on the ACL and on the UCI config); views
  `status.js` (interfaces, counters, poll 5 s), `rules.js` (table with hit counters, modal form with every RuleSpec
  field — interface dropdowns from `status`, proto/kind checkboxes, action select, log checkbox, comma lists,
  position — "Insert above" and "Delete" per row, Save, Apply, the unsaved banner), `hosts.js` (hosts and
  services tables, poll 5 s), `settings.js` (`form.Map('discowall')`, `widgets.DeviceSelect` multiple for the
  interfaces, ipv4/ipv6/ssdp flags, rules file). A failed poll shows in the banner instead of LuCI's error path.
- **Lab** (`f3befb1`, `d490aa2`): case `openwrt` in `lab.py` (`--ipk` installs packages on the VM, `--ui` drives
  the pages in headless Firefox through `tests/lab/luci-ui.js`, `--keep-openwrt`), `[openwrt]` in
  `lab.toml.example`, [tests/lab/openwrt.md](../../../tests/lab/openwrt.md) with how the SDK container and the VM
  were built. `igmp_querier.py` reopens its socket per round (see *Found* below).

### Lab run (measured 2026-10-13)

Guests: CT 904 `discowall-sdk` (Debian 13, OpenWrt SDK 24.10.8, 30 GB, 8 cores; memory raised from 4 to 10 GB
after the first Rust build was OOM-killed, set back to 4 GB once the packages were built) and VM 905 `discowall-openwrt` (OpenWrt 24.10.8 x86-64
generic-ext4-combined, 512 MB, net0 management + VLANs 110/120/130 as `br-lan`(eth1)/`eth2`/`eth3` at .2, zones
iot and guest with input REJECT and allow rules for 5353/1900). CT 900's discowalld is stopped while the case runs
and started again afterwards.

- `lab.py run --ui --ipk discowall_0.0.1-r1_x86_64.ipk --ipk luci-app-discowall_0.0.1-r1_all.ipk openwrt` →
  **PASS openwrt 36/36 checks** (68.8 s): opkg install; uci-defaults created the config; the init script rendered
  `interfaces = ["br-lan", "eth2", "eth3"]`; the four views served; `luci.discowall status` over uhttpd's
  JSON-RPC (`session login`, then `call`) with three interfaces up and joined v4+v6; empty chain → the lan host sees
  no printer (control: the VM is the only forwarder); four `rule_add` with RuleSpec objects → in `discoctl rule
  list`; insert at 1 + `rule_del 1`; a bad action → `{"result":"error"}` with exit 2; avahi on the lan host browses
  and resolves "Lab Printer" on the iot host over IPv4 and IPv6 through the VM; gssdp-discover on lan finds
  minidlna on iot (search proxied); `hosts` knows the iot host on `eth2`; rule hits counted; `save` writes
  `rules.toml` and clears `unsaved`; flush + `apply` brings the four rules back; settings written as LuCI does
  (`uci set`, `uci apply` without rollback) → `ssdp = false` rendered, new daemon PID; the saved chain is back; and
  in Firefox: login, menu Services → discowall with the four pages, Status rows, a rule added through the modal at
  position 1 (`proto mdns in eth2 out br-lan kind query service _http._tcp,_ssh._tcp drop (log)`), unsaved banner,
  delete, Save clears the banner, Hosts and services list the iot host and the printer, Settings renders and Save &
  Apply with SSDP off restarts the daemon (uptime 3 s), no JavaScript error on the app's pages.
- **With the packages as the SDK builds them** (the feed's Rust 1.90.0, `make package/discowall/compile` 1 min
  46 s after a 65-minute Rust host build, `luci-app-discowall` from the same tree), installed fresh with opkg on the
  VM: `lab.py run --ui --ipk … openwrt` → **PASS openwrt 36/36 checks** (58.0 s).
- The first 36/36 run's `discowall` ipk was built in a second SDK tree from the static musl binaries of this WSL machine
  (a scratch Makefile with `Build/Compile` empty, not committed) because the feed's Rust was still compiling;
  `luci-app-discowall` came from the SDK as is. The earlier runs with the files copied by hand: 20/20, 21/21,
  35/35 (the same checks as they were added).

### Found

- **OpenWrt firewall:** a zone with input REJECT drops the unicast answers to the proxies' ephemeral sockets.
  Measured: without a rule for UDP from source port 1900 every minidlna answer to a proxied M-SEARCH hit the iot
  zone's reject counter (3 per search) and gssdp-discover found nothing; with `src_port 1900` accepted it found
  the device. The same holds for one-shot mDNS queries (answers from 5353). Documented in both READMEs (OpenWrt
  section) and in `tests/lab/openwrt.md`; the package installs no firewall rules (the zones are the user's).
- **The lab's IGMP querier** died silently for the iot network after the `dynamics` case re-created CT 900's iot
  interface (ENODEV every round since); the bridge forgot the iot memberships of 239.255.255.250 and SSDP into iot
  stopped. Fixed in `igmp_querier.py` (fresh socket per round), pushed to CT 900 and restarted.
- **Headless Chromium under WSL** never painted a frame (LuCI stayed at "Loading view…"); Firefox from
  playwright's cache works.
- The VM got no DHCP lease on the management network (eight DISCOVERs on the bridge, no OFFER); it is reached over
  IPv6 link-local through the node. Not a discowall matter.
- The daemon lowercases instance names in its tables (`lab printer._ipp._tcp.local`) — existing behaviour, seen
  in the Hosts page; noted, not changed.

### Open

- **Version (D-12):** downloads.openwrt.org lists **25.12.5** as the newest stable release (2026-10-13); the Q&A
  answer named 24.10 and the lab was built with 24.10.8. Per D-12 the target is 25.12 now (apk instead of opkg;
  the case already uses `apk add --allow-untrusted` when present). For the maintainer: move the lab to 25.12.
- ACL: tested only with the root login (rpcd gives root every ACL group); a non-root rpcd user with only
  `luci-app-discowall` was not tried.
- `PKG_HASH:=skip` until a release tarball exists; the luci package's Maintainer field is luci.mk's default.
- Translations (`po/`) not written.

🕵️ 2026-10-13: built; the VM shows the UI working end to end (36/36). Pending: OpenWrt 25.12 (see *Open*); a real router.
Later the same day: the packages built with the feed's Rust in the SDK pass the same 36 checks.

**Q&A 2026-10-13 (closing the two open points):** OpenWrt 25.12 → packages for 24.10 and 25.x, the lab VM stays on 24.10 for now (D-15); firewall rules → not installed by the package, documented (D-16).
