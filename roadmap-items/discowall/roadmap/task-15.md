## 15. IGMP snooping without a querier: SSDP dies minutes after start (measured on a Linux bridge) — document, or refresh memberships 🤷

**Created** 2026-10-13.

**Created** 2026-10-13 (Fable, from the lab). **Measured** in the Proxmox lab (task 11): the node's Linux bridge
`vmbr0` has `multicast_snooping=1`, `multicast_querier=0`, `multicast_membership_interval=26000` (260 s). The lab's
`ssdp` case passed when run alone and failed whenever it ran a few minutes after the daemon's last start: `discoctl
watch` showed the M-SEARCH from the lan host no longer reaching the router at all, or reaching it and the proxied
search on iot getting no answer; `bridge mdb show` had `239.255.255.250` for the lan and iot hosts' ports (they had
just joined) but **not for the router's ports** — their memberships had expired, nobody queried, the bridge stopped
forwarding the group to them. **mDNS was never affected:** Linux bridges flood `224.0.0.0/24` (the local network
control block) regardless of snooping; `239.255.255.250` is snooped. The same expiry hits every SSDP device and
control point on such a bridge, not only discowall. A real network has a querier (a router or a switch); the lab now
runs one on the router container (`tests/lab/igmp_querier.py`, see the lab README).

**For discowall (the maintainer decides, 🤷):**
1. **Document only** (README troubleshooting): "SSDP works for a few minutes after start and then stops, mDNS keeps
   working → your bridge/switch does IGMP snooping without a querier; enable the querier (Linux bridge:
   `multicast_querier=1`; Proxmox: `bridge-mcsnoop` / `bridge-mcquerier` in `/etc/network/interfaces`)".
2. **Refresh memberships** from the daemon every ~2 minutes (leave + re-join, or an unsolicited report) — keeps
   discowall itself reachable, but the devices and control points on the same bridge still expire, so the user's
   problem is only half solved; and a leave can make a snooping bridge drop the group for a moment.
3. **Be the querier** (`igmp_querier = true` config option sending IGMPv2 general queries and MLDv1 queries on the
   configured interfaces): fixes the whole network behind the bridge, is what the lab does, and is unusual for a
   discovery daemon; needs `CAP_NET_RAW` for the raw IGMP socket (D-7 says no `NET_RAW`) — or `IPPROTO_IGMP`
   datagram sockets, which also need the capability.

Recommendation: 1 now (a paragraph in both READMEs), 2 or 3 only if users report it.
