## 5. P3: legacy-query proxy and QU, fe80 stripping, IPv6 end to end, dedup, fuzzing, unit hardening 🕵️

**Created** 2026-10-10.

**Created** 2026-10-10 (Fable, from the Q&A of the same day). Research: [mdns-router-research.md](../../../mdns-router-research.md) sections 1, 3.7, 6 (P3). Decisions D-1…D-9 apply.

- Legacy one-shot queries and QU (D-6): re-ask on the target interfaces from an ephemeral port, collect for ~500 ms, relay unicast to the original source; never rely on unicast to 5353 (avahi may hold it, D-3).
- Strip fe80::/10 AAAA (option, default on); IPv6 end to end in the netns tests; dedup against a second reflector; `cargo fuzz` on the parser boundary; unit hardening reviewed.

**Status 2026-10-10 (Fable):** built, green locally including the namespace test; waits for CI and a router.

- **Legacy one-shot queries** (`proxy.rs`, research 3.7, D-6): a query whose source port is not 5353 is not
  reflected. Instead the rules decide per target interface as for any query, and `proxy::ask` sends the (possibly
  rebuilt) question from an **ephemeral socket** of the same family (`LinuxSocket::open_ephemeral`, through the
  `SocketFactory` the core holds) on every allowed interface; a thread collects the unicast answers for
  `legacy_window_ms` (default 500, 0 drops such queries) and hands them to the core tagged with `reply_to` (the
  querier's address and interface). `relay_answer` runs them through the rules (in: the answering interface, out:
  the querier's) and sends them **by unicast from port 5353** to the querier (`send_unicast` on the trait, `send_to`
  with the pktinfo egress). Counters `proxied_queries`, `proxied_answers`; watch results `asked`, `asked-partial`,
  `relayed`, `relayed-partial`. The answers carry the query's id and question as the responder wrote them.
- **QU bit:** cleared on every forwarded question (the rebuild does it), the id stays.
- **Link-local addresses:** `strip_link_local` (default on) removes A/AAAA records with fe80::/10 or 169.254/16
  from forwarded packets; counter `dropped_link_local`, not counted as a rule drop.
- **IPv6 end to end:** the namespace test gives the router and the leaves ULA addresses, waits for DAD, and sends
  the browse over ff02::fb; it arrives in the other leaf from the router's link-local address on that interface.
  `send_to` sets the scope id on multicast and link-local v6 destinations.
- **Parser robustness:** `packet::tests::random_and_mutated_input_never_panics` runs 20 000 random and mutated
  packets through parse, classification and re-encode (no nightly, so no `cargo fuzz`; the idea is the same).
- **Dedup against a second reflector:** the hash window exists since P0; a test with a second reflector on the
  same segments needs avahi, i.e. the lab (task 10). Not done here.
- **Unit hardening:** `contrib/discowalld.service` is unchanged (no capabilities, DynamicUser, strict sandbox);
  the ephemeral sockets need nothing new. `systemd-analyze security` was not run (no systemd on this WSL).
- Tests: 36. The namespace test also runs the legacy scenario end to end: a `dig`-style query from an ephemeral
  port in leaf A, a legacy responder in leaf B (`mdns_probe.py respond`) that answers the daemon's ephemeral query
  by unicast, and the answer arriving in A from the router's 5353.
