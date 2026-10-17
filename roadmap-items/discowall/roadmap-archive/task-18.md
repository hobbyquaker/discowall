## 18. README: drop the "has not run on a real router yet" caution (first real-router run 2026-10-17) ✅

**Created** 2026-10-17 (maintainer: one task for removing the "did not run on a real router" warning).

**The caution** is at the top of `README.md` and `README.de.md` (line 9): "The reflector with its rule chain and CLI
exists (P0, P1) but has not run on a real router yet."

**Since 2026-10-17 it has:** discowall 0.0.2 from the release `.deb` replaces avahi's reflector on a Debian 12
x86_64 router, under the shipped systemd unit:
- five interfaces: an untagged LAN, three VLANs and a VXLAN; IPv4 and IPv6;
- avahi-daemon keeps running beside it as the router's own responder (`enable-reflector=no`);
- the rule chain is `default = "accept"` to start with (the same as the reflector it replaces), SSDP off;
- verified on the day: all five interfaces joined on both families, Matter (`_matter._tcp`, `_matterc._udp`) and
  HomeKit (`_hap._tcp`) services from the other segments in `discoctl hosts`, IPv4/IPv6 twins dropped as
  duplicates, no send errors;
- found on the way: B-4 (empty TXT makes a packet unparsable), B-5 (`--check` ignores the rules file), task 17.

**To do:**
- replace the caution in both READMEs with what is true now (still under development; runs on a real router since
  2026-10-17 with 0.0.2), or drop it; the maintainer decides the wording;
- mark this task done in the README roadmap list (both READMEs).

Hold the change until the router has run for a while (a few days, a reboot and an interface flap) so that the
claim covers more than the first hour.

**Done 2026-10-17** on the maintainer's word ("fix/implement B4, B5, Task-17, Task-18, release 0.0.3"), which
overrides the hold above (a few days, a reboot, an interface flap). The `[!CAUTION]` at the top of both READMEs is
now a `[!NOTE]`: young (0.0.x), under development, on a real router since October 2026 (Debian, five interfaces
incl. VLANs and a VXLAN, beside avahi-daemon), every change tested end to end in the lab; rough edges expected. The
wording is the session's choice; the item asked the maintainer to decide it, so it may change. ✅ 2026-10-17.
