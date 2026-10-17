## B-4. A TXT record with RDLENGTH 0 makes the whole packet unparsable (hickory InvalidEmptyRecord) 🤷

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
