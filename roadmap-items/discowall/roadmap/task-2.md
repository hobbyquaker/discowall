## 2. P0: Cargo project, sockets, interface tracking, plain reflector with loop prevention, systemd unit 🕵️

**Created** 2026-10-10.

**Created** 2026-10-10 (Fable, from the Q&A of the same day). Research: [mdns-router-research.md](../../../mdns-router-research.md) sections 3.1, 3.2, 6 (P0). Decisions D-1…D-9 apply.

- Cargo workspace: `discowalld`, `discoctl`, a shared crate for the control protocol. MIT, `cargo fmt`/`clippy -D warnings`/`cargo test` as the gate, GitHub Actions from the first commit.
- Sockets per research 3.1 behind a **socket trait** (D-4): AF_INET and AF_INET6 UDP on 5353, SO_REUSEADDR+SO_REUSEPORT, IP_PKTINFO/IPV6_RECVPKTINFO, group join per configured interface, multicast loop off, TTL 255, send via IP_MULTICAST_IF control data.
- rtnetlink interface tracking: join/leave on link and address changes; own-address list for the loop check.
- Reflect everything between configured interfaces: never back to the in-interface, drop own-source packets, dedup cache (~1 s).
- TOML config (`/etc/discowall/discowall.toml`: interfaces, control socket path), journald logging via `tracing`, systemd unit with hardening, runs as user `discowall`.
- Tests: a netns integration test (router namespace with the daemon, two leaf namespaces over veth, a packet crosses, a packet does not come back). Skipped without the capability.
- Depends on task 10's result for the coexistence detail; does not wait for it.

**Status 2026-10-10 (Fable, while the maintainer was away):** built and green locally; waits for the first CI run
on GitHub (no remote yet) and for a try on a real router.

- Workspace `crates/discowall-proto` (the JSON-lines control protocol), `crates/discowalld`, `crates/discoctl`.
  Rust 1.99 stable, installed with rustup on the WSL machine this day.
- `discowalld`: `config.rs` (TOML, `deny_unknown_fields`, validation), `socket.rs` (the `MdnsSocket` trait and
  `linux::LinuxSocket`: socket2 + nix `recvmsg`/`sendmsg` with `IP_PKTINFO`/`IPV6_RECVPKTINFO`, `SO_REUSEADDR`+
  `SO_REUSEPORT`, `IP_MULTICAST_ALL=0`, loop off, TTL 255, egress interface per packet through the pktinfo control
  message), `ifaces.rs` (rtnetlink: dump then notifications; up = `Up && LowerUp`), `reflector.rs` (interface state,
  own-source drop, dedup hash window, forward to every other up+joined interface, counters, status), `control.rs`
  (unix socket, JSON lines: `ping`, `status`). Two blocking receive threads feed the tokio core over a channel.
- `discoctl status [--json]`, `ping`; exit codes 0/1/2/3; `--socket` or `DISCOWALL_SOCKET`.
- `contrib/discowall.toml`, `contrib/discowalld.service` (DynamicUser, no capabilities, strict sandbox).
- Tests: 2 + 6 unit tests (protocol, config, reflector with a fake socket) and `tests/netns.rs`: re-executes itself
  under `unshare -rnm`, builds a router namespace with two veths to two leaf namespaces, starts the daemon, sends an
  mDNS packet in leaf A with `tests/mdns_probe.py`, asserts it arrives in leaf B from the router's address, that
  nothing comes back to A, and that the status over the control socket shows both interfaces joined and tx=1.
  Skips with a message where user namespaces, `ip`, `nsenter` or `python3` are missing. Runs here in 2.4 s.
- Gate: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` — all green. CI workflow
  `.github/workflows/ci.yml` does the same plus a release build.
- Release binaries (glibc, x86_64, stripped, LTO): discowalld ≈ 1.6 MiB, discoctl ≈ 0.7 MiB. musl targets come
  with P4.

**Open in this item:** the first CI run; a try on a real router (two VLAN interfaces, `discoctl status`, a browse
from one side); IPv6 is implemented but only unit-tested (the netns test is v4; v6 end to end is P3's).
