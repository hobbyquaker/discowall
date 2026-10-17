## 17. Say why a packet was unparsable: reason in discoctl watch and a rate-limited info log ☑️

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
