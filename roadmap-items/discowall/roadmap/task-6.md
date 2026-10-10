## 6. P4: packaging (tarball, deb, rpm, OpenWrt), cross builds, release workflow, README and man pages ☑️

**Created** 2026-10-10.

**Created** 2026-10-10 (Fable, from the Q&A of the same day). Research: [mdns-router-research.md](../../../mdns-router-research.md) section 5, 6 (P4). Decisions D-1…D-9 apply.

- Static musl tarballs (aarch64, armv7, x86_64) with unit and example config; `cargo-deb`; rpm; OpenWrt feed package with procd script; release workflow on tags (the maintainer tags).
- README: the rule language, recipes (HomeKit, AirPlay/RAOP, printers, Chromecast, Sonos), the firewall reminder (discovery is not the data path); man pages from clap.
