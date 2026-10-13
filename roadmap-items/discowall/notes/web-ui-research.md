# Web UIs for discowall: LuCI (OpenWrt) and pfSense/OPNsense — research

**Written** 2026-10-13 by Fable for the maintainer, who asked: *"file task: lucy gui for openwrt package. do
research for pfsense ui and file another task for that."* Tasks: 13 (LuCI), 14 (pfSense). Everything below is
**read from the sources named**, on 2026-10-13, not measured on a device; nothing has been built yet.

## 0. In short

- **LuCI (task 13) is cheap and fits well.** A modern LuCI app is client-side JavaScript plus an rpcd backend; our
  backend is a thin ucode script that runs `discoctl --json …`, because every `discoctl` answer is JSON already and
  the rule chain lives in `rules.toml`, not UCI. Three views (status, rules, hosts/services) are a few hundred lines.
  Blocked in practice until the OpenWrt package (task 6, `contrib/openwrt/`) has been built once; an OpenWrt VM in the
  Proxmox lab is the test bed.
- **pfSense (task 14) is a different world and comes after the FreeBSD backend (task 9).** A pfSense package is a
  FreeBSD port `net/pfSense-pkg-discowall` that depends on a port `net/discowall` (the daemon, built by pfSense's
  builders — a Rust/cargo port, which is the first open question), plus PHP: an XML descriptor registers menu, service
  and hooks, a `.inc` renders the firewall's `config.xml` settings into our TOML and starts the rc.d service, a `.php`
  page draws the form with pfSense's `Form_*` classes. **Prior art exists in-tree:** `pfSense-pkg-Avahi` (reflector
  settings) and, closer, `pfSense-pkg-mDNS-Bridge` for Denny Page's **mdns-bridge**, a filtering mDNS forwarder with
  per-interface inbound/outbound allow/deny lists — the nearest thing to discowall on pfSense today, and a design to
  learn from and to position against.
- **OPNsense is a third target, structurally the easiest** (MVC plugin: an XML model, an XML form, a Jinja template that
  writes our config, a `configd` action file; `os-mdns-repeater` is the 15-file example). Same FreeBSD prerequisite.
  Filed as part of task 14's scope decision, not as its own task yet.
- **What the UIs need from discowall itself:** nothing new for LuCI; for pfSense/OPNsense the daemon must build on
  FreeBSD (task 9) and the UIs want a *declarative* config path — the rule chain as a file the UI writes and
  `discoctl apply` (or SIGHUP) reloads — which exists. A `discoctl rule add --json <RuleSpec>` would spare every UI
  the token-list syntax; worth adding when the first UI is written.

## 1. LuCI (OpenWrt)

Source: `openwrt/luci`, `applications/luci-app-example` (README.md, structure.md, Makefile, the ACL and menu JSON,
`root/usr/share/rpcd/ucode/example.uc`, `htdocs/luci-static/resources/view/example/rpc.js`), read 2026-10-13 from
the master branch; the Freifunk GSoC 2023 guide on migrating LuCI apps to JavaScript.

- **Shape of an app** `applications/luci-app-discowall/`:
  - `Makefile`: `LUCI_TITLE`, `LUCI_DEPENDS:=+luci-base +discowall`, `LUCI_PKGARCH:=all`, `include ../../luci.mk`.
  - `htdocs/luci-static/resources/view/discowall/*.js`: the views, client-side (`'require view'`, `'require form'`,
    `'require rpc'`, `view.extend({ load(), render() })`, DOM via `E()`). The Lua server-side way is deprecated.
  - `root/usr/share/luci/menu.d/luci-app-discowall.json`: menu entries `admin/services/discowall/…` → views, with
    `"depends": {"acl": ["luci-app-discowall"]}`.
  - `root/usr/share/rpcd/acl.d/luci-app-discowall.json`: what the UI may call — `read → ubus → luci.discowall →
    [methods]`, `write → ubus → luci.discowall → [methods]`. A wrong ACL shows "Access denied"; rpcd must be restarted
    after changing it.
  - `root/usr/share/rpcd/ucode/discowall.uc`: the backend, a ucode script that `return { 'luci.discowall': methods }`;
    each method is `{ call: function(req) {…} }` and must return JSON. The example uses UCI; ours shells out to
    `discoctl --json` and returns the parsed answer (`rule_add` takes the tokens as `args`). Alternative: a shell
    script in `/usr/libexec/rpcd/luci.discowall` (older style, also supported).
  - `po/templates/discowall.pot` for translations.
- **Views for discowall:** *Status* (interfaces, counters — `status`), *Rules* (table with hit counters from
  `rule list`; add/insert/delete; Save and Apply; the `unsaved` flag), *Hosts* and *Services* (read-only, poll every few
  seconds). *Watch* streams over the control socket; LuCI's request model is call/response, so either poll `stats`
  or leave `watch` to the terminal.
- **Config file vs UCI:** the OpenWrt package ships `discowall.toml` and `rules.toml` as conffiles. OpenWrt custom
  would be a UCI config `/etc/config/discowall` (interfaces, ipv4, ipv6, ssdp) rendered to TOML by the init script
  before start; the rule chain stays in `rules.toml` under `discoctl`'s control either way. Decide when the package
  is first built.
- **Effort:** backend half a day, three views one to two days, packaging and the OpenWrt VM in the lab one day.
- **Where it lives:** the `openwrt/luci` feed requires the `discowall` package in `openwrt/packages` first; until
  then `contrib/openwrt/luci-app-discowall/` beside the package.

## 2. pfSense

Sources: Netgate docs *Developing Packages* and *Package Port Directory Structure*; the `devel` branch of
`pfsense/FreeBSD-ports`: `net/pfSense-pkg-Avahi` (Makefile, info.xml, avahi.xml, avahi.priv.inc, pkg-plist, the
function list of avahi.inc, the form of avahi_settings.php) and `net/pfSense-pkg-mDNS-Bridge` (mdns_bridge.xml, the
form of mdns-bridge.php, the function list of mdns-bridge.inc), `net/mdns-bridge` (the daemon's port) and the README
of `dennypage/mdns-bridge`; all read 2026-10-13.

### 2.1 How a pfSense package is built

- **Two ports.** The daemon is an ordinary FreeBSD port (`net/mdns-bridge`: `USE_GITHUB`, `USE_RC_SUBR=mdns_bridge`,
  installs the binary and a `.conf.sample`). The pfSense package is a second port `net/pfSense-pkg-<Name>` with
  `NO_BUILD=yes`, `RUN_DEPENDS` on the daemon's port, and `do-install` copying its files; both live in
  `pfsense/FreeBSD-ports`, submitted as a pull request against `devel`, built by Netgate's builders, reaching users
  when merged into the release branch. `make package` in the port directory builds it locally for `pkg add`.
- **Files of the package port** (from the docs and both packages):
  - `files/usr/local/share/pfSense-pkg-<Name>/info.xml` — the manifest: name, `internal_name`, descr, `%%PKGVERSION%%`,
    `configurationfile`.
  - `files/usr/local/pkg/<name>.xml` — the **packagegui descriptor**: `<menu>` (name, section `Services`, url),
    `<service>` (name, `rcfile`, `executable`, description, `starts_on_sync`), `<plugins>` (`plugin_carp` for HA),
    hooks `custom_php_resync_config_command`, `custom_php_pre_deinstall_command`, `custom_php_install_command`,
    `custom_php_validation_command` …; `<donotsave>true</donotsave>` when the page has its own form (both packages do).
    The descriptor can also generate the form itself from `<fields>` (`interfaces_selection`, `checkbox`, `input`,
    `select`, `rowhelper` …) — the generated form is enough for simple settings, not for a rule table.
  - `files/usr/local/pkg/<name>.inc` — PHP: reads the settings from the firewall's `config.xml`
    (`config_get_path('installedpackages/<name>/config/0')`), writes the daemon's configuration file and an rc script
    (`write_rcfile([...])`), `start_service`/`stop_service`/`is_service_running`, CARP plugin, deinstall.
  - `files/usr/local/www/<name>.php` — the page: `require("guiconfig.inc")`, `$form = new Form`, `Form_Section`,
    `Form_Checkbox`, `Form_Select` (interfaces), `Form_Input`, `Form_Group`, `Form_StaticText`, `Form_Button`; on POST
    validate, `write_config()`, call the resync function.
  - `files/etc/inc/priv/<name>.priv.inc` — the privilege `page-services-<name>` matching the page.
  - `files/usr/local/www/shortcuts/pkg_<name>.inc` — the service/log/status shortcuts.
  - `pkg-plist`, `pkg-descr`, `files/pkg-install.in`/`pkg-deinstall.in` (identical across packages).
- **Settings live in `config.xml`**, under `installedpackages/<name>/config`; the package renders them to the
  application's native format on every sync. For discowall: render `discowall.toml` and `rules.toml`, then
  `service discowall restart` (or `discoctl apply` for rules only).

### 2.2 Prior art: mdns-bridge (and its pfSense package)

- `mdns-bridge` (Denny Page, BSD-2, C, v2.4.0 in ports) forwards mDNS between interfaces for IPv4 and IPv6 with
  **filtering**: a global allow/deny filter list and, per interface, inbound and outbound allow/deny lists; filters
  are label subsets of mDNS names (`_ipp`, `Office-Printer._ipp`), comma-separated; its README recommends allowing
  whole services and using host names only in deny lists. Configuration is an ini file with a global section and one
  section per interface. The pfSense page (`mdns-bridge.php`) shows: General (enable, interfaces, CARP), Global
  Settings (IP protocols, global filter mode and list), one section per interface (IP protocols, inbound filter mode
  and list, outbound filter mode and list), Advanced.
- **What discowall adds over it**, for the task's positioning: an ordered first-match chain instead of per-interface
  lists, matches on kind (query/response/announce/probe), source address/prefix, record type and record *bundles*,
  the legacy-query/QU proxy, SSDP, `watch`/`hosts`/`services` observability, and a CLI. **What its UI gets right**
  and ours should copy: the interface selector from the firewall's interface list, filter mode + list per direction as
  the simple case, CARP awareness.

### 2.3 What a discowall package would look like

- `net/discowall` — a **Rust port** (`USES=cargo`, `CARGO_CRATES` list generated by `make cargo-crates`), `USE_RC_SUBR=
  discowalld`, installs `discowalld`, `discoctl`, the examples. **Open:** whether pfSense's builders build cargo ports
  (FreeBSD ports do; pfSense's tree carries only what its packages need — to be asked on the Netgate forum or checked
  against an existing Rust-based pfSense package; none was found in `net/` on 2026-10-13). Fallback: ship a prebuilt
  static binary via `MASTER_SITES` (frowned upon in ports).
- `net/pfSense-pkg-discowall` — descriptor with `<service>` (rc `discowalld`), page `discowall.php` with: General
  (enable, interfaces as a multi-select from the firewall's interface list → `interfaces = [...]`, ipv4/ipv6, SSDP),
  **Rules** (a table of the chain: one row per rule with in/out/src/kind/service/instance/host/rrtype/proto/action/log;
  add/edit/delete with a `discowall_rule_edit.php` page, the pfSense pattern for lists is a separate edit page with
  `rowhelper` or a list page + edit page as in `avahi_settings.php`'s reflection filters) rendered to `rules.toml`,
  **Status** (read `discoctl --json status/hosts/services` and show tables; pfSense pages are PHP, `shell_exec` of
  `discoctl --json` is the simplest bridge; the control socket's permissions must allow the web server's user —
  pfSense's PHP runs as root), CARP plugin as in both packages.
- **Effort** after task 9: port one day (more if cargo ports are a problem), package and pages three to four days,
  a pfSense VM in the Proxmox lab (task 11 lab, a CE ISO) one day.

## 3. OPNsense (for the scope decision)

Source: `opnsense/plugins` `net/mdns-repeater` (Makefile, model XML, form XML, Menu.xml, ACL.xml, the configd
actions file, the Jinja template and `+TARGETS`, `plugins.inc.d/mdnsrepeater.inc`, the API controllers), read
2026-10-13; the OPNsense developer "hello world" example.

- A plugin is 15 files: `Makefile` (`PLUGIN_NAME`, `PLUGIN_DEPENDS=<daemon port>`, `.include "../../Mk/plugins.mk"`);
  a **model** `models/OPNsense/<Name>/<Name>.xml` (fields with types `BooleanField`, `InterfaceField` with
  `Multiple`, `NetworkField` …, mounted at `//OPNsense/<Name>` in config.xml) and its PHP class; a **form**
  `controllers/OPNsense/<Name>/forms/general.xml` (field ids → model, types checkbox/select_multiple/…); a Volt view;
  `Menu/Menu.xml`, `ACL/ACL.xml`; API controllers that are mostly declarations (`ApiMutableServiceControllerBase`
  with `$internalServiceClass/Template/Enabled/Name`); a **Jinja template** that renders the daemon's config
  (`service/templates/OPNsense/<Name>/…` + `+TARGETS` mapping to the output path) and `actions_<name>.conf` for
  configd (start/stop/status/restart = `service <daemon> …`); `plugins.inc.d/<name>.inc` registering the service.
- For discowall: model = interfaces, ipv4, ipv6, ssdp, default policy, and an **ArrayField of rules** (OPNsense's
  grids handle lists natively — a better fit for a rule table than pfSense's rowhelper); template renders
  `discowall.toml` and `rules.toml`. Same FreeBSD prerequisite (task 9); the daemon's port is shared with pfSense.
- Effort after task 9: two to three days; structurally the least code of the three UIs.

## 4. Open for the maintainer (task 14)

1. pfSense only, or pfSense and OPNsense from the same FreeBSD port? (OPNsense is less work and has a cleaner model;
   pfSense has the larger installed base. The daemon port is shared.)
2. pfSense CE or Plus as the target (the package repository is the same tree; CE for the lab VM).
3. Does the FreeBSD backend (task 9) get pulled forward for this, or does task 14 wait?

Sources: Netgate docs [Developing Packages](https://docs.netgate.com/pfsense/en/latest/development/develop-packages.html)
and [Package Port Directory Structure](https://docs.netgate.com/pfsense/en/latest/development/package-directories.html);
[pfsense/FreeBSD-ports](https://github.com/pfsense/FreeBSD-ports) `devel`: `net/pfSense-pkg-Avahi`,
`net/pfSense-pkg-mDNS-Bridge`, `net/mdns-bridge`; [dennypage/mdns-bridge](https://github.com/dennypage/mdns-bridge);
[opnsense/plugins](https://github.com/opnsense/plugins) `net/mdns-repeater`; [openwrt/luci](https://github.com/openwrt/luci)
`applications/luci-app-example`; [Freifunk GSoC 2023: Migrating LuCI Apps to JavaScript](https://blog.freifunk.net/2023/07/01/gsoc-23-migrating-luci-apps-to-javascript-a-comprehensive-guide/).
