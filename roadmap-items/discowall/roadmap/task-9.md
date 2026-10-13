## 9. FreeBSD backend (kqueue, IP_RECVIF) and pfSense package 💤

**Created** 2026-10-10.

**Created** 2026-10-10 (Fable, from the Q&A of the same day). Research: [mdns-router-research.md](../../../mdns-router-research.md) section 9 (platform). Decisions D-1…D-9 apply.

Postponed (D-4): after v1. A second backend of the socket trait: kqueue, `IP_RECVIF`/`IPV6_PKTINFO`, FreeBSD's multicast join semantics; CI on a FreeBSD runner; a pfSense package. pfSense ships avahi, so D-3's coexistence applies there as well.

**2026-10-13:** the pfSense *package* part of this item moves to task 14 (pfSense package and web UI, with the
research in notes/web-ui-research.md); this item keeps the FreeBSD backend (kqueue, IP_RECVIF) and the port of the
daemon, which task 14 depends on.
