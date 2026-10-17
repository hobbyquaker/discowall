# The OpenWrt VM and the SDK container (task 13)

Two more guests beside the four LXC containers of [README.md](README.md): an OpenWrt x86-64 VM that forwards
between the three lab VLANs instead of the router container, with `discowall` and `luci-app-discowall`
installed from packages, and a Debian container with the OpenWrt SDK that builds those packages. `lab.py run
openwrt` tests the VM; `--ui` also drives the LuCI pages in a browser. Built on 2026-10-13 with OpenWrt
**24.10.8** (D-12 says the latest stable; see *Version* below).

## The SDK container (default id 904)

```sh
# on the node
pct create 904 <template-storage>:vztmpl/debian-13-standard_13.1-2_amd64.tar.zst --hostname discowall-sdk \
  --storage <storage> --rootfs <storage>:30 --memory 10240 --swap 4096 --cores 8 \
  --net0 name=eth0,bridge=vmbr0,ip=dhcp --unprivileged 1 --features nesting=1 --tags discowall-lab
pct start 904
# in the container
apt-get install -y build-essential clang flex bison g++ gawk gcc-multilib gettext git libncurses-dev \
  libssl-dev python3-setuptools rsync swig unzip zlib1g-dev file wget zstd python3 python3-dev curl
useradd -m build    # the SDK as an ordinary user
su - build
wget https://downloads.openwrt.org/releases/24.10.8/targets/x86/64/openwrt-sdk-24.10.8-x86-64_gcc-13.3.0_musl.Linux-x86_64.tar.zst
tar --zstd -xf openwrt-sdk-*.tar.zst && mv openwrt-sdk-*-x86_64 sdk && cd sdk
./scripts/feeds update base packages luci
./scripts/feeds install rust luci-base ucode lua libubox libjson-c ubus uci rpcd
cp -r <repo>/contrib/openwrt/discowall <repo>/contrib/openwrt/luci-app-discowall package/
make defconfig
make package/feeds/packages/rust/host/compile -j6     # rustc 1.90.0 and LLVM from source: the long part
make package/discowall/compile package/luci-app-discowall/compile -j6
ls bin/packages/x86_64/base/                          # discowall_0.0.3-r1_x86_64.ipk, luci-app-discowall_0.0.3-r1_all.ipk
```

Measured 2026-10-13: the Rust host build needs more than 4 GB of memory at `-j8` (cc1plus was OOM-killed while
building LLVM after about 45 minutes); with 10 GB at `-j6` the rest took 65 minutes (wall clock) and used 12 GB of
disk in all. `package/discowall/compile` then took 1 min 46 s. `luci-app-discowall` needs no Rust, but
`luci-base`'s build dependencies (`rpcd-mod-luci`, `lucihttp`, `ucode-mod-html`) need the `base` feed's `ucode`,
`lua` and friends installed — without them the build stops at `ucode/module.h: No such file or directory`. To build the current working tree instead of the release tag,
put a tarball of it where the Makefile's `PKG_SOURCE` points (`git archive --prefix=discowall-0.0.3/ -o
<sdk>/dl/discowall-0.0.3.tar.gz HEAD`; `PKG_HASH:=skip` accepts it).

### Packages from the release binaries (task 16)

What the release workflow does, by hand: `contrib/openwrt/build-packages.sh` downloads the SDK of one
target, packages the release's static binary as `discowall` (`contrib/openwrt/discowall-prebuilt`) and, with
`--luci`, builds `luci-app-discowall`. No Rust, no `packages` feed for `discowall`:

```sh
# as the build user, with the three release tarballs in tarballs/
contrib/openwrt/build-packages.sh -o 24.10.8 -t x86/64 -v 0.0.3 -O out -d tarballs --luci
contrib/openwrt/build-packages.sh -o 25.12.5 -t mediatek/filogic -v 0.0.3 -O out -d tarballs
```

Measured 2026-10-13 in the SDK container: about 20 s per architecture for `discowall` (SDK download included),
about 2.5 min with `--luci` (the `base`, `packages` and `luci` feeds and luci-base's build dependencies).
The `discowalld` in the package is byte-identical to the one in the release tarball (`STRIP:=:`).

## The OpenWrt VM (default id 905)

```sh
# on the node
wget https://downloads.openwrt.org/releases/24.10.8/targets/x86/64/openwrt-24.10.8-x86-64-generic-ext4-combined.img.gz
gunzip openwrt-24.10.8-x86-64-generic-ext4-combined.img.gz     # "trailing garbage ignored" is normal
qm create 905 --name discowall-openwrt --memory 512 --cores 2 --ostype l26 --tags discowall-lab \
  --net0 virtio,bridge=vmbr0,link_down=1 --net1 virtio,bridge=vmbr0,tag=110,link_down=1 \
  --net2 virtio,bridge=vmbr0,tag=120,link_down=1 --net3 virtio,bridge=vmbr0,tag=130,link_down=1 \
  --serial0 socket --vga serial0 --scsihw virtio-scsi-pci
qm importdisk 905 openwrt-24.10.8-x86-64-generic-ext4-combined.img <storage>
qm set 905 --scsi0 <storage>:vm-905-disk-0 --boot order=scsi0
qm start 905
```

**All links down at first boot:** OpenWrt starts with `eth0` in `br-lan` at 192.168.1.1 *with a DHCP
server*; on the untagged management network that would hand out leases to real machines. The VM is
configured over its serial console (`qm terminal 905`, or a script writing to
`/var/run/qemu-server/905.serial0`) before any link comes up:

```sh
passwd root                                   # the LuCI login; keep the password outside the repository
cat > /etc/dropbear/authorized_keys           # your SSH public key
uci set system.@system[0].hostname='discowall-openwrt'
uci set network.mgmt=interface; uci set network.mgmt.device='eth0'; uci set network.mgmt.proto='dhcp'
uci set network.@device[0].ports='eth1'       # br-lan = the lan VLAN
uci set network.lan.ipaddr='10.110.0.2'; uci -q delete network.lan.ip6assign
uci add_list network.lan.ip6addr='fd00:110::2/64'
uci -q delete network.wan; uci -q delete network.wan6; uci -q delete network.globals.ula_prefix
for n in iot:eth2:120 guest:eth3:130; do set -- $(echo $n | tr : ' ')
  uci set network.$1=interface; uci set network.$1.proto=static; uci set network.$1.device=$2
  uci set network.$1.ipaddr=10.$3.0.2; uci set network.$1.netmask=255.255.255.0
  uci add_list network.$1.ip6addr=fd00:$3::2/64; done
uci set dhcp.lan.ignore=1; uci set dhcp.lan.dhcpv4=disabled; uci set dhcp.lan.dhcpv6=disabled
uci set dhcp.lan.ra=disabled; uci -q delete dhcp.wan
uci add_list firewall.@zone[0].network='mgmt'  # management in the lan zone
for z in iot guest; do
  uci set firewall.$z=zone; uci set firewall.$z.name=$z; uci add_list firewall.$z.network=$z
  uci set firewall.$z.input=REJECT; uci set firewall.$z.output=ACCEPT; uci set firewall.$z.forward=REJECT
  for r in mdns:dest_port:5353 ssdp:dest_port:1900 mdns_answers:src_port:5353 ssdp_answers:src_port:1900; do
    set -- $(echo $r | tr : ' ')
    uci set firewall.${z}_$1=rule; uci set firewall.${z}_$1.name=Allow-$1-$z; uci set firewall.${z}_$1.src=$z
    uci set firewall.${z}_$1.proto=udp; uci set firewall.${z}_$1.$2=$3; uci set firewall.${z}_$1.target=ACCEPT
  done; done
uci commit; reload_config
```

then `qm set 905 --netN …` without `link_down=1` for each NIC. The `.2` addresses keep the VM clear of the
router container (`.1`); the case stops `discowalld` in the router container while it runs, so there is one
forwarder on the VLANs, and starts it again afterwards.

**Management address:** in this lab the network's DHCP server never answered the VM (eight DISCOVERs seen on
the bridge, no OFFER, 2026-10-13), so `mgmt` is `proto none` and the VM is reached over its IPv6 link-local
address on the node's bridge: SSH with the node as ProxyJump, HTTP with `curl` on the node (`%25<bridge>` as
the zone in the URL). That is what `[openwrt]` in `lab.toml` holds. Packages come onto the VM with
`lab.py run --ipk …`, not from the internet.

**The firewall rules matter:** with a zone's input at REJECT, the router drops the multicast discowalld
listens to unless 5353 and 1900 are accepted, and the proxies' unicast answers come back to an ephemeral
port: from source port 1900 (an SSDP device answering a proxied M-SEARCH) or 5353 (a responder answering a
proxied one-shot query). Measured 2026-10-13: without the `src_port 1900` rule every minidlna answer hit the
iot zone's reject counter (3 per search) and `gssdp-discover` on lan found nothing; with it, the device was
found.

## Running

```
python3 tests/lab/lab.py run --ipk discowall_0.0.3-r1_x86_64.ipk --ipk luci-app-discowall_0.0.3-r1_all.ipk openwrt
python3 tests/lab/lab.py run --ui openwrt          # the pages in headless Firefox as well (luci-ui.js)
python3 tests/lab/lab.py run --keep-openwrt openwrt   # leave the VM forwarding afterwards
```

The case checks: the packages install (opkg, or apk from OpenWrt 25.12 on); uci-defaults created
`/etc/config/discowall`; the init script rendered `discowall.toml` from UCI; the four views are served; the
rpcd object `luci.discowall` answers `status` over uhttpd's JSON-RPC (`/ubus`, after `session login`) with the
three interfaces up and joined; with an empty chain the lan host sees no printer (the control); four rules
added with `rule_add` as RuleSpec objects show in `discoctl rule list`, an insert at 1 and `rule_del 1`, a bad
rule comes back as `{"result": "error"}`; avahi on the lan host browses and resolves the iot printer through
the VM; `gssdp-discover` finds minidlna through the VM; `hosts` and `stats` over rpcd; `save` writes
`rules.toml`, `apply` reloads it; settings written as LuCI writes them (`uci set`, `uci apply`) re-render the
TOML and procd restarts the daemon; the saved chain is back after the restart. With `--ui`, `luci-ui.js`
logs in, finds the menu entries, reads the Status page, adds a rule through the modal form at position 1,
sees the unsaved banner, deletes the rule, saves, reads Hosts and services, and toggles SSDP on the Settings
page with Save & Apply (the daemon's uptime restarts), with no JavaScript error on the app's pages.

**Browser:** `luci-ui.js` needs `playwright-core` (any `node_modules` that has it, `ui_node_path`) and a
Firefox build (`ui_firefox`; playwright's own works). Headless Chromium under WSL never painted a frame —
LuCI stayed at "Loading view…" and screenshots timed out — Firefox worked (2026-10-13).

## Version

D-12 says "the latest stable OpenWrt". On 2026-10-13 downloads.openwrt.org lists **25.12.5** as the newest
release (25.12 uses `apk` instead of `opkg`); the maintainer's Q&A answer named 24.10, which is what this
lab was built and tested with. Moving the lab to 25.12 is the same procedure with the 25.12 SDK
(`gcc-14.3.0`) and image; the case installs with `apk add --allow-untrusted` when `/usr/bin/apk` exists.
