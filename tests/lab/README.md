# The end-to-end lab

What the namespace test on every push cannot reach — real avahi on both ends, avahi beside discowalld on the
router, the `.deb` and the systemd unit, VLANs, a real UPnP device and control point, reboots, hours of traffic —
runs here: four unprivileged Debian LXC containers on one VLAN-aware bridge of a Proxmox VE node, driven from a
workstation by `lab.py`. The research behind it is
[roadmap-items/discowall/notes/e2e-testing-research.md](../../roadmap-items/discowall/notes/e2e-testing-research.md);
the lab is not part of CI and never will be — it is run by hand before a release and on demand.

## Topology

```
Proxmox node, bridge vmbr0 with bridge-vlan-aware yes

  router      eth0 untagged, DHCP (management: apt, pct push)
              lan   tag 110  10.110.0.1/24  fd00:110::1/64     discowalld (from the .deb), avahi only in the
              iot   tag 120  10.120.0.1/24  fd00:120::1/64     coexistence case, ip_forward on
              guest tag 130  10.130.0.1/24  fd00:130::1/64
  lanhost     eth0 DHCP; lan   tag 110  .20   avahi-daemon (browses), gssdp-discover, python-zeroconf, the probe
  iothost     eth0 DHCP; iot   tag 120  .30   avahi-daemon (publishes "Lab Printer" _ipp._tcp), minidlna (UPnP)
  guesthost   eth0 DHCP; guest tag 130  .40   avahi-daemon — must see nothing
```

The tags are fresh ones, so nothing real is reached and no real device sees lab traffic. avahi is confined to the
lab interface in every container (`allow-interfaces`, `deny-interfaces=eth0`), multicast is routed to the lab
interface (`224.0.0.0/4 dev <lab-if>`), so nothing leaks into the management network. The hosts route the other
lab networks through the router, which makes the data path (HTTP from lan to the iot host) testable.

## Setting it up

1. On the node, as root: copy `lab-create.sh` there and run `sh lab-create.sh create`. Defaults: ids 900–903, the
   Debian 13 template, storage `local-zfs`, tags 110/120/130, 512 MB each; every one is an environment variable
   (`ROUTER=950 TAG_LAN=210 … sh lab-create.sh create`). The script touches only its own ids and refuses to destroy
   a container whose hostname is not its own. `sh lab-create.sh destroy` removes the lab.
2. On the workstation: `cp lab.toml.example ~/.config/discowall-lab/lab.toml`, set `node` (an SSH destination
   with key access to the node as root) and the ids and tags if they differ. The file stays outside the
   repository; nothing lab-specific is committed.
3. `python3 tests/lab/lab.py provision` installs the peers (avahi, gupnp-tools, minidlna, python-zeroconf, …),
   writes their configuration and pushes the probe. Idempotent.

## Running

```
python3 tests/lab/lab.py list                              the cases
python3 tests/lab/lab.py run --deb discowall_0.0.1_amd64.deb      everything, installing that package first
python3 tests/lab/lab.py run avahi legacy                  two cases against what is installed
python3 tests/lab/lab.py run --soak 3600 scale             the hour-long soak
python3 tests/lab/lab.py run --prev-deb old.deb --deb new.deb upgrade
python3 tests/lab/lab.py -v run …                          every command and every check printed
python3 tests/lab/lab.py run --json out.json …             the checks as JSON for the item file
```

One line per case, `PASS`/`FAIL`/`SKIP` with the count of checks; a failing check is printed under it with its
detail. The exit code is 1 if any case failed. Each case sets the rule chain it needs and leaves the lab usable for
the next; the `scale` case ends with `discoctl save`, the `ipv6only` case restores `ipv4 = true`.

| Case | Proves |
| --- | --- |
| `install` | the `.deb` installs, the unit is active, three interfaces up and joined on both families, journal clean and plain text, `systemd-analyze security` exposure ≤ 3 |
| `avahi` | avahi publishes on iot, avahi browses and resolves on lan over IPv4 and IPv6 (cache flushed first, so the forwarding is what is tested), TXT intact, the guest sees nothing, HTTP across the router |
| `coexistence` | task 10: avahi-daemon beside discowalld on one host — multicast reaches both; unicast to 5353 reaches the socket bound last; avahi's own reflector on as well causes no storm |
| `legacy` | a one-shot query from an ephemeral port is proxied and answered by unicast (the probe, `dig @router`); `dig @224.0.0.251` gets the answer on the wire but discards it itself; a fresh browse (QU) resolves |
| `ssdp` | gssdp-discover on lan finds minidlna on iot through the search proxy; the guest finds nothing; `ssdp:byebye` and `ssdp:alive` are forwarded |
| `dynamics` | link down/up, the iot host rebooted, the router's iot interface hot-unplugged and re-added — joins come back, browsing works |
| `scale` | 306 rules, 50 services from python-zeroconf, a soak of `--soak` seconds with a browse every 10 s: memory flat, no send errors |
| `ipv6only` | `ipv4 = false`: joined on v6 only, the browse and the resolve over the ULA addresses, nothing over IPv4 |
| `upgrade` | the previous package with saved rules, then the new one: rules and conffiles survive, the unit is active |

## Writing a case

A case is a function `case_<name>(lab, args)` returning a list of `Check(name, ok, detail)`; add it to `CASES`.
`lab.sh(who, script)` runs a shell script in a container (`who` is a role from `lab.toml`), `lab.out` returns its
stdout, `lab.bg_start`/`lab.bg_stop` run something in the background as a transient systemd unit and return its
output, `lab.ctl`/`lab.ctl_json` call `discoctl` on the router, `lab.set_rules` replaces the chain, `browse`
flushes a host's avahi cache and browses. A case that could be a namespace test should be a namespace test.
