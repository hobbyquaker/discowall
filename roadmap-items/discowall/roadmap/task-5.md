## 5. P3: legacy-query proxy and QU, fe80 stripping, IPv6 end to end, dedup, fuzzing, unit hardening 🏗️

**Created** 2026-10-10.

**Created** 2026-10-10 (Fable, from the Q&A of the same day). Research: [mdns-router-research.md](../../../mdns-router-research.md) sections 1, 3.7, 6 (P3). Decisions D-1…D-9 apply.

- Legacy one-shot queries and QU (D-6): re-ask on the target interfaces from an ephemeral port, collect for ~500 ms, relay unicast to the original source; never rely on unicast to 5353 (avahi may hold it, D-3).
- Strip fe80::/10 AAAA (option, default on); IPv6 end to end in the netns tests; dedup against a second reflector; `cargo fuzz` on the parser boundary; unit hardening reviewed.
