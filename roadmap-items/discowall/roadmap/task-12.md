## 12. avahi inside the namespace test: publish in one leaf, browse in the other, avahi beside the daemon in the router namespace ☑️

**Created** 2026-10-10.

**Created** 2026-10-10 (Fable, research [../notes/e2e-testing-research.md](../notes/e2e-testing-research.md),
option 1). Cheap extension of the gate: install `avahi-daemon` on the runner, run it inside the leaf namespaces
(own D-Bus session, `avahi-publish` in one, `avahi-browse -rpt` in the other) and once inside the router
namespace with `enable-reflector=no` beside discowalld. Skips where avahi is not installed. About half a day.
