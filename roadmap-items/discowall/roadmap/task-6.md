## 6. P4: packaging (tarball, deb, rpm, OpenWrt), cross builds, release workflow, README and man pages 🕵️

**Created** 2026-10-10.

**Created** 2026-10-10 (Fable, from the Q&A of the same day). Research: [mdns-router-research.md](../../../mdns-router-research.md) section 5, 6 (P4). Decisions D-1…D-9 apply.

- Static musl tarballs (aarch64, armv7, x86_64) with unit and example config; `cargo-deb`; rpm; OpenWrt feed package with procd script; release workflow on tags (the maintainer tags).
- README: the rule language, recipes (HomeKit, AirPlay/RAOP, printers, Chromecast, Sonos), the firewall reminder (discovery is not the data path); man pages from clap.

**Status 2026-10-10 (Fable):** built; the release workflow has never run (no tag: the maintainer tags, D-9).

- **Static builds:** `x86_64-unknown-linux-musl` builds locally with `rustup target add` alone (every crate is pure
  Rust); discowalld 4.0 MiB, discoctl 1.0 MiB, no dependencies. aarch64 and armv7 musl need a cross linker and
  are left to `cross` in the workflow.
- **Man pages** from clap: `discoctl generate-man <dir>` (hidden subcommand, one page per command) and
  `discowalld --generate-man <dir>`; `clap_mangen`.
- **`.deb` and `.rpm`:** `[package.metadata.deb]` and `[package.metadata.generate-rpm]` in `crates/discowalld/
  Cargo.toml` package both binaries, the two config files (conffiles), the unit, the README and three man pages
  under the package name `discowall`; built and inspected locally with `cargo deb --no-build --target
  x86_64-unknown-linux-musl` and `cargo generate-rpm` (1.4 / 1.5 MiB). `depends = ""`: static binary.
- **Release workflow** `.github/workflows/release.yml`: on a `v*` tag, a matrix of three musl targets built with
  `cross`, tarballs (binaries, man pages, unit, examples, READMEs, LICENSE), `.deb` for amd64/arm64/armhf, `.rpm`
  for x86_64/aarch64/armv7hl, all attached to the GitHub release. **Untested until the first tag.** The
  `cargo-generate-rpm --target` path rewriting and the `cross` builds are the two things most likely to need a
  fix on the first run.
- **OpenWrt:** `contrib/openwrt/discowall/` with a `Makefile` on the packages feed's `rust-package.mk` and a
  procd init script. **Untested** (no OpenWrt build host here); `PKG_HASH:=skip` until a release exists.
- **Docs:** both READMEs have Install, Configuration, Rules (the match table, the file and the CLI form,
  whitelist/blacklist), Recipes (HomeKit, AirPlay/Spotify, printers, Cast with a guest block, Sonos), the firewall
  reminder with the ports, discoctl, How it works. `cargo install --git` works for source installs.
- **Not done:** nothing of P4's list; what is listed as untested needs the first tag and an OpenWrt host.

**2026-10-10, later:** the release workflow ran for the first time on the tag `v0.0.1` (the maintainer's word) and
**succeeded on the first run**: three musl targets with `cross`, tarballs, `.deb` for amd64/arm64/armhf, `.rpm` for
x86_64/aarch64/armv7hl, nine assets on
[github.com/hobbyquaker/discowall/releases/tag/v0.0.1](https://github.com/hobbyquaker/discowall/releases/tag/v0.0.1).
Untested still: the OpenWrt package (needs a build host) and installing the packages on real systems (lab, task 11).
