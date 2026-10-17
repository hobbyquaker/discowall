## 17. Say why a packet was unparsable: reason in discoctl watch and a rate-limited info log ✅

**Created** 2026-10-17.

**Why:** on the first real-router run (task 18) the `unparsable` counter rose by about 200 in a few minutes.
`discoctl watch` only says `unparsable`, and the decode error is logged at `debug`
(`crates/discowalld/src/reflector.rs`, mDNS and SSDP paths). Finding the cause (B-4) took tcpdump, a hand decode of
the packet and a scratch program against hickory-proto. With the reason visible it would have been one look.

**Shape:**
- the watch event carries the decode error: `unparsable: InvalidEmptyRecord` (the JSON event gets a `reason` field);
- an `info` log line per source address and interface with the reason, rate-limited (e.g. once per source per
  10 minutes, with a count of the suppressed ones), so the journal shows it without `--log debug`;
- `discoctl stats` (or `status`) shows the last reason and source next to the counter;
- the same for the SSDP path;
- tests: the event and the rate limit.

**Built 2026-10-17.** One path for both protocols, `Reflector::unparsable` (`reflector.rs`): counts, remembers the
last one, sends the watch event with `reason` (new optional field of `WatchEvent`; older clients ignore it), and
logs at info at most once per source address and interface per 10 minutes, with `(N more from there since the last
report)`. `status` and `stats` carry `last_unparsable` (protocol, interface, source, reason, seconds ago), printed by
discoctl as `last unparsable: mdns from … on …, Ns ago: <reason>`; a watch line for such a packet ends with the
reason. The reason is hickory-proto's error text, e.g. `unexpected record with length 0 in non-update message` for an
empty record of a type other than TXT (measured in the unit test; an empty TXT is no longer unparsable, B-4).
Tests: the event's reason on the mDNS and SSDP paths, `status`/`stats` agree, the limiter (once per window, per
interface, the suppressed count after the window). READMEs (both): the discoctl section. ✅ 2026-10-17.
