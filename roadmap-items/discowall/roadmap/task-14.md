## 14. pfSense package and web UI (and OPNsense plugin?): FreeBSD port, packagegui descriptor, rules page 🤷

**Created** 2026-10-13.

**Created** 2026-10-13 (maintainer: "do research for pfsense ui and file another task for that"). Research:
[../notes/web-ui-research.md](../notes/web-ui-research.md), sections 2 and 3.

**The ask:** discowall installable and configurable from pfSense's web UI.

**Shape (read from the Netgate docs and the in-tree packages `pfSense-pkg-Avahi` and `pfSense-pkg-mDNS-Bridge`,
2026-10-13):** two FreeBSD ports in `pfsense/FreeBSD-ports` — `net/discowall` (the daemon, a `USES=cargo` port with
an rc script) and `net/pfSense-pkg-discowall` (`info.xml`, the packagegui descriptor `discowall.xml` with `<menu>`,
`<service>`, CARP plugin and the resync/deinstall hooks, `discowall.inc` rendering the settings from `config.xml`
into `discowall.toml` and `rules.toml` and starting the service, pages `discowall.php` (General: enable, interfaces,
ipv4/ipv6, SSDP; Status: `discoctl --json status/hosts/services`) and `discowall_rule_edit.php` (the chain as a list
with an edit page, the pfSense pattern), `discowall.priv.inc`, shortcuts, `pkg-plist`). Prior art and competitor on
pfSense: **mdns-bridge** (per-interface inbound/outbound allow/deny label lists); the research lists what discowall
adds and what to copy from its UI.

**Blocked by** task 9 (FreeBSD backend: kqueue, `IP_RECVIF`) — nothing runs on pfSense before that. 🚧 would be the
state once task 9 is enqueued; 🤷 now because of the questions below.

**Open (🤷, the maintainer's):**
1. pfSense only, or pfSense **and OPNsense** from the same daemon port? OPNsense's MVC plugin (model + form + Jinja
   template + configd actions, like `os-mdns-repeater`) is less code and its grids fit a rule table better.
2. pfSense CE or Plus as the target; CE for a lab VM in the Proxmox lab (task 11).
3. Pull task 9 forward for this, or let task 14 wait?
4. Does pfSense's build infrastructure build cargo (Rust) ports? To ask on the Netgate forum before writing the port;
   no Rust-based package was found in `net/` on 2026-10-13.

**Wanted from discowall for any UI:** `discoctl rule add --json <RuleSpec>` (no token syntax in UIs); otherwise the
file-and-apply path (`rules.toml` + `discoctl apply`/SIGHUP) is what the pfSense `.inc` and the OPNsense template use.
