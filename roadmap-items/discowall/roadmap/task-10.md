## 10. Lab: avahi coexistence on port 5353 (multicast delivery to both, unicast to one) ☑️

**Created** 2026-10-10.

**Created** 2026-10-10 (Fable, from the Q&A of the same day). Research: [mdns-router-research.md](../../../mdns-router-research.md) section 3.1 (beside avahi). Decisions D-1…D-9 apply.

A lab measurement, not code (the maintainer's lab, OQ-11; nothing lab-specific is written here, D-9). On a Linux host running avahi-daemon with its default `disallow-other-stacks=no`: bind a second UDP socket to 0.0.0.0:5353 with SO_REUSEADDR (and with SO_REUSEPORT), join the group on one interface, and record (1) whether multicast queries and responses arrive at both sockets, (2) which socket receives a unicast packet to port 5353, (3) whether avahi's own reflector, if enabled, and discowall ping-pong. Result goes into this item and into research section 3.1 with a **[measured]** mark; task 2's socket options follow it. Expected: (1) both, (2) one of them, (3) only if both reflect between the same interfaces.
