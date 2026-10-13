## 13. LuCI app for the OpenWrt package: rules, hosts, services and status in the web UI ☑️

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
