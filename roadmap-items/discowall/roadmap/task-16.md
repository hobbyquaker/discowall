## 16. OpenWrt packages as release assets: luci-app-discowall (all) and discowall .ipk/.apk per architecture from the prebuilt musl binaries 📥

**Created** 2026-10-13.

**Created** 2026-10-13 (maintainer: "will we have an artifact for openwrt to download then? or how do users install it
on openwrt?"). Today the release has tarballs, `.deb` and `.rpm`; OpenWrt users must build the two packages with the
SDK (`tests/lab/openwrt.md`) or install by hand from the static tarball (README, *OpenWrt*). That is not how OpenWrt
users install things.

**Shape:**
- **`luci-app-discowall_<v>_all.ipk`** (and `.apk` for OpenWrt ≥ 25): architecture-independent, no Rust; build it in the
  release workflow with `openwrt/gh-action-sdk` (or the SDK tarball in a job) for one target and attach it.
- **`discowall_<v>_<arch>.ipk`/`.apk`** from the **prebuilt static musl binaries** the release already builds
  (x86_64, aarch64, armv7): a second package Makefile (or a `PREBUILT` switch in the existing one) that installs the
  release tarball's binaries instead of compiling with the feed's Rust — the feed's Rust toolchain build takes about
  65 minutes per architecture in the SDK (measured 2026-10-13, task 13), too slow and too fragile for the release
  workflow. OpenWrt's package architectures are finer than Rust's targets (`aarch64_cortex-a53`, `aarch64_generic`,
  `arm_cortex-a7_neon-vfpv4`, `arm_cortex-a9_vfpv3-d16`, `x86_64` …); a static binary runs on every CPU of its family,
  so either build one `.ipk` per common OpenWrt arch from the same binary (a loop in the workflow) or document
  `opkg install --force-architecture`. Decide the list from the OpenWrt 24.10 target table (the lab's VM is `x86_64`).
- **MIPS** (the many older routers: `mips_24kc`, `mipsel_24kc`): Rust has no tier-2 musl targets for them
  (`mips-unknown-linux-musl` is tier 3 since Rust 1.72, no prebuilt std) — out of reach for the release workflow
  until `cross`/`build-std` is tried; say so in the README.
- Install docs: `opkg install ./discowall_….ipk ./luci-app-discowall_….ipk` (or `apk add --allow-untrusted …`), then
  `uci add_list discowall.main.interface=…`, `/etc/init.d/discowall enable; start`, the firewall note.
- Test: the lab's `openwrt` case takes `--ipk`; run it with the workflow's artefacts.

Effort: one to two days. **Open (maintainer):** which OpenWrt architectures to attach; sign the packages or stay
with `--allow-untrusted`/`--force-architecture`.

📥 2026-10-13: enqueued in an Opus lane (maintainer: "start an opus lane for task 16 and attach the ipk/apk artifacts to 0.0.2 release").
