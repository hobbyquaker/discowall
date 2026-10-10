## 4. P2: discoctl watch, hosts, services, stats, status; per-rule counters and log 🕵️

**Created** 2026-10-10.

**Created** 2026-10-10 (Fable, from the Q&A of the same day). Research: [mdns-router-research.md](../../../mdns-router-research.md) section 3.6, 6 (P2). Decisions D-1…D-9 apply.

- `discoctl watch` (streaming: time, in-iface, source, kind, names, decision, rule, out-ifaces), `hosts`, `services`, `stats` (per interface and rule), `status`; all with `--json`.
- Per-rule hit counters; a rule's `log` flag emits a structured journal line per hit.

**Status 2026-10-10 (Fable):** built, green locally and in the namespace test; waits for CI and a router.

- `discoctl watch [--iface X] [--json]`: the daemon keeps a tokio broadcast channel of `WatchEvent`s (ts, in-iface,
  source, length, kind or the pre-rule drop reason, service types, instances, hosts, and per target interface
  `sent`/`partial`/`dropped`/`down`/`error` with the number of records dropped and the rules that decided). The
  control connection streams them as JSON lines until the client disconnects; a lagging client gets an error line
  and goes on. Text form: `13:05:01.123 iot 10.1.0.5:5353 announce _hap._tcp "living room" bridge.local -> lan sent(2)  guest dropped`.
  A closed pipe (`| head`) ends it quietly. No subscriber: no event is built.
- `discoctl services`: the instances per interface with service type, host, port and TTL left.
- `discoctl stats`: counters, per interface rx/tx, per rule hits in CLI form, default-policy hits, host and
  instance counts. (`rule list` keeps the hits too; `log` rules write a journal line per hit since P1.)
- Tests: the HomeKit unit test checks the first and last events and the stats; the namespace test runs
  `discoctl watch --json --iface b0` beside the announcements and checks both events, plus `services` and `stats`
  in text and JSON. 33 tests.
