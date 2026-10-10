## 7. nftables set feed: addresses of allowed hosts per interface pair 💤

**Created** 2026-10-10.

**Created** 2026-10-10 (Fable, from the Q&A of the same day). Research: [mdns-router-research.md](../../../mdns-router-research.md) section 3.8 (P5). Decisions D-1…D-9 apply.

Postponed (D-8): after v1. The daemon maintains nftables sets with the addresses of hosts whose services were accepted per interface pair, so one static nft rule per pair opens the data path. Needs `nftables` crate or `nft` subprocess; decide then.
