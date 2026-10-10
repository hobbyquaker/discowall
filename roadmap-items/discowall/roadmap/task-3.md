## 3. P1: parser, host table and record bundles, rule chain, control socket, discoctl rule commands 🕵️

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

**Status 2026-10-10, later (Fable):** built; the P1 scope is in and green locally, waiting for CI and a router.

- `rules.rs`: `Glob` (`*`/`?`, case-insensitive), `Prefix` (address or CIDR, v4/v6, no crate), `Rule::compile`
  from `RuleSpec`, `Subject` (in/out/src/kind/services/instances/hosts/rrtype), `Chain` with `decide` (counts hits)
  and `peek` (`rule test`), insert/remove/flush/to_set/view; `short_instance`, `short_host`.
- `hosts.rs`: the host table per in-interface — instance → service/host/port, host → addresses/instances, TTL
  expiry every 5 s, goodbyes (TTL 0) honoured, probes' authority records learned; `bundle_of(iface, name)` ties a
  name to its bundle; `view()` for `discoctl hosts`.
- `reflector.rs`: parse → learn → per target interface `filter`: every question and every record gets a `Subject`
  from its own name(s) plus the bundle the table knows, the chain decides, dropped records are removed and the
  packet is **rebuilt** with hickory (`rebuild`); a query with no question left or a response with no answer left is
  not sent; `log` rules write a journal line per hit. Counters: `dropped_unparsable`, `dropped_by_rules`,
  `records_dropped`. `handle(Request)` answers every control request; SIGHUP reloads the rules file.
- Rules file: `rules_file` in the config (default `/etc/discowall/rules.toml`), `RuleSet` TOML (`default`,
  `[[rule]]`), missing file = drop everything; `save` writes atomically (tmp + rename), `apply` reloads.
- `discoctl`: `rule list|add|insert N|del N|flush|test`, `save`, `apply`, `hosts`, all with `--json`; the rule
  syntax parser (`in lan out iot kind query service _hap._tcp accept [log] [comment …]`) with tests; exit code 2 on
  a syntax error; `--help` prints the syntax.
- Protocol crate: `RuleSpec` (serde, `Display` in the CLI form), `RuleSet`, `RuleList`/`RuleView`, `TestSpec`/
  `TestResult`, `HostView`; requests `rule_list`, `rule_add`, `rule_del`, `rule_flush`, `rule_save`, `rule_apply`,
  `rule_test`, `hosts`. A serde trap found by the netns test: a tagged enum cannot hold a newtype variant with a
  sequence, so `hosts` is a struct variant.
- Tests: 31 in all. `reflector.rs` has the HomeKit scenario (direction, bundles through the table, a mixed packet
  rebuilt with one PTR, counters step by step), the rule commands with save/apply/test, the missing-file case.
  `tests/netns.rs` now sends real mDNS packets (`tests/mdns_probe.py` builds queries and announcements and decodes
  what it receives), configures the chain through **discoctl** (flush, add, save, apply, list, test), announces a
  camera and a bridge in leaf B and asserts that only the bridge's announcement reaches leaf A, then checks
  `discoctl hosts` text and JSON. The test kills its daemon on drop (a failing assertion left one behind once and
  hung `cargo test` on the inherited stdout). `contrib/rules.toml` is the example.
- `cargo test -p discowalld` does not rebuild `discoctl`; `cargo build --all` first (CI does).

**Still open in P1's scope:** nothing functional; `discoctl watch` and per-interface stats are P2 (task 4), the QU
bit, legacy queries and fe80 stripping are P3 (task 5). Default policy `drop` assumed (OQ-10).
