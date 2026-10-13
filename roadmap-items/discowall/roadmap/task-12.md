## 12. avahi inside the namespace test: publish in one leaf, browse in the other, avahi beside the daemon in the router namespace 🕵️

**Created** 2026-10-10.

**Created** 2026-10-10 (Fable, research [../notes/e2e-testing-research.md](../notes/e2e-testing-research.md),
option 1). Cheap extension of the gate: install `avahi-daemon` on the runner, run it inside the leaf namespaces
(own D-Bus session, `avahi-publish` in one, `avahi-browse -rpt` in the other) and once inside the router
namespace with `enable-reflector=no` beside discowalld. Skips where avahi is not installed. About half a day.

🏗️ 2026-10-13: started; developed here, tested in the lab's lan host container (avahi, nested user namespaces), then on GitHub.

**Built 2026-10-13** — `avahi_between_namespaces` in `crates/discowalld/tests/netns.rs` (the topology is now a
`Rig` shared with the first test). Each leaf gets its own mount namespace with a tmpfs on `/run` and a bind-mounted
`passwd`/`group` in which `avahi` is uid 0 (avahi insists that `/run/avahi-daemon` belongs to its user, and a user
namespace with one mapped uid cannot chown); a private D-Bus system bus per instance (`dbus-daemon --config-file`,
permissive policy, `DBUS_SYSTEM_BUS_ADDRESS` for the tools); `avahi-daemon --no-drop-root --no-chroot --no-rlimits
-f` with `allow-interfaces` and its own `host-name`. B publishes `Lab Printer`, A browses and resolves it over
IPv4 and IPv6 through default-drop rules (the lab's printer rules; this is also the regression test for B-2),
`avahi-resolve-host-name leafb.local`; then avahi beside discowalld in the router namespace with
`enable-reflector=no`: `router.local` resolves from A (multicast reached avahi), A's avahi restarted for a fresh
cache still browses the printer (multicast reached discowalld), own-source drops > 0 (task 10's result (1) in the
gate). Skips without avahi-daemon/avahi-utils/dbus-daemon. CI installs them (`ci.yml`).

**Tested** 2026-10-13 in the lab's lan host container (Debian 13, avahi 0.8, nested user namespaces): both namespace
tests pass, the avahi one in 12 s. This WSL has no avahi (skips). 🕵️ until the first CI run on GitHub is green
(the maintainer pushes).
