#!/usr/bin/env bash
# Builds the OpenWrt packages of a discowall release with the OpenWrt SDK (task 16): discowall for the
# SDK's package architecture from the release's prebuilt static binary (discowall-prebuilt/Makefile),
# and with --luci also luci-app-discowall (architecture-independent). OpenWrt 24.10 builds .ipk,
# 25.12 and later .apk. Used by the release workflow (one run per OpenWrt version and architecture)
# and by hand. Needs the SDK's host tools (tests/lab/openwrt.md lists the Debian packages); no Rust.
#
#   build-packages.sh -o 24.10.8 -t mediatek/filogic -v 0.0.2 -O out/ [-d tarballs/] [-s src/] [--luci]
#
#   -o  OpenWrt release whose SDK to use          -t  target/subtarget of that SDK (one per architecture)
#   -v  discowall version (the release tag without v)
#   -O  output directory: the packages land here, named <name>_<version>-r<release>_<arch>.ipk|.apk
#   -d  directory with discowall-<v>-<rust target>.tar.gz (else the SDK downloads it from the release)
#   -s  a checkout of the release's sources (init script, uci-defaults, luci-app-discowall); default:
#       this script's repository. The package Makefile always comes from this script's repository.
#   -w  work directory (default: a new one under /tmp); -c  cache for SDK downloads (default: <work>/dl)
#   --luci  build luci-app-discowall too (needs the base, packages and luci feeds: a few minutes more)
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
src=$(cd "$here/../.." && pwd)
ow= target= ver= out= tarballs= work= cache= luci=0
while [ $# -gt 0 ]; do
	case "$1" in
	-o) ow=$2; shift 2 ;;
	-t) target=$2; shift 2 ;;
	-v) ver=$2; shift 2 ;;
	-O) out=$2; shift 2 ;;
	-d) tarballs=$2; shift 2 ;;
	-s) src=$(cd "$2" && pwd); shift 2 ;;
	-w) work=$2; shift 2 ;;
	-c) cache=$2; shift 2 ;;
	--luci) luci=1; shift ;;
	*) echo "unknown argument: $1" >&2; exit 2 ;;
	esac
done
if [ -z "$ow" ] || [ -z "$target" ] || [ -z "$ver" ] || [ -z "$out" ]; then
	sed -n '2,17p' "$0" >&2
	exit 2
fi
mkdir -p "$out"; out=$(cd "$out" && pwd)
[ -n "$work" ] || work=$(mktemp -d /tmp/discowall-sdk.XXXXXX)
mkdir -p "$work"; work=$(cd "$work" && pwd)
[ -n "$cache" ] || cache=$work/dl
mkdir -p "$cache"

# The SDK: its file name from the target's sha256sums, downloaded once, checked, unpacked.
base=https://downloads.openwrt.org/releases/$ow/targets/$target
curl -fsSL "$base/sha256sums" -o "$work/sha256sums"
line=$(grep -E ' \*?openwrt-sdk-.*\.tar\.(zst|xz)$' "$work/sha256sums" | head -n 1)
[ -n "$line" ] || { echo "no SDK in $base" >&2; exit 1; }
sum=${line%% *}
file=${line##* }; file=${file#\*}
if ! echo "$sum  $cache/$file" | sha256sum -c --status 2>/dev/null; then
	curl -fsSL "$base/$file" -o "$cache/$file"
	echo "$sum  $cache/$file" | sha256sum -c --quiet
fi
sdk=$work/sdk
rm -rf "$sdk"; mkdir -p "$sdk"
tar -C "$sdk" --strip-components=1 -xf "$cache/$file"

# The packages: the Makefile from here, the files from the release's sources, the version from -v.
cp -rL "$here/discowall-prebuilt" "$sdk/package/discowall"
rm -rf "$sdk/package/discowall/files"
cp -r "$src/contrib/openwrt/discowall/files" "$sdk/package/discowall/files"
sed -i "s/^PKG_VERSION:=.*/PKG_VERSION:=$ver/" "$sdk/package/discowall/Makefile"
if [ "$luci" = 1 ]; then
	cp -r "$src/contrib/openwrt/luci-app-discowall" "$sdk/package/luci-app-discowall"
	sed -i "s/^PKG_VERSION:=.*/PKG_VERSION:=$ver/" "$sdk/package/luci-app-discowall/Makefile"
fi
mkdir -p "$sdk/dl"
if [ -n "$tarballs" ]; then
	cp "$tarballs"/discowall-"$ver"-*.tar.gz "$sdk/dl/"
fi

cd "$sdk"
if [ "$luci" = 1 ]; then
	./scripts/feeds update base packages luci
	./scripts/feeds install luci-base ucode lua libubox libjson-c ubus uci rpcd
fi
make defconfig >/dev/null
jobs=$(nproc)
make package/discowall/compile -j"$jobs" || make package/discowall/compile -j1 V=s
if [ "$luci" = 1 ]; then
	make package/luci-app-discowall/compile -j"$jobs" || make package/luci-app-discowall/compile -j1 V=s
fi

# Collect, with the architecture in every name (apk names carry none of their own).
n=0
while IFS= read -r f; do
	b=$(basename "$f")
	arch=$(basename "$(dirname "$(dirname "$f")")")
	case "$b" in
	*.ipk) name=$b ;;
	*.apk)
		pkg=${b%.apk}                       # discowall-0.0.2-r1
		v=${pkg#"${pkg%-*-r*}"}; v=${v#-}   # 0.0.2-r1
		p=${pkg%-"$v"}                      # discowall
		case "$p" in luci-app-*) arch=all ;; esac
		name=${p}_${v}_${arch}.apk ;;
	esac
	case "$name" in luci-app-*_all.*) ;; luci-app-*) name=${name%_*}_all.${name##*.} ;; esac
	cp "$f" "$out/$name"
	echo "$out/$name"
	n=$((n + 1))
done < <(find bin/packages -type f \( -name 'discowall[_-]*.ipk' -o -name 'discowall-*.apk' \
	-o -name 'luci-app-discowall[_-]*.ipk' -o -name 'luci-app-discowall-*.apk' \))
want=1; [ "$luci" = 1 ] && want=2
if [ "$n" -lt "$want" ]; then
	echo "expected $want packages, found $n (is the architecture covered? see discowall-prebuilt/Makefile)" >&2
	exit 1
fi
