## 11. Proxmox lab for end-to-end tests: three LXC on a VLAN-aware bridge, lab.py with the nine cases ✅

**Created** 2026-10-10.

**Created** 2026-10-10 (Fable, from the maintainer's ask of the same day for end-to-end testing suggestions).

The research: [../notes/e2e-testing-research.md](../notes/e2e-testing-research.md) — three Debian LXC containers
on a VLAN-aware bridge (router with lan/iot/guest tags, two hosts with avahi and a UPnP peer), a `tests/lab/lab.py`
driven over SSH with nine cases the GitHub gate cannot reach (install from the `.deb`, avahi end to end, avahi
beside discowalld = task 10, legacy/QU through real stacks, SSDP with a real control point, interface dynamics,
scale, IPv6 only, upgrade), the lab config outside the repository (D-9). Effort 2–3.5 days in all.

**Open** (the maintainer's, research section 5): fresh or real VLAN tags; SSH for sessions to `pct` and the
containers, or the maintainer creates the containers from `lab-create.sh`; OpenWrt VM later.

**2026-10-13 (maintainer):** "continue with our roadmap. you may create lxc containers on my proxmox for testing
(but dont touch the hosts systems itself and dont touch other containers/VMs!)". That answers section 5's second
question: a session has SSH to the Proxmox node and creates the containers itself. **Taken from that, by the
session (not maintainer decisions, changeable at any time):**

- **Fresh VLAN tags** (110 lan, 120 iot, 130 guest) rather than the real ones: the lab cannot reach any real device
  and no real device sees lab mDNS; real tags can be used later by editing `lab.toml`.
- **The harness talks to the node over SSH and to the containers with `pct exec`**, not with SSH into each
  container: no keys in the containers, no DHCP-address hunting, one hop. Each container has one untagged
  management interface for `apt` and `pct push`; avahi and discowalld are confined to the lab interfaces so that
  nothing leaks into the real network.
- **Four containers** (router + lan + iot + guest host): the guest host makes "the guest sees nothing" an assertion
  instead of an absence. Unprivileged, Debian 13 template, 512 MB each, ids and storage in `lab.toml`.
- OpenWrt VM: later, when the package is built (unchanged).

🏗️ 2026-10-13: lab being built.

**Built and run 2026-10-13.** `tests/lab/lab-create.sh` (on the node), `tests/lab/lab.py` (nine cases, `list`,
`provision`, `run --deb/--prev-deb/--soak/--json`), `tests/lab/igmp_querier.py`, `tests/lab/README.md`,
`tests/lab/lab.toml.example`; the real `lab.toml` and the packages live in `~/.config/discowall-lab/` (D-9).

**Final run** (0.0.2-dev2 build with B-1/B-2/B-3 fixed, upgrade from the v0.0.1 deb, soak 120 s), all green:
install 10/10 · avahi 8/8 · coexistence 9/9 · legacy 6/6 · ssdp 5/5 · dynamics 8/8 · scale 6/6 · ipv6only 4/4 ·
upgrade 9/9 (about 7.5 minutes). Against the v0.0.1 release the same harness fails `install` (B-1), `avahi` (B-2),
`scale` and `upgrade` (B-3) — that is what it was for.

**What the lab found:** B-1, B-2, B-3 (fixed, ✅) and task 15 (IGMP snooping without a querier on the node's bridge;
the lab runs its own querier now). Harness lessons: flush avahi's cache before every browse or a stale cache fakes a
pass; a daemon restart reloads the rules file, so set the chain afterwards; dig discards answers from an address other
than the one it queried (use `dig @router` or the probe); the old version cannot save (B-3), so the upgrade case writes
the rules file itself; python-zeroconf registers 50 services in parallel only with the async API.

**Not done, deliberately:** the hour-long soak (`--soak 3600`, run it before a release), an OpenWrt VM (task 6's
package must build first), the manual real-router checklist (research section 4 — the maintainer's). ✅ 2026-10-13.
