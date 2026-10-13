## 11. Proxmox lab for end-to-end tests: three LXC on a VLAN-aware bridge, lab.py with the nine cases 🏗️

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
