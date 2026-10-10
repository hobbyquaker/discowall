## 11. Proxmox lab for end-to-end tests: three LXC on a VLAN-aware bridge, lab.py with the nine cases 🤷

**Created** 2026-10-10.

**Created** 2026-10-10 (Fable, from the maintainer's ask of the same day for end-to-end testing suggestions).

The research: [../notes/e2e-testing-research.md](../notes/e2e-testing-research.md) — three Debian LXC containers
on a VLAN-aware bridge (router with lan/iot/guest tags, two hosts with avahi and a UPnP peer), a `tests/lab/lab.py`
driven over SSH with nine cases the GitHub gate cannot reach (install from the `.deb`, avahi end to end, avahi
beside discowalld = task 10, legacy/QU through real stacks, SSDP with a real control point, interface dynamics,
scale, IPv6 only, upgrade), the lab config outside the repository (D-9). Effort 2–3.5 days in all.

**Open** (the maintainer's, research section 5): fresh or real VLAN tags; SSH for sessions to `pct` and the
containers, or the maintainer creates the containers from `lab-create.sh`; OpenWrt VM later.
