## 4. P2: discoctl watch, hosts, services, stats, status; per-rule counters and log ☑️

**Created** 2026-10-10.

**Created** 2026-10-10 (Fable, from the Q&A of the same day). Research: [mdns-router-research.md](../../../mdns-router-research.md) section 3.6, 6 (P2). Decisions D-1…D-9 apply.

- `discoctl watch` (streaming: time, in-iface, source, kind, names, decision, rule, out-ifaces), `hosts`, `services`, `stats` (per interface and rule), `status`; all with `--json`.
- Per-rule hit counters; a rule's `log` flag emits a structured journal line per hit.
