# End-to-end testing for discowall — research and a proposal

**Written** 2026-10-10 by Fable for the maintainer, who asked: *"do research and make suggestions for end2end
testing. i guess this will not work on github, but we could use my local proxmox environment and setup 3
containers, one router, 2 hosts in different vlans or smth. or is that too complicated? what would you suggest."*

## 0. In short

- **What we have already is more than most projects get:** the namespace test (`crates/discowalld/tests/netns.rs`)
  runs on every push on GitHub's runner with real multicast over real veth interfaces, on both address families,
  with rules driven through `discoctl`, the legacy-query proxy and SSDP end to end. It uses a small Python probe as
  the mDNS/SSDP peer. **What it cannot do:** talk to real stacks (avahi, Apple's mDNSResponder, Android, Sonos,
  Chromecast, HomeKit bridges), test the coexistence with avahi on one host (task 10), the systemd unit, the
  packages, routing across VLANs, or anything that needs a kernel the runner does not have (OpenWrt).
- **The Proxmox lab is the right next layer, and it is not complicated:** three Debian **LXC containers** on one
  VLAN-aware bridge — a router container with three interfaces (lan, iot, guest as VLAN tags) running discowalld
  from the `.deb`, and two "host" containers in two of the VLANs running **avahi-daemon** (publishing and browsing)
  plus a UPnP peer. Setting it up is an hour with the Proxmox UI or a `pct create` script; a test script of a
  few hundred lines drives it over SSH. It gives exactly what the namespace test lacks: real avahi on both ends,
  avahi beside discowalld on the router, the unit, the package, VLAN routing, long runs.
- **The third layer is the real network**, and no lab replaces it: an iPhone finding a HomeKit bridge, a Sonos
  app, a Chromecast, a printer. The proposal is a short **manual checklist** per release, run on the maintainer's
  own router with his devices, with `discoctl watch` as the instrument.
- **Recommendation:** build the Proxmox lab (section 3), drive it with a script in the repository
  (`tests/lab/`, reading a lab config that stays outside the repository, D-9), run it by hand before a release and
  on demand from a session; keep the namespace test as the gate on every push; add the manual checklist to the
  release procedure. Effort: half a day for the lab, one to two days for the script, an hour per release for the
  checklist.

## 1. What end-to-end means here, and what each layer can prove

| Layer | Peers | Proves | Cannot prove | Where it runs |
| --- | --- | --- | --- | --- |
| **Unit tests** (36) | fake sockets, canned packets | the chain, the table, the parsers, the rebuild, the proxy logic | anything about the kernel or real peers | everywhere, `cargo test` |
| **Namespace test** (the gate) | the Python probe | real multicast joins and `IP_PKTINFO`, both families, the daemon as a process, `discoctl` over the socket, the legacy and SSDP proxies against a compliant peer | real stacks, avahi beside us, VLANs, the unit, the packages | GitHub runner, this WSL, any Linux with `unshare -rn` |
| **Proxmox lab** (proposed) | avahi-daemon, a UPnP stack, optionally a Home Assistant or a Matter/HomeKit emulator | coexistence with avahi (task 10), the `.deb` and the unit, VLAN routing, interface up/down and reboots, hours of traffic, dedup against avahi's own reflector | consumer devices and their quirks | the maintainer's Proxmox, driven over SSH |
| **Real network** (checklist) | the maintainer's devices | that the product works for a person | reproducibility | the maintainer's router |

The namespace test will stay the thing that gates every push. The lab is for what it cannot reach, not a
replacement; a lab test that could be a namespace test should be a namespace test.

## 2. Options weighed

1. **GitHub-hosted, more of the same.** The runner allows `unshare -rn` and the namespace test uses it. One could
   install avahi-daemon inside leaf namespaces (avahi can run in a netns with its own D-Bus session) and so test
   against a real stack on every push. Worth doing for one case — avahi publishing in a leaf, avahi-browse in the
   other — because it is cheap and catches wire-format quirks of the most common stack. It still cannot test avahi
   *beside* discowalld on the router namespace (two daemons on one 5353 in one namespace is exactly the open
   question; it can be tried there too, actually: avahi in the router namespace with `enable-reflector=no`). This
   is an extension of the gate, not a lab. **Recommended as a small follow-up item.**
2. **A single Proxmox VM with namespaces.** Same as the runner, no gain except a kernel the maintainer controls.
   Not worth a VM.
3. **Three LXC containers on a VLAN-aware bridge** (the maintainer's idea). Cheap on Proxmox (Debian template, 512 MB
   each), fast to create and destroy, snapshot-able, and the router container needs no special privileges: port
   5353, multicast joins and netlink work unprivileged; the only thing to allow is multicast on the bridge (it is
   on by default) and, for the router container, three network interfaces with VLAN tags. Routing between the
   VLANs is the router container's job (`sysctl net.ipv4.ip_forward=1` plus nftables), which is also what makes the
   data-path tests possible (connect to the found service). **Recommended.**
4. **VMs instead of LXC.** Needed only for OpenWrt (its own kernel and init) or pfSense/FreeBSD (task 9). For the
   Linux router an LXC is enough and much lighter. Keep a VM in mind for the OpenWrt package once it exists.
5. **Real devices in the lab.** A spare Chromecast or a HomeKit bridge can be put into a lab VLAN on a real switch
   port; that is the maintainer's call and infrastructure, and it turns the lab into a hybrid. The checklist
   covers it without the infrastructure.

Complicated? No. The Proxmox part is three containers and a bridge setting. The work is in the test script and in
deciding what to assert; that is the same work the namespace test already did once, so most of it is a port.

## 3. The lab, concretely

### 3.1 Topology

```
Proxmox host, vmbr0 VLAN-aware (bridge-vlan-aware yes, bridge-vids 2-4094)

  CT 1 "router"   eth0 tag 10 (lan)   10.10.0.1/24  fd00:10::1/64
                  eth1 tag 20 (iot)   10.20.0.1/24  fd00:20::1/64
                  eth2 tag 30 (guest) 10.30.0.1/24  fd00:30::1/64
                  + eth3 untagged on the management network for SSH
                  discowalld (.deb), optionally avahi-daemon, nftables, ip_forward

  CT 2 "lanhost"  eth0 tag 10         10.10.0.20     avahi-daemon (browse), a UPnP control point, python probe
  CT 3 "iothost"  eth0 tag 20         10.20.0.30     avahi-daemon (publish), a UPnP device, python probe
```

A third host container in VLAN 30 (guest) is cheap and makes the "guest sees nothing" rule testable; without it
the router's guest interface is still there for "down interface" cases. If the maintainer's real VLANs 10/20/30
already exist on the Proxmox host's trunk, the lab can use fresh tags (e.g. 110/120/130) to stay isolated, or
real ones to let real devices in.

`pct create` for each with the Debian 13 template; the router gets `-net0 name=eth0,bridge=vmbr0,tag=10,ip=...`
and so on; `-features nesting=0`, unprivileged. No `lxc.cap` tweaks: nothing in discowall needs a capability.

### 3.2 Peers

- **avahi-daemon** on both hosts: `avahi-publish -s "Lab Printer" _ipp._tcp 631 txtvers=1` on iothost,
  `avahi-browse -rpt _ipp._tcp` (parsable output) on lanhost; `avahi-resolve-host-name iothost.local`. Also
  `avahi-daemon` on the **router** with `enable-reflector=no` for the coexistence test, and with
  `enable-reflector=yes` for the ping-pong/dedup test.
- **mDNS from other stacks** where cheap: `python3 -m zeroconf` (python-zeroconf's own stack, used by Home
  Assistant), `dns-sd` is macOS only; `mdns-scan`; `systemd-resolved` with `MulticastDNS=yes` as a third
  resolver stack.
- **SSDP:** `gssdp-discover` (gupnp-tools) as the control point, `upnpc -l` (miniupnpc) as a one-shot searcher,
  a tiny Python UPnP device (the probe's `ssdp-respond` plus a periodic alive) or `rygel`/`minidlna` as a real
  media server.
- **The probe** (`tests/mdns_probe.py`) stays useful for exact-bytes cases.

### 3.3 What the lab tests (the cases the gate cannot)

1. **Install and start** the `.deb` from the release (or the tarball): unit active, `discoctl status` shows three
   interfaces up and joined, journal clean.
2. **avahi end to end:** publish on iothost, browse on lanhost through the rules; resolve the host name; the
   data path (`curl http://iothost.local:631/` through the router's nftables) works.
3. **Coexistence (task 10):** avahi-daemon on the router beside discowalld — multicast reaches both (avahi still
   publishes `router.local`, discowall still forwards); unicast to 5353 reaches one (document which); then avahi's
   reflector on as well → discowall's dedup must keep the ping-pong down (`stats` duplicate counter, no storm).
4. **Legacy and QU through real stacks:** `dig @224.0.0.251 -p 5353 _ipp._tcp.local PTR` from lanhost;
   `avahi-browse` uses QU on its first query — check that iothost's avahi answers and lanhost sees it.
5. **SSDP with a real control point:** `gssdp-discover -n 3` on lanhost finds the device on iothost; a byebye
   removes it; `discoctl services` agrees.
6. **Interface dynamics:** `ip link set eth1 down/up` on the router, a container reboot, a VLAN removed and
   re-added — joins come back, `status` agrees, no stale state.
7. **Rules at scale:** a few hundred rules, 50 services announced in a loop, `watch` keeps up, memory stays flat
   over an hour (`systemctl status` RSS, `discoctl stats`).
8. **IPv6 only:** `ipv4 = false`, ULA addresses, the same browse.
9. **Upgrade:** install the previous `.deb` with rules, upgrade, rules and conffiles survive.

### 3.4 The harness

- `tests/lab/` in the repository: `lab.toml.example` (hosts, SSH users, interface names, VLAN tags), a `lab.py`
  (standard library plus `ssh`, no Ansible) with one function per case above, each idempotent, each printing a
  one-line verdict, `--only <case>`, `--keep` (do not clean up), JSON summary for the item file. The real
  `lab.toml` stays outside the repository (`~/.config/discowall-lab/`), as D-9 demands.
- Setup is a separate script (`lab-create.sh` with `pct` commands) run once on the Proxmox host; it is also
  the documentation of the topology.
- A session runs it with `python3 tests/lab/lab.py --release v0.0.1` and writes the verdicts into the item; the
  maintainer runs it before tagging. Not in CI: GitHub cannot reach the lab, and a self-hosted runner on the
  Proxmox host is possible later but not worth it now.

### 3.5 Effort

| | Days |
| --- | --- |
| Proxmox: bridge setting, three containers, avahi and tools installed, SSH keys, snapshots | 0.5 (the maintainer, or a session with `pct` access) |
| `lab.py` with the nine cases, `lab-create.sh`, README section | 1–2 |
| First run and the fixes it will find (the coexistence outcome goes into research 3.1 and task 10) | 0.5–1 |
| The manual checklist (section 4) | 0.2 |
| avahi inside the namespace test on GitHub (option 1) | 0.5 |

## 4. The manual checklist (per release, on the real router)

Five minutes with `discoctl watch` open: an iPhone's Home app sees the bridge and controls an accessory; AirPlay
from a phone to a speaker in the media VLAN; a Chromecast appears in YouTube; a printer appears in the print
dialog; the Sonos app finds the players; nothing from the guest VLAN appears anywhere; `discoctl stats` shows no
send errors and a sane dedup count. What breaks here and not in the lab is a device quirk, and that is what the
checklist is for.

## 5. Open for the maintainer

- Fresh VLAN tags for the lab or the real ones (and so real devices in the lab)?
- Does a session get SSH to the Proxmox host (`pct`) and to the containers, as in the openccu lab, or does the
  maintainer create the containers himself from `lab-create.sh`?
- OpenWrt: a VM in the same lab later, when the package exists?
