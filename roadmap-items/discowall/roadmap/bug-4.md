## B-4. A TXT record with RDLENGTH 0 makes the whole packet unparsable (hickory InvalidEmptyRecord) 🕵️

**Created** 2026-10-17.

**Found** 2026-10-17 on the first real-router run (task 18): `discoctl status` showed the `unparsable` counter
rising by one to two packets a second, and `discoctl watch` showed `unparsable` for **every** packet from one host in
one segment, over IPv4 and IPv6 alike.

**The packets** (measured with tcpdump and decoded by hand): the host is a Windows PC running Meta Quest Link, which
announces `_oculusal_sp._tcp` and `_oculusal_sp_v2._tcp`. Its responses carry one answer (PTR) and six additional
records (SRV, TXT, A, three AAAA). The TXT record has **RDLENGTH 0**. Everything else is well-formed.

**Reproduced** 2026-10-17 with hickory-proto 0.26.3 (the version in `crates/discowalld/Cargo.toml`): the captured
packet fed to `Message::from_vec` gives `Err(InvalidEmptyRecord)`. The same packet with that TXT record changed to
RDLENGTH 1 (one zero-length string) parses with all records.

**Effect:** the whole packet is dropped, so PTR, SRV, A and AAAA never reach the other interfaces either: the
service is invisible outside its segment.

**What the RFC says** (RFC 6763 §6.1): an empty TXT record "is not strictly legal, but should one be received, it
should be interpreted as the same as a single empty string"; DNS-SD clients MUST treat a TXT record with a single
zero byte, an empty TXT record and no TXT record as equivalent. A reflector should therefore not drop it.

**To decide (🤷):**
- a pre-pass before hickory that rewrites an RDLENGTH-0 TXT into a single zero byte, then decide whether the
  original bytes or the re-encoded packet are forwarded;
- or a newer hickory-proto that accepts it (check the changelog), or a patch upstream;
- or the own parser that `packet.rs` already keeps the door open for (research 3.3).

Whatever the fix: a unit test with a hand-built packet (PTR + SRV + empty TXT + A), not captured lab data.

**Refined 2026-10-17 (maintainer: "i would say we should forward as-is, as avahi reflector does also").**

**What avahi does — read from its source** (`avahi/avahi` master, 2026-10-17): `avahi-core/dns.c` parses a TXT with
`rdlength == 0` as an empty string list instead of failing; the reflector (`avahi-core/server.c`, `reflect_response`)
re-emits record by record, and `avahi-common/strlst.c` serialises an empty list as a single NUL byte ("to comply with
section 6.1 of the DNS-SD spec"). So avahi keeps the record and forwards it normalised, never drops the packet.

**Decision for the fix:** the packet is forwarded, not dropped. discowall already sends the **original bytes** when
the rules drop nothing (`filter()` in `reflector.rs`: `dropped == 0 && link_local == 0 && !qu` → `original.to_vec()`),
so only the parse has to succeed:

- **Lenient pre-pass, classification only.** When `Message::from_vec` fails, walk the wire format once; for every
  record with TYPE TXT and RDLENGTH 0, make a **copy** with RDLENGTH 1 and one zero byte inserted, adjusting every
  compression pointer that targets an offset after an insertion point. Parse the copy. Record order and indices are
  unchanged, so the rule verdicts (`keep_r`) apply to the original packet one to one.
- **Forwarded as-is** when nothing is dropped: the original bytes, empty TXT and all — more literal than avahi.
- **Rebuilt** when the rules drop records, a link-local address is stripped or the QU bit is cleared: the rebuild
  comes from the parsed copy, so the TXT goes out as a single zero byte — exactly avahi's output and RFC 6763 §6.1's
  equivalent form.
- **Only TXT.** An RDLENGTH-0 record of another type stays unparsable (a one-byte A or SRV would be garbage); task 17
  makes the reason visible. If a later hickory-proto accepts an empty TXT, the pre-pass goes; note the version checked.
- **Counters:** the packet no longer counts as `unparsable`; a debug log line says an empty TXT was normalised.
- **Tests:** unit tests with hand-built packets (D-9: no captured lab data) — PTR + SRV + empty TXT + A forwarded
  byte-identical under accept-all; the same with a rule dropping the A record → rebuilt, TXT as `\x00`; an empty TXT
  behind a compression pointer that must be shifted; two empty TXTs in one packet; an empty A record still unparsable.
  Then the real-router check of task 18: the Quest Link service becomes visible outside its segment.

Effort: half a day. ☑️ 2026-10-17.

**Built 2026-10-17.** `normalize_empty_txt` in `crates/discowalld/src/packet.rs` walks the packet once (questions,
owner names, the compressible names inside NS/CNAME/PTR/NSEC/MX/SRV/SOA data), inserts one zero byte into every
RDLENGTH-0 TXT of a **copy** and moves every compression pointer whose target lies at or behind an insertion;
`Packet::parse` falls back to it only when the strict parse fails, and logs at debug. hickory-proto stays at 0.26.3
(the version checked). Tests, all on a hand-built announcement (PTR, empty TXT, SRV, A whose owner points behind
the insertion): the copy parses with all four records and the shifted pointer still names `pc.local`; two empty
TXTs; nothing to do for a well-formed packet; an empty A record stays unparsable; accept-all forwards the original
bytes byte-identical with `unparsable` at 0; a rule dropping the A record rebuilds the packet, which parses strictly
and carries the TXT as one zero byte. 🕵️ until the real router shows the Quest Link service outside its segment.

**Measured in the lab 2026-10-17** (0.0.3 deb on the router container): the hand-built announcement (PTR, empty TXT,
SRV, A) sent by multicast from the iot host arrived on the lan host **byte-identical**, from the router; `unparsable`
stayed 0; `discoctl hosts` learned `pc.local` with `_quest._tcp`. The nine lab cases pass with this build. Still 🕵️
until the real router shows the Quest Link service outside its segment.
