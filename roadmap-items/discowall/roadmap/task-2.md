## 2. P0: Cargo project, sockets, interface tracking, plain reflector with loop prevention, systemd unit ☑️

**Created** 2026-10-10.

**Created** 2026-10-10 (Fable, from the Q&A of the same day). Research: [mdns-router-research.md](../../../mdns-router-research.md) sections 3.1, 3.2, 6 (P0). Decisions D-1…D-9 apply.

- Cargo workspace: `discowalld`, `discoctl`, a shared crate for the control protocol. MIT, `cargo fmt`/`clippy -D warnings`/`cargo test` as the gate, GitHub Actions from the first commit.
- Sockets per research 3.1 behind a **socket trait** (D-4): AF_INET and AF_INET6 UDP on 5353, SO_REUSEADDR+SO_REUSEPORT, IP_PKTINFO/IPV6_RECVPKTINFO, group join per configured interface, multicast loop off, TTL 255, send via IP_MULTICAST_IF control data.
- rtnetlink interface tracking: join/leave on link and address changes; own-address list for the loop check.
- Reflect everything between configured interfaces: never back to the in-interface, drop own-source packets, dedup cache (~1 s).
- TOML config (`/etc/discowall/discowall.toml`: interfaces, control socket path), journald logging via `tracing`, systemd unit with hardening, runs as user `discowall`.
- Tests: a netns integration test (router namespace with the daemon, two leaf namespaces over veth, a packet crosses, a packet does not come back). Skipped without the capability.
- Depends on task 10's result for the coexistence detail; does not wait for it.
