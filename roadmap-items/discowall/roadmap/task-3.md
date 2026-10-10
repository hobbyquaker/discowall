## 3. P1: parser, host table and record bundles, rule chain, control socket, discoctl rule commands ☑️

**Created** 2026-10-10.

**Created** 2026-10-10 (Fable, from the Q&A of the same day). Research: [mdns-router-research.md](../../../mdns-router-research.md) sections 3.3, 3.4, 3.5, 6 (P1). Decisions D-1…D-9 apply.

- Parser: hickory-proto (`mdns` feature) behind an internal trait (`parse → Packet`, `Packet → bytes`); fixtures from captures (anonymised).
- Classification: query | response | announce | probe; service types, instances, hosts, addresses.
- **Host table and bundles** (research 3.5): the part to prototype first; learned passively per in-interface, TTL-expired, resolves a name rule to PTR+SRV+TXT+A/AAAA.
- Rule chain (D-5): ordered, first match, default policy (OQ-10), matches in/out/src/kind/service/instance/host/rrtype, actions accept/drop + log; packet rebuilt with the surviving records.
- Control socket (`/run/discowall/control.sock`, JSON lines); `discoctl rule list|add|insert|del|flush`, `save`, `apply`, `test`, `--json`, exit codes 0/1/2/3; `/etc/discowall/rules.toml` reloaded atomically on SIGHUP or `apply`.
- Netns test: rules decide what crosses.
