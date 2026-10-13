## 13. LuCI app for the OpenWrt package: rules, hosts, services and status in the web UI 🤷

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
