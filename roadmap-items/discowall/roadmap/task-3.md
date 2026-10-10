## 3. P1: parser, host table and record bundles, rule chain, control socket, discoctl rule commands 🏗️

**Created** 2026-10-10.

**Created** 2026-10-10 (Fable, from the Q&A of the same day). Research: [mdns-router-research.md](../../../mdns-router-research.md) sections 3.3, 3.4, 3.5, 6 (P1). Decisions D-1…D-9 apply.

- Parser: hickory-proto (`mdns` feature) behind an internal trait (`parse → Packet`, `Packet → bytes`); fixtures from captures (anonymised).
- Classification: query | response | announce | probe; service types, instances, hosts, addresses.
- **Host table and bundles** (research 3.5): the part to prototype first; learned passively per in-interface, TTL-expired, resolves a name rule to PTR+SRV+TXT+A/AAAA.
- Rule chain (D-5): ordered, first match, default policy (OQ-10), matches in/out/src/kind/service/instance/host/rrtype, actions accept/drop + log; packet rebuilt with the surviving records.
- Control socket (`/run/discowall/control.sock`, JSON lines); `discoctl rule list|add|insert|del|flush`, `save`, `apply`, `test`, `--json`, exit codes 0/1/2/3; `/etc/discowall/rules.toml` reloaded atomically on SIGHUP or `apply`.
- Netns test: rules decide what crosses.

**Progress 2026-10-10 (Fable, while the maintainer was away):** started with the parser.

- `crates/discowalld/src/packet.rs`: `Packet::parse` on hickory-proto 0.26.3 (`mdns` feature, no default
  features): `Kind` (query / probe = query with authority records / response / announce = response with a
  cache-flush record), questions with the QU bit, records per section with name, type, TTL, cache-flush and a reduced
  `Data` (address, PTR target, SRV target+port, TXT, other); `service_types()`, `instances()`, `hosts()`,
  `addresses()`, `summary()`; the name helpers `service_type_of` (handles `_sub`, excludes the
  `_services._dns-sd._udp` meta-query), `is_instance`, `is_host`. Names are rendered from the raw labels as UTF-8
  (instance names have spaces; hickory's text form would escape them as `\032`), inner dots escaped, lowercased.
  The parsed `Message` is kept in the `Packet` for the rebuild after records are dropped.
- 7 unit tests (browse query with QU, a full HomeKit announcement, plain response, probe, UTF-8 names, name helpers,
  garbage). The reflector's debug line now carries the packet summary; nothing else uses the parser yet
  (`#[allow(dead_code)]` on the module until the rule chain lands).

**Next in this item:** the host table and bundles (research 3.5), then the rule chain and the packet rebuild, the
control protocol's rule commands, `discoctl rule …`, the rules file. Default policy assumed `drop` (OQ-10).
