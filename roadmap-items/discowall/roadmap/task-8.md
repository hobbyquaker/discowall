## 8. SSDP (UPnP, 239.255.255.250:1900) as a second protocol 💤

**Created** 2026-10-10.

**Created** 2026-10-10 (Fable, from the Q&A of the same day). Research: [mdns-router-research.md](../../../mdns-router-research.md) section 7 item 9. Decisions D-1…D-9 apply.

Postponed (D-3): after v1. A second parser (SSDP is HTTP-like text over UDP 1900, M-SEARCH/NOTIFY, with ST/NT/USN as the names) and a second socket set on 239.255.255.250:1900 / ff02::c; the rule chain gets `proto mdns|ssdp`. The layers of tasks 2 and 3 are written so this is an addition.
