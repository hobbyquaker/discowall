## 16. OpenWrt packages as release assets: luci-app-discowall (all) and discowall .ipk/.apk per architecture from the prebuilt musl binaries 🕵️

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

Effort: one to two days.

**Q&A outcome (maintainer, 2026-10-13; recorded as D-14, D-15, D-16):**
- **Architectures (D-14):** x86_64; aarch64_cortex-a53, aarch64_cortex-a72, aarch64_generic; arm_cortex-a7_neon-vfpv4,
  arm_cortex-a9_vfpv3-d16, arm_cortex-a15_neon-vfpv4, arm_cortex-a8_vfpv3, arm_cortex-a9, arm_cortex-a9_neon — one
  static binary per family; the README says how to install on a sibling architecture; no MIPS.
- **Unsigned, local install (D-14):** `opkg install ./file.ipk`, `apk add --allow-untrusted`; a signed feed is not
  wanted now (its own task if ever).
- **Both formats (D-15):** `.ipk` for 24.10, `.apk` for 25.x, from the two SDKs in the release workflow, both
  attached to v0.0.2. The lab VM stays on 24.10: the x86_64 ipk is verified there with `lab.py`'s `openwrt` case;
  for `.apk` the package metadata and the ELF only.
- **No firewall rules from the package (D-16):** documentation only (README, OpenWrt section).

📥 2026-10-13: enqueued in an Opus lane (maintainer: "start an opus lane for task 16 and attach the ipk/apk artifacts to 0.0.2 release").

🏗️ 2026-10-13: started in the Opus lane.

**Built** (commit `d9b5420`):
- `contrib/openwrt/discowall-prebuilt/Makefile`: package `discowall` from the release tarball
  `discowall-<v>-<rust target>.tar.gz` (target from the SDK's `ARCH`: x86_64, aarch64, else armv7 musleabihf),
  `files` a link to `../discowall/files` (copy with `cp -rL`), the same install steps, conffiles, init script and
  uci-defaults as the source-built package, `STRIP:=:`/`RSTRIP:=:` so the binaries go in byte for byte,
  `DEPENDS:=@(x86_64||aarch64||arm_v7)`. The source-built `contrib/openwrt/discowall` is unchanged.
- `contrib/openwrt/build-packages.sh -o <openwrt> -t <target/subtarget> -v <version> -O <out> [-d tarballs] [-s src]
  [--luci]`: downloads and checks the SDK (sha256sums of the target), builds, names the outputs
  `<name>_<v>-r1_<arch>.ipk|.apk` (apk files carry no architecture in the SDK's name; luci-app → `_all`).
- `.github/workflows/release.yml`: `build` uploads its tarballs as artifacts; new job `openwrt` (matrix: OpenWrt
  24.10.8 and 25.12.5 × the ten D-14 architectures, one SDK target each, `--luci` on x86/64) runs the script with the
  packaging files from the workflow's commit and the packaged files from the tag, checks the control data, and
  `gh release upload --clobber`. `workflow_dispatch` with input `tag` skips `build` and takes the tarballs from that
  release. actionlint 1.7.7 clean. **Not exercised on GitHub:** a dispatch needs the workflow on the default branch,
  and pushing is the maintainer's; the same script ran in the lab instead (below).

**Measured 2026-10-13** (lab SDK container, Debian 13, 8 cores, 4 GB): all 20 SDK runs green
(24.10.8 and 25.12.5 × x86/64, mediatek/filogic, bcm27xx/bcm2711, armsr/armv8, ipq40xx/generic, mvebu/cortexa9,
ipq806x/generic, sunxi/cortexa8, bcm53xx/generic, imx/cortexa9): 15–24 s each for `discowall` including the SDK
download, 141 s (24.10) and 104 s (25.12) for x86/64 with `--luci` (feeds and luci-base's build deps).
- Every `.ipk`: `Architecture:` = the SDK's arch (`all` for luci-app), `Depends: libc` (luci-app: `libc, luci-base,
  discowall`), conffiles `/etc/discowall/rules.toml` (`/etc/config/discowall` is not in the package, so ipkg-build
  drops it — as with the source-built package); the binary a static ELF without INTERP of the right class/machine,
  sha256 identical to the release tarball's (x86_64 `d34ebfb7…`, aarch64 `2fa379d6…`, armv7 `876d4d5e…`).
- Every `.apk` (the 25.12 SDK's own `apk adbdump`/`apk extract`): `arch` right (`noarch` for luci-app), `depends:
  libc` (luci-app: `discowall, libc, luci-base`), conffiles `/etc/config/discowall` and `/etc/discowall/rules.toml`,
  same binaries.
- bcm53xx's SDK `.config` has no `CONFIG_HAS_FPU` (ipq40xx: `=y`); the armv7 binary's ELF attributes say
  `Tag_FP_arch: VFPv3`, `Tag_ABI_VFP_args: VFP registers` — so `arm_cortex-a9` (D-14) is built and attached but is
  expected to SIGILL on FPU-less Northstar SoCs (BCM4708/4709); not tried on hardware. README says so.
- OpenWrt VM (24.10.8, lab): packages removed and config deleted first, then `lab.py run --ui --ipk
  discowall_0.0.2-r1_x86_64.ipk --ipk luci-app-discowall_0.0.2-r1_all.ipk openwrt` → **PASS openwrt 36/36 checks**
  (55.5 s) with exactly the attached files.
- opkg 2024.10.16 on that VM: a plain `opkg install` of a foreign-architecture ipk (aarch64_cortex-a53) crashed with a
  segmentation fault; with the router's own `--add-arch` entries plus `--add-arch aarch64_cortex-a53:5` it was
  accepted (`--noaction`). There is no `--force-architecture` in opkg; the README documents `--add-arch`. Not tried
  for apk.

**Attached to v0.0.2** (2026-10-13, `gh release upload`, built locally with the same script because the dispatch was
not possible): `luci-app-discowall_0.0.2-r1_all.{ipk,apk}` and `discowall_0.0.2-r1_<arch>.{ipk,apk}` for x86_64,
aarch64_cortex-a53, aarch64_cortex-a72, aarch64_generic, arm_cortex-a7_neon-vfpv4, arm_cortex-a8_vfpv3, arm_cortex-a9,
arm_cortex-a9_neon, arm_cortex-a9_vfpv3-d16, arm_cortex-a15_neon-vfpv4 — 22 files.

**Pending:** the `openwrt` job on GitHub (first real run: the next tag, or a dispatch with `tag: v0.0.2` after the
push — it re-uploads with `--clobber`); `.apk` on a 25.12 system (D-15: later lab session); a real router of any of
the architectures. The OpenWrt versions in the matrix (24.10.8, 25.12.5) are pinned and need a bump per OpenWrt
point release.

🕵️ 2026-10-13: assets on the release, x86_64 ipk verified in the lab (36/36); the workflow job waits for its first run.

