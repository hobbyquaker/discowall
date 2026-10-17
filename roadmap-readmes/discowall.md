# discowall roadmap

## Roadmap

- 18. README: drop the "has not run on a real router yet" caution (first real-router run 2026-10-17) ☑️ — [task-18.md](../roadmap-items/discowall/roadmap/task-18.md)
- 17. Say why a packet was unparsable: reason in discoctl watch and a rate-limited info log ☑️ — [task-17.md](../roadmap-items/discowall/roadmap/task-17.md)
- B-5. discowalld --check does not read the rules file: a broken rules.toml passes ☑️ — [bug-5.md](../roadmap-items/discowall/roadmap/bug-5.md)
- B-4. A TXT record with RDLENGTH 0 makes the whole packet unparsable (hickory InvalidEmptyRecord) 🤷 — [bug-4.md](../roadmap-items/discowall/roadmap/bug-4.md)
- 16. OpenWrt packages as release assets: luci-app-discowall (all) and discowall .ipk/.apk per architecture from the prebuilt musl binaries 🕵️ — [task-16.md](../roadmap-items/discowall/roadmap/task-16.md)
- 15. IGMP snooping without a querier: SSDP dies minutes after start (measured on a Linux bridge) — document, or refresh memberships 🤷 — [task-15.md](../roadmap-items/discowall/roadmap/task-15.md)
- 14. pfSense package and web UI (and OPNsense plugin?): FreeBSD port, packagegui descriptor, rules page 🤷 — [task-14.md](../roadmap-items/discowall/roadmap/task-14.md)
- 13. LuCI app for the OpenWrt package: rules, hosts, services and status in the web UI 🕵️ — [task-13.md](../roadmap-items/discowall/roadmap/task-13.md)
- 9. FreeBSD backend (kqueue, IP_RECVIF) and pfSense package 💤 — [task-9.md](../roadmap-items/discowall/roadmap/task-9.md)
- 8. SSDP (UPnP, 239.255.255.250:1900) as a second protocol 🕵️ — [task-8.md](../roadmap-items/discowall/roadmap/task-8.md)
- 7. nftables set feed: addresses of allowed hosts per interface pair 💤 — [task-7.md](../roadmap-items/discowall/roadmap/task-7.md)
- 6. P4: packaging (tarball, deb, rpm, OpenWrt), cross builds, release workflow, README and man pages 🕵️ — [task-6.md](../roadmap-items/discowall/roadmap/task-6.md)
- 5. P3: legacy-query proxy and QU, fe80 stripping, IPv6 end to end, dedup, fuzzing, unit hardening 🕵️ — [task-5.md](../roadmap-items/discowall/roadmap/task-5.md)
- 4. P2: discoctl watch, hosts, services, stats, status; per-rule counters and log 🕵️ — [task-4.md](../roadmap-items/discowall/roadmap/task-4.md)
- 3. P1: parser, host table and record bundles, rule chain, control socket, discoctl rule commands 🕵️ — [task-3.md](../roadmap-items/discowall/roadmap/task-3.md)
- 2. P0: Cargo project, sockets, interface tracking, plain reflector with loop prevention, systemd unit 🕵️ — [task-2.md](../roadmap-items/discowall/roadmap/task-2.md)

## Archive

- B-3. discoctl save fails under the shipped unit: /etc/discowall is read-only for the daemon (ProtectSystem=strict, DynamicUser) ✅ — [bug-3.md](../roadmap-items/discowall/roadmap-archive/bug-3.md)
- 10. Lab: avahi coexistence on port 5353 (multicast delivery to both, unicast to one) ✅ — [task-10.md](../roadmap-items/discowall/roadmap-archive/task-10.md)
- B-2. Dedup drops the IPv4 twin of a query avahi sends on both families at once ✅ — [bug-2.md](../roadmap-items/discowall/roadmap-archive/bug-2.md)
- B-1. Log output carries ANSI colour codes when stdout is not a terminal (journal under systemd) ✅ — [bug-1.md](../roadmap-items/discowall/roadmap-archive/bug-1.md)
- 12. avahi inside the namespace test: publish in one leaf, browse in the other, avahi beside the daemon in the router namespace ✅ — [task-12.md](../roadmap-items/discowall/roadmap-archive/task-12.md)
- 11. Proxmox lab for end-to-end tests: three LXC on a VLAN-aware bridge, lab.py with the nine cases ✅ — [task-11.md](../roadmap-items/discowall/roadmap-archive/task-11.md)
- 1. Concept: rules model, CLI shape and runtime — discuss the research with the maintainer ✅ — [task-1.md](../roadmap-items/discowall/roadmap-archive/task-1.md)
