## 10. Lab: avahi coexistence on port 5353 (multicast delivery to both, unicast to one) ✅

**Created** 2026-10-10.

**Created** 2026-10-10 (Fable, from the Q&A of the same day). Research: [mdns-router-research.md](../../../mdns-router-research.md) section 3.1 (beside avahi). Decisions D-1…D-9 apply.

A lab measurement, not code (the maintainer's lab, OQ-11; nothing lab-specific is written here, D-9). On a Linux host running avahi-daemon with its default `disallow-other-stacks=no`: bind a second UDP socket to 0.0.0.0:5353 with SO_REUSEADDR (and with SO_REUSEPORT), join the group on one interface, and record (1) whether multicast queries and responses arrive at both sockets, (2) which socket receives a unicast packet to port 5353, (3) whether avahi's own reflector, if enabled, and discowall ping-pong. Result goes into this item and into research section 3.1 with a **[measured]** mark; task 2's socket options follow it. Expected: (1) both, (2) one of them, (3) only if both reflect between the same interfaces.

**Measured 2026-10-13** in the Proxmox lab (task 11), case `coexistence` of `tests/lab/lab.py`, 9/9 checks; the
environment and the three results are in research section 3.1 with the **[measured]** mark:

1. **Multicast: both.** avahi on the router (allow-interfaces lan,iot,guest, enable-reflector=no) answers
   `discowall-router.local` for a querier on lan while discowalld, on the same host, forwards that querier's browse
   of the printer from iot. `ss -lunp` shows both daemons on `0.0.0.0:5353` and `[::]:5353`.
2. **Unicast to 5353: the socket bound last.** Measured both ways by restarting one daemon after the other: a unicast
   query from an ephemeral port and one from port 5353 reach discowalld (visible in `discoctl watch`, avahi silent)
   when discowalld bound last, and avahi (answers by unicast, nothing in `watch`) when avahi bound last. discowall
   never relies on unicast to 5353 (D-3, D-6; the proxy asks from an ephemeral socket), so the order is harmless.
3. **Ping-pong with avahi's reflector on: none.** 8 packets on lan in 10 s around a fresh announcement and a browse;
   avahi's reflected copies have the router's address as source and are dropped as own-source (counter rose), the
   rest dedup; the printer still browses. No change to task 2's socket options needed (SO_REUSEADDR and SO_REUSEPORT
   are set; both daemons coexist as the research expected).

Found on the way, both fixed: B-1 (ANSI codes in the journal), B-2 (dedup dropped the IPv4 twin of avahi's
dual-family queries). ✅ 2026-10-13.
