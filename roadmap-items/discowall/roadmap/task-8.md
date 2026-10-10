## 8. SSDP (UPnP, 239.255.255.250:1900) as a second protocol 🕵️

**Created** 2026-10-10.

**Created** 2026-10-10 (Fable, from the Q&A of the same day). Research: [mdns-router-research.md](../../../mdns-router-research.md) section 7 item 9. Decisions D-1…D-9 apply.

Postponed (D-3): after v1. A second parser (SSDP is HTTP-like text over UDP 1900, M-SEARCH/NOTIFY, with ST/NT/USN as the names) and a second socket set on 239.255.255.250:1900 / ff02::c; the rule chain gets `proto mdns|ssdp`. The layers of tasks 2 and 3 are written so this is an addition.

**Status 2026-10-10 (Fable, on the maintainer's word of the same day — D-3's "later" is now):** built, green
locally including the namespace test; waits for CI and a router.

- `ssdp.rs`: the parser for `NOTIFY` (alive/byebye/update), `M-SEARCH` (needs `MAN: "ssdp:discover"`) and
  `HTTP/1.1 200 OK`; NT/ST → `service`, the USN's `uuid:` → `instance`, the LOCATION's host → `host`, max-age, MX.
- Sockets: `Proto` on the trait and on `Received`; a second socket pair on 239.255.255.250:1900 / ff02::c with
  TTL 4, opened when `ssdp = true` (default on). `SocketFactory` takes the protocol.
- Reflector `on_ssdp`: notifications are forwarded by multicast under the chain (one message = one subject,
  `kind` announce), **searches are always proxied** (UPnP answers are unicast to the searcher's port: the proxy
  asks from an ephemeral 1900-family socket, window MX+0.5 s capped at 5.5 s, answers relayed by unicast from
  port 1900 after the rules with `kind` response), a stray 200 OK on the multicast socket is not forwarded, a
  LOCATION on a link-local address is dropped. The host table learns uuid → type/host/port from alive and
  response messages, a byebye expires at once; `bundle_of` resolves a uuid, so a byebye that names only
  `upnp:rootdevice` still matches a rule on the renderer type.
- Rules: `proto mdns|ssdp` as a match (absent: both), in the file, the CLI and `rule test`; watch events carry
  `proto`. `contrib/rules.toml` and both READMEs have the UPnP recipe and the SSDP name mapping.
- Tests: 44. Parser tests, the host-table test, two reflector tests (rules and table for notifications; the
  search proxy and relay, the stray answer, the link-local LOCATION), the discoctl parser, and the namespace test:
  an alive announced in leaf B reaches leaf A from the router's 1900, an `ssdp:all` search from A is answered by a
  responder in B and the answer comes back to A by unicast from the router.
- Not done: SSDP over IPv6 is implemented (ff02::c) but only unit- and v4-tested; the UPnP site-local group
  ff05::c is not joined.
