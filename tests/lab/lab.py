#!/usr/bin/env python3
"""End-to-end tests for discowall in the Proxmox lab (tests/lab/README.md). Standard library only.

The lab is four LXC containers on one VLAN-aware bridge, built by lab-create.sh on the Proxmox node: a
router (discowalld between the interfaces lan, iot and guest) and one host per network running avahi-daemon,
the iot host also minidlna as a real UPnP device. This script runs on a workstation, reaches the node over
SSH and the containers with `pct exec`; the containers need no SSH of their own. The lab's addresses, ids and
the node's name live outside the repository in lab.toml (lab.toml.example is the shape).

  lab.py [--config lab.toml] provision             install the peers in freshly created containers
  lab.py [--config lab.toml] run [--deb X.deb] [--prev-deb Y.deb] [--soak N] [--json out.json] [CASE ...]
  lab.py list                                      the cases

Every case leaves the lab in a state the next case can use; a failing check does not stop the case. The
verdict is one line per case; `--json` writes the checks for the item file.
"""
import argparse
import json
import os
import re
import shlex
import subprocess
import sys
import time

try:
    import tomllib
except ImportError:  # Python < 3.11
    tomllib = None


class Check:
    def __init__(self, name, ok, detail=""):
        self.name, self.ok = name, bool(ok)
        self.detail = " | ".join(l.strip() for l in str(detail).splitlines() if l.strip())

    def as_dict(self):
        return {"check": self.name, "ok": self.ok, "detail": self.detail}


class Lab:
    """SSH to the node, `pct exec` into the containers, a few helpers around discoctl and systemd-run."""

    def __init__(self, cfg, verbose=False):
        self.cfg = cfg
        self.node = cfg["node"]
        self.ct = cfg["containers"]
        self.nets = cfg["networks"]
        self.verbose = verbose
        self.tmp_pushed = set()

    # --- plumbing ---------------------------------------------------------------------------------
    def node_sh(self, cmd, timeout=120, check=False):
        if self.verbose:
            print(f"    node$ {cmd[:160]}", file=sys.stderr)
        p = subprocess.run(["ssh", "-o", "BatchMode=yes", self.node, cmd], capture_output=True, text=True,
                           timeout=timeout)
        if check and p.returncode != 0:
            raise RuntimeError(f"node: {cmd}\n{p.stderr}")
        return p

    def sh(self, who, script, timeout=120, check=False):
        """Run a shell script inside container `who` (a role name or an id); returns CompletedProcess."""
        cid = self.ct.get(who, who)
        if self.verbose:
            print(f"    {who}$ {script[:160]}", file=sys.stderr)
        p = self.node_sh(f"pct exec {cid} -- sh -c {shlex.quote(script)}", timeout=timeout)
        if check and p.returncode != 0:
            raise RuntimeError(f"{who}: {script}\n{p.stdout}{p.stderr}")
        return p

    def out(self, who, script, timeout=120):
        return self.sh(who, script, timeout=timeout).stdout

    def push(self, who, local, remote):
        """Copy a local file into a container (through the node's /root)."""
        cid = self.ct.get(who, who)
        staged = f"/root/discowall-lab-{os.path.basename(local)}"
        if local not in self.tmp_pushed:
            subprocess.run(["scp", "-q", "-o", "BatchMode=yes", local, f"{self.node}:{staged}"], check=True)
            self.tmp_pushed.add(local)
        self.node_sh(f"pct push {cid} {staged} {shlex.quote(remote)}", check=True)

    def bg_start(self, who, unit, cmd):
        """Start `cmd` in the container as a transient unit; its stdout goes to /tmp/lab-<unit>.out."""
        self.sh(who, f"systemctl stop lab-{unit} 2>/dev/null; rm -f /tmp/lab-{unit}.out; "
                     f"systemd-run -q --unit lab-{unit} -p StandardOutput=file:/tmp/lab-{unit}.out {cmd}")

    def bg_stop(self, who, unit):
        return self.out(who, f"systemctl stop lab-{unit} 2>/dev/null; cat /tmp/lab-{unit}.out 2>/dev/null")

    # --- the lab's shape ----------------------------------------------------------------------------
    def host_of(self, net):
        return self.nets[net]["host"]

    def addr(self, net, v6=False):
        return self.nets[net]["host_addr6" if v6 else "host_addr"]

    def router_addr(self, net, v6=False):
        tag = self.nets[net]["tag"]
        return f"fd00:{tag}::1" if v6 else f"10.{tag}.0.1"

    def hostname(self, who):
        return self.out(who, "hostname").strip()

    # --- discowalld on the router -------------------------------------------------------------------
    def ctl(self, *args, timeout=60):
        p = self.sh("router", "discoctl " + " ".join(shlex.quote(a) for a in args), timeout=timeout)
        if p.returncode != 0:
            raise RuntimeError(f"discoctl {args}: {p.stdout}{p.stderr}")
        return p.stdout

    def ctl_json(self, *args):
        return json.loads(self.sh("router", "discoctl --json " + " ".join(shlex.quote(a) for a in args),
                                  check=True).stdout)

    def counters(self):
        return self.ctl_json("status")["counters"]

    def set_rules(self, rules):
        """Replace the chain: flush, add each rule (a string in discoctl's syntax), keep default drop."""
        script = "discoctl rule flush && " + " && ".join(f"discoctl rule add {r}" for r in rules)
        self.sh("router", script, check=True)

    def set_config(self, **overrides):
        """Rewrite /etc/discowall/discowall.toml from the shipped example with the lab's interfaces."""
        lines = ['interfaces = ["lan", "iot", "guest"]']
        for k, v in overrides.items():
            lines.append(f"{k} = {json.dumps(v)}")
        content = "\n".join(lines) + "\n"
        self.sh("router", f"printf %s {shlex.quote(content)} > /etc/discowall/discowall.toml", check=True)

    def restart_daemon(self):
        self.sh("router", "systemctl restart discowalld", check=True)
        for _ in range(50):
            if self.sh("router", "discoctl ping >/dev/null 2>&1").returncode == 0:
                break
            time.sleep(0.1)
        time.sleep(0.5)  # the interface dump and the joins

    def flush_avahi_cache(self, who):
        self.sh(who, "systemctl restart avahi-daemon", check=True)
        time.sleep(1.0)

    # --- lab state every run needs ------------------------------------------------------------------
    def ensure_routes(self):
        """Multicast and the other lab networks go out the lab interface, never the management one."""
        for net, n in self.nets.items():
            tag = n["tag"]
            others = " ".join(f"ip route replace 10.{o['tag']}.0.0/24 via 10.{tag}.0.1 dev {net};"
                              for o in self.nets.values() if o is not n)
            self.sh(n["host"], f"ip route replace 224.0.0.0/4 dev {net}; {others}", check=True)
        self.sh("router", "sysctl -qw net.ipv4.ip_forward=1 net.ipv6.conf.all.forwarding=1", check=True)

    def ensure_printer(self):
        """`Lab Printer` (_ipp._tcp on 631) published by avahi on the iot host."""
        if self.out("iothost", "systemctl is-active lab-printer").strip() != "active":
            self.bg_start("iothost", "printer", 'avahi-publish -s "Lab Printer" _ipp._tcp 631 txtvers=1 rp=ipp/print')
            time.sleep(1.5)

    def ensure_minidlna(self):
        if self.out("iothost", "systemctl is-active minidlna").strip() != "active":
            self.sh("iothost", "systemctl start minidlna", check=True)
            time.sleep(2)


# --- provisioning ------------------------------------------------------------------------------------
PROBE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "crates", "discowalld", "tests",
                     "mdns_probe.py")
COMMON_PKGS = "avahi-daemon avahi-utils python3 tcpdump curl dnsutils procps"


def provision(lab):
    """Install and configure the peers; idempotent. Run once after lab-create.sh, again at will."""
    pkgs = {
        "router": COMMON_PKGS + " nftables",
        "lanhost": COMMON_PKGS + " gupnp-tools miniupnpc python3-zeroconf",
        "iothost": COMMON_PKGS + " minidlna python3-zeroconf",
        "guesthost": COMMON_PKGS,
    }
    for who, p in pkgs.items():
        print(f"  {who}: apt-get install {p.split()[0]} …")
        lab.sh(who, "export DEBIAN_FRONTEND=noninteractive; apt-get -qq update && "
                    f"apt-get -qq -y -o Dpkg::Options::=--force-confold install {p} >/tmp/apt.log 2>&1",
               timeout=900, check=True)
    allow = {"router": "lan,iot,guest"}
    for net, n in lab.nets.items():
        allow[n["host"]] = net
    for who, ifaces in allow.items():
        lab.sh(who, "sed -i -e 's/^#*allow-interfaces=.*/allow-interfaces=%s/' "
                    "-e 's/^#*deny-interfaces=.*/deny-interfaces=eth0/' "
                    "-e 's/^#*enable-reflector=.*/enable-reflector=no/' "
                    "-e 's/^#*disallow-other-stacks=.*/disallow-other-stacks=no/' "
                    "/etc/avahi/avahi-daemon.conf" % ifaces, check=True)
        if who == "router":
            # avahi on the router runs only in the coexistence case.
            lab.sh(who, "systemctl disable --now avahi-daemon.socket avahi-daemon.service 2>/dev/null; true")
        else:
            lab.sh(who, "systemctl enable avahi-daemon.socket avahi-daemon.service >/dev/null 2>&1; "
                        "systemctl restart avahi-daemon", check=True)
    lab.sh("iothost", "sed -i -e 's/^#*network_interface=.*/network_interface=iot/' "
                      "-e 's/^#*friendly_name=.*/friendly_name=Lab DLNA/' "
                      "-e 's/^#*notify_interval=.*/notify_interval=30/' /etc/minidlna.conf; "
                      "systemctl enable minidlna >/dev/null 2>&1; systemctl restart minidlna", check=True)
    for who in ("lanhost", "iothost", "guesthost"):
        lab.push(who, PROBE, "/root/mdns_probe.py")
    lab.ensure_routes()
    print("  provisioned")


# --- the cases ---------------------------------------------------------------------------------------
PRINTER_RULES = [
    "in lan out iot kind query service _ipp._tcp accept",
    "in iot out lan kind response,announce service _ipp._tcp accept",
    "in lan out iot kind query rrtype A,AAAA accept",
    "in iot out lan kind response host discowall-iothost accept",
]
SSDP_RULES = [
    "proto ssdp in lan out iot kind query accept",
    "proto ssdp in iot out lan kind announce,response service urn:schemas-upnp-org:device:MediaServer:1,upnp:rootdevice accept",
]


def install_deb(lab, deb):
    lab.push("router", deb, "/root/" + os.path.basename(deb))
    p = lab.sh("router", "export DEBIAN_FRONTEND=noninteractive; "
                         f"apt-get -qq -y -o Dpkg::Options::=--force-confold install /root/{os.path.basename(deb)}",
               timeout=300)
    return p


def case_install(lab, args):
    """1. Install from the .deb: unit active, three interfaces up and joined, journal clean."""
    checks = []
    if args.deb:
        p = install_deb(lab, args.deb)
        checks.append(Check("apt-get install .deb", p.returncode == 0, (p.stdout + p.stderr).strip()[-300:]))
        lab.set_config()
        lab.sh("router", "systemctl enable discowalld >/dev/null 2>&1; true")
        lab.sh("router", "journalctl --rotate --vacuum-time=1s >/dev/null 2>&1; true")
        lab.restart_daemon()
    else:
        checks.append(Check("apt-get install .deb", True, "no --deb given: the installed version is tested"))
    checks.append(Check("package installed", "ii  discowall" in lab.out("router", "dpkg -l discowall | tail -1")))
    checks.append(Check("unit active", lab.out("router", "systemctl is-active discowalld").strip() == "active"))
    st = lab.ctl_json("status")
    names = {i["name"] for i in st["interfaces"]}
    checks.append(Check("three interfaces", names == {"lan", "iot", "guest"}, str(sorted(names))))
    checks.append(Check("all up and joined v4+v6", all(i["up"] and i["joined_v4"] and i["joined_v6"]
                                                       for i in st["interfaces"]),
                        json.dumps([(i["name"], i["up"], i["joined_v4"], i["joined_v6"]) for i in st["interfaces"]])))
    checks.append(Check("no send errors", st["counters"]["send_errors"] == 0))
    # Only the lines of the running process: older ones may be from the version installed before.
    journal = lab.out("router", "journalctl -u discowalld -o cat --no-pager "
                                "_PID=$(systemctl show -p MainPID --value discowalld) | tail -50")
    checks.append(Check("journal without WARN/ERROR", not re.search(r"\b(WARN|ERROR)\b", journal),
                        journal[-300:]))
    checks.append(Check("journal plain text (no ANSI codes, B-1)", "\x1b[" not in journal))
    sec = lab.out("router", "systemd-analyze security discowalld --no-pager 2>/dev/null | tail -1").strip()
    m = re.search(r"([0-9.]+) ", sec)
    checks.append(Check("systemd-analyze security exposure <= 3", bool(m) and float(m.group(1)) <= 3.0, sec))
    checks.append(Check("version", True, st["version"]))
    return checks


def browse(lab, who, net, service="_ipp._tcp", timeout=8):
    """avahi-browse -rpt on a host, after a cache flush: the lines for its lab interface."""
    lab.flush_avahi_cache(who)
    out = lab.out(who, f"timeout {timeout} avahi-browse -rpt {service}", timeout=timeout + 10)
    return [l for l in out.splitlines() if f";{net};" in l]


def case_avahi(lab, args):
    """2. avahi end to end: publish on iot, browse and resolve from lan, guest sees nothing, data path."""
    lab.ensure_printer()
    lab.ensure_minidlna()
    lab.set_rules(PRINTER_RULES)
    iot = lab.addr("iot")
    iot6 = lab.addr("iot", v6=True)
    checks = []
    lines = browse(lab, "lanhost", "lan")
    resolved = [l for l in lines if l.startswith("=;lan;")]
    checks.append(Check("lan browses the printer over IPv4", any(f";IPv4;Lab\\032Printer;" in l and f";{iot};631;" in l
                                                                for l in resolved), "\n".join(lines)[-400:]))
    checks.append(Check("lan browses the printer over IPv6", any(f";IPv6;Lab\\032Printer;" in l and f";{iot6};631;" in l
                                                                for l in resolved), "\n".join(lines)[-400:]))
    checks.append(Check("TXT records intact", any('"rp=ipp/print" "txtvers=1"' in l for l in resolved)))
    r = lab.out("lanhost", "avahi-resolve-host-name -4 discowall-iothost.local").strip()
    checks.append(Check("lan resolves iothost.local", r.endswith(iot), r))
    r6 = lab.out("lanhost", "avahi-resolve-host-name -6 discowall-iothost.local").strip()
    checks.append(Check("lan resolves iothost.local (AAAA)", r6.endswith(iot6), r6))
    glines = browse(lab, "guesthost", "guest", timeout=5)
    checks.append(Check("guest sees nothing", not glines, "\n".join(glines)[-300:]))
    code = lab.out("lanhost", f"curl -s -o /dev/null -w '%{{http_code}}' --max-time 5 http://{iot}:8200/rootDesc.xml")
    checks.append(Check("data path lan -> iot through the router (HTTP)", code.strip() == "200", code))
    hosts = lab.ctl_json("hosts")["hosts"]
    checks.append(Check("host table knows iothost", any(h["host"] == "discowall-iothost.local" and h["iface"] == "iot"
                                                        for h in hosts)))
    return checks


def unicast_query(lab, who, dst, name, from_5353):
    """Send one unicast A query for `name` to dst:5353 from `who`; returns the reply source or 'no reply'."""
    bind = f's.bind(("{lab.addr("lan")}", 5353))' if from_5353 else ""
    script = f"""python3 - <<'EOF'
import socket, struct
def n(x):
    o=b""
    for l in x.split("."): o+=bytes([len(l)])+l.encode()
    return o+b"\\x00"
q=struct.pack("!HHHHHH",0,0,1,0,0,0)+n("{name}")+struct.pack("!HH",1,1)
s=socket.socket(socket.AF_INET,socket.SOCK_DGRAM); s.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1); s.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEPORT,1)
{bind}
s.sendto(q,("{dst}",5353)); s.settimeout(2)
try:
    d,src=s.recvfrom(9000); print("reply from %s:%d" % src)
except socket.timeout: print("no reply")
EOF"""
    return lab.out(who, script).strip()


def case_coexistence(lab, args):
    """3. avahi-daemon beside discowalld on the router (task 10): multicast to both, unicast to one, reflector."""
    lab.ensure_printer()
    lab.set_rules(PRINTER_RULES)
    rtr = lab.router_addr("lan")
    checks = []
    lab.sh("router", "sed -i 's/^enable-reflector=.*/enable-reflector=no/' /etc/avahi/avahi-daemon.conf; "
                     "systemctl start avahi-daemon.socket avahi-daemon.service", check=True)
    time.sleep(2)
    checks.append(Check("avahi and discowalld both active", lab.out("router", "systemctl is-active avahi-daemon discowalld")
                        .split() == ["active", "active"]))
    sockets = lab.out("router", "ss -lunpH | grep ':5353 ' | grep -o 'avahi-daemon\\|discowalld' | sort | uniq -c")
    checks.append(Check("both hold 0.0.0.0:5353 and [::]:5353", "2 avahi-daemon" in sockets and "2 discowalld" in sockets,
                        sockets.strip()))
    # (1) multicast: a query from lan is answered by avahi on the router (it got the multicast) and the
    # printer still browses through discowall (it got the multicast too).
    r = lab.out("lanhost", "avahi-resolve-host-name -4 discowall-router.local").strip()
    checks.append(Check("multicast reaches avahi (router.local resolves)", r.endswith(rtr), r))
    lines = browse(lab, "lanhost", "lan")
    checks.append(Check("multicast reaches discowalld (printer still browses)",
                        any(l.startswith("=;lan;IPv4;") for l in lines), "\n".join(lines)[-300:]))
    # (2) unicast to port 5353: whoever bound last gets it. discowalld bound last …
    lab.restart_daemon()
    lab.bg_start("router", "watch", "discoctl --json watch --iface lan")
    time.sleep(1)
    a = unicast_query(lab, "lanhost", rtr, "discowall-router.local", from_5353=False)
    b = unicast_query(lab, "lanhost", rtr, "discowall-router.local", from_5353=True)
    time.sleep(1)
    watched = [json.loads(l) for l in lab.bg_stop("router", "watch").splitlines() if l.strip()]
    seen = [w for w in watched if "discowall-router.local" in (w.get("hosts") or [])]
    checks.append(Check("discowalld bound last: unicast (ephemeral and 5353 source) reaches discowalld, not avahi",
                        a == "no reply" and b == "no reply" and len(seen) >= 2,
                        f"ephemeral: {a}; from 5353: {b}; watch saw {len(seen)} queries for router.local"))
    # … avahi bound last.
    lab.sh("router", "systemctl restart avahi-daemon", check=True)
    time.sleep(2)
    lab.bg_start("router", "watch", "discoctl --json watch --iface lan")
    time.sleep(1)
    a = unicast_query(lab, "lanhost", rtr, "discowall-router.local", from_5353=False)
    time.sleep(1)
    watched = [json.loads(l) for l in lab.bg_stop("router", "watch").splitlines() if l.strip()]
    seen = [w for w in watched if "discowall-router.local" in (w.get("hosts") or [])]
    checks.append(Check("avahi bound last: unicast reaches avahi (answers), not discowalld",
                        a.startswith(f"reply from {rtr}:5353") and not seen,
                        f"ephemeral: {a}; watch saw {len(seen)} queries for router.local"))
    # (3) avahi's own reflector on as well: no storm, duplicates and own-source drops absorb it.
    lab.sh("router", "sed -i 's/^enable-reflector=.*/enable-reflector=yes/' /etc/avahi/avahi-daemon.conf; "
                     "systemctl restart avahi-daemon", check=True)
    time.sleep(2)
    before = lab.counters()
    lab.bg_start("lanhost", "tcpdump", "tcpdump -i lan -n -l udp port 5353")
    time.sleep(1)
    lab.sh("iothost", "systemctl restart lab-printer")  # a fresh announcement
    lab.flush_avahi_cache("lanhost")
    lab.out("lanhost", "timeout 5 avahi-browse -pt _ipp._tcp", timeout=15)
    time.sleep(5)
    dump = lab.bg_stop("lanhost", "tcpdump")
    pkts = len([l for l in dump.splitlines() if ".5353" in l])
    after = lab.counters()
    checks.append(Check("avahi reflector on: no storm (< 150 packets on lan in ~12 s)", pkts < 150, f"{pkts} packets"))
    checks.append(Check("own-source and duplicate drops absorb the second reflector",
                        after["dropped_own_source"] > before["dropped_own_source"],
                        f"own_source {before['dropped_own_source']} -> {after['dropped_own_source']}, "
                        f"duplicate {before['dropped_duplicate']} -> {after['dropped_duplicate']}"))
    lines = browse(lab, "lanhost", "lan")
    checks.append(Check("printer still browses with both reflectors", any(l.startswith("=;lan;IPv4;") for l in lines)))
    # Back to the normal lab: avahi off on the router.
    lab.sh("router", "sed -i 's/^enable-reflector=.*/enable-reflector=no/' /etc/avahi/avahi-daemon.conf; "
                     "systemctl stop avahi-daemon.socket avahi-daemon.service", check=True)
    lab.restart_daemon()
    return checks


def case_legacy(lab, args):
    """4. Legacy one-shot queries and QU through real stacks: avahi answers the proxy, dig and the probe ask."""
    lab.ensure_printer()
    lab.set_rules(PRINTER_RULES)
    rtr = lab.router_addr("lan")
    iot = lab.addr("iot")
    checks = []
    before = lab.counters()
    got = lab.out("lanhost", f"python3 /root/mdns_probe.py legacy {lab.addr('lan')} _ipp._tcp.local 3").strip()
    checks.append(Check("probe: multicast one-shot query answered by unicast from the router",
                        got.startswith(f"{rtr}:5353 response _ipp._tcp.local") and "discowall-iothost.local/1" in got
                        and "/33" in got and "/16" in got, got))
    after = lab.counters()
    checks.append(Check("proxied_queries and proxied_answers moved",
                        after["proxied_queries"] > before["proxied_queries"]
                        and after["proxied_answers"] > before["proxied_answers"],
                        f"{before['proxied_queries']}/{before['proxied_answers']} -> "
                        f"{after['proxied_queries']}/{after['proxied_answers']}"))
    # dig to the group discards an answer that does not come from 224.0.0.251 (measured: the answer is on
    # the wire within 2 ms); dig to the router's own address is a unicast legacy query the daemon proxies.
    d = lab.out("lanhost", f"dig +short +time=3 +tries=1 @{rtr} -p 5353 _ipp._tcp.local PTR 2>&1").strip()
    checks.append(Check("dig @router -p 5353 (unicast legacy query) gets the printer",
                        "Printer._ipp._tcp.local." in d, d))  # dig prints the space as \032
    d = lab.out("lanhost", f"dig +short +time=3 +tries=1 @{rtr} -p 5353 discowall-iothost.local A 2>&1").strip()
    checks.append(Check("dig @router A iothost.local", d == iot, d))
    lab.bg_start("lanhost", "tcpdump", "tcpdump -i lan -n -l udp port 5353")
    time.sleep(1)
    lab.out("lanhost", "dig +short +time=2 +tries=1 @224.0.0.251 -p 5353 _ipp._tcp.local PTR >/dev/null 2>&1; true")
    time.sleep(1)
    dump = lab.bg_stop("lanhost", "tcpdump")
    answered = any(f"{rtr}.5353 >" in l and "Lab Printer" in l for l in dump.splitlines())
    checks.append(Check("dig @224.0.0.251: the proxy's unicast answer is on the wire (dig itself discards it)",
                        answered, "\n".join(dump.splitlines()[-4:])[-400:]))
    # QU: avahi-browse's first query after a cache flush carries the QU bit; case 2 already proved it is
    # answered. Here: the first browse after a flush completes within the avahi default of ~2 s.
    t = time.time()
    lines = browse(lab, "lanhost", "lan", timeout=6)
    checks.append(Check("QU: fresh avahi-browse resolves the printer", any(l.startswith("=;lan;IPv4;") for l in lines),
                        f"{time.time() - t:.1f} s including the flush"))
    return checks


def case_ssdp(lab, args):
    """5. SSDP with a real device (minidlna) and a real control point (gssdp-discover)."""
    lab.ensure_minidlna()
    lab.set_rules(PRINTER_RULES + SSDP_RULES)
    iot = lab.addr("iot")
    checks = []
    out = lab.out("lanhost", "timeout 12 gssdp-discover -i lan -n 4 -t upnp:rootdevice 2>&1", timeout=20)
    checks.append(Check("gssdp-discover on lan finds minidlna (search proxied, answer relayed)",
                        f"Location: http://{iot}:8200/rootDesc.xml" in out, out[-300:]))
    gout = lab.out("guesthost", "timeout 12 gssdp-discover -i guest -n 4 -t ssdp:all 2>&1", timeout=20)
    checks.append(Check("guest finds nothing", "resource available" not in gout, gout[-300:]))
    services = lab.ctl_json("services")["services"]
    checks.append(Check("discoctl services lists the UPnP device", any(s["iface"] == "iot" and s["host"] == iot
                                                                      and s["instance"].startswith("uuid:")
                                                                      for s in services)))
    # byebye: a listener on lan sees the device go when minidlna stops.
    lab.bg_start("lanhost", "gssdp", "gssdp-discover -i lan -n 8 -t ssdp:all -m unavailable")
    time.sleep(2)
    lab.sh("iothost", "systemctl stop minidlna", check=True)
    time.sleep(7)
    bye = lab.bg_stop("lanhost", "gssdp")
    checks.append(Check("ssdp:byebye reaches lan", "resource unavailable" in bye, bye[-300:]))
    lab.ensure_minidlna()
    # A NOTIFY alive arrives on its own (notify_interval is 30 s in the lab).
    out = lab.out("lanhost", "timeout 40 gssdp-discover -i lan -n 35 -t upnp:rootdevice -m available 2>&1", timeout=50)
    checks.append(Check("ssdp:alive from minidlna forwarded", f"Location: http://{iot}:8200/rootDesc.xml" in out, out[-200:]))
    return checks


def case_dynamics(lab, args):
    """6. Interface dynamics: link down/up, the iot host rebooted, the iot interface removed and re-added."""
    lab.ensure_printer()
    lab.set_rules(PRINTER_RULES)
    checks = []

    def iface(name):
        return next(i for i in lab.ctl_json("status")["interfaces"] if i["name"] == name)

    lab.sh("router", "ip link set iot down", check=True)
    time.sleep(1)
    i = iface("iot")
    checks.append(Check("link down: status shows iot down", not i["up"], json.dumps(i)))
    lab.sh("router", "ip link set iot up", check=True)
    time.sleep(3)
    i = iface("iot")
    checks.append(Check("link up: iot joined again", i["up"] and i["joined_v4"] and i["joined_v6"], json.dumps(i)))
    lines = browse(lab, "lanhost", "lan")
    checks.append(Check("browse works after down/up", any(l.startswith("=;lan;IPv4;") for l in lines)))
    # The iot host reboots: its avahi comes back, the printer is re-published, the browse works.
    lab.node_sh(f"pct reboot {lab.ct['iothost']}", timeout=180, check=True)
    for _ in range(60):
        if lab.out("iothost", "systemctl is-system-running 2>/dev/null").strip() in ("running", "degraded"):
            break
        time.sleep(2)
    lab.ensure_routes()
    lab.ensure_printer()
    lab.ensure_minidlna()
    lines = browse(lab, "lanhost", "lan")
    checks.append(Check("browse works after the iot host rebooted", any(l.startswith("=;lan;IPv4;") for l in lines),
                        "\n".join(lines)[-300:]))
    # The router's iot interface disappears and comes back (hot-unplug and -plug on the node).
    cfg = lab.node_sh(f"pct config {lab.ct['router']}", check=True).stdout
    spec = next(l.split(": ", 1)[1] for l in cfg.splitlines() if l.startswith("net2: "))
    assert "name=iot" in spec, spec
    lab.node_sh(f"pct set {lab.ct['router']} --delete net2", check=True)
    time.sleep(2)
    st = lab.ctl_json("status")
    gone = [i for i in st["interfaces"] if i["name"] == "iot"]
    checks.append(Check("iot removed: not up / absent in status", not gone or not gone[0]["up"], json.dumps(gone)))
    checks.append(Check("daemon survives the removal", lab.out("router", "systemctl is-active discowalld").strip() == "active"))
    spec_new = ",".join(p for p in spec.split(",") if not p.startswith("hwaddr="))
    lab.node_sh(f"pct set {lab.ct['router']} --net2 {shlex.quote(spec_new)}", check=True)
    for _ in range(20):
        time.sleep(1)
        st = lab.ctl_json("status")
        back = [i for i in st["interfaces"] if i["name"] == "iot" and i["up"] and i["joined_v4"]]
        if back:
            break
    checks.append(Check("iot re-added: up and joined again", bool(back), json.dumps([i for i in st["interfaces"] if i["name"] == "iot"])))
    lab.sh("router", "ifup iot >/dev/null 2>&1; true")  # the addresses, if ifupdown did not add them itself
    time.sleep(2)
    lines = browse(lab, "lanhost", "lan")
    checks.append(Check("browse works after the re-add", any(l.startswith("=;lan;IPv4;") for l in lines),
                        "\n".join(lines)[-300:]))
    return checks


ZEROCONF_PUBLISH = r'''
import socket, sys, time
from zeroconf import Zeroconf, ServiceInfo, IPVersion, InterfaceChoice
n = int(sys.argv[1]); addr = sys.argv[2]
zc = Zeroconf(interfaces=[addr], ip_version=IPVersion.V4Only)
infos = [ServiceInfo("_labsvc._tcp.local.", f"svc{i:03d}._labsvc._tcp.local.", addresses=[socket.inet_aton(addr)],
                     port=9000 + i, properties={"n": str(i)}, server=f"labsvc{i:03d}.local.") for i in range(n)]
for info in infos:
    zc.register_service(info, allow_name_change=False)
print("registered", n, flush=True)
while True:
    time.sleep(3600)
'''


def case_scale(lab, args):
    """7. Rules at scale and a soak: 300 rules, 50 services, memory flat over --soak seconds."""
    lab.ensure_printer()
    checks = []
    rules = ["default = \"drop\""]
    for i in range(300):
        rules.append(f'[[rule]]\nin = "lan"\nout = "iot"\nkind = ["query"]\nservice = ["_filler{i}._tcp"]\naction = "drop"\n')
    body = "\n".join(rules) + "\n"
    lab.sh("router", f"printf %s {shlex.quote(body)} > /etc/discowall/rules.toml", check=True)
    lab.ctl("apply")
    # The working rules after the filler, through discoctl (appended).
    for r in PRINTER_RULES + ["in lan out iot kind query service _labsvc._tcp accept",
                              "in iot out lan kind response,announce service _labsvc._tcp accept"]:
        lab.ctl("rule", "add", *r.split())
    n_rules = len(lab.ctl_json("rule", "list")["rules"])
    checks.append(Check("306 rules loaded", n_rules == 306, str(n_rules)))
    lab.sh("iothost", f"printf %s {shlex.quote(ZEROCONF_PUBLISH)} > /root/publish50.py", check=True)
    lab.bg_start("iothost", "svc50", f"python3 /root/publish50.py 50 {lab.addr('iot')}")
    time.sleep(4)
    checks.append(Check("50 services registered (python-zeroconf)",
                        "registered 50" in lab.out("iothost", "cat /tmp/lab-svc50.out")))
    lines = browse(lab, "lanhost", "lan", service="_labsvc._tcp", timeout=12)
    found = {l.split(";")[3] for l in lines if l.startswith("+;lan;IPv4;")}
    checks.append(Check("lan browses all 50 instances", len(found) == 50, f"{len(found)} instances"))
    rss0 = int(lab.out("router", "systemctl show -p MemoryCurrent --value discowalld").strip() or 0)
    c0 = lab.counters()
    lab.bg_start("lanhost", "watch", f"sh -c 'for i in $(seq 1 {max(1, args.soak // 10)}); do avahi-browse -pt _labsvc._tcp >/dev/null; sleep 10; done'")
    t0 = time.time()
    while time.time() - t0 < args.soak:
        time.sleep(min(10, args.soak))
        lab.sh("iothost", "systemctl restart lab-printer")  # announcements keep coming
    lab.bg_stop("lanhost", "watch")
    rss1 = int(lab.out("router", "systemctl show -p MemoryCurrent --value discowalld").strip() or 0)
    c1 = lab.counters()
    checks.append(Check(f"memory flat over {args.soak} s (< +4 MiB)", rss1 - rss0 < 4 * 1024 * 1024,
                        f"RSS {rss0 // 1024} KiB -> {rss1 // 1024} KiB; rx {c0['rx']} -> {c1['rx']}, "
                        f"send_errors {c1['send_errors']}"))
    checks.append(Check("no send errors", c1["send_errors"] == 0))
    checks.append(Check("unit still active", lab.out("router", "systemctl is-active discowalld").strip() == "active"))
    lab.bg_stop("iothost", "svc50")
    lab.set_rules(PRINTER_RULES)
    lab.ctl("save")
    return checks


def case_ipv6only(lab, args):
    """8. IPv6 only: ipv4 = false, the same browse over the ULA addresses, nothing over IPv4."""
    lab.ensure_printer()
    lab.set_rules(PRINTER_RULES)
    lab.set_config(ipv4=False)
    lab.restart_daemon()
    iot6 = lab.addr("iot", v6=True)
    checks = []
    st = lab.ctl_json("status")
    checks.append(Check("v6 joined, v4 not", all(i["joined_v6"] and not i["joined_v4"] for i in st["interfaces"]),
                        json.dumps([(i["name"], i["joined_v4"], i["joined_v6"]) for i in st["interfaces"]])))
    lines = browse(lab, "lanhost", "lan")
    checks.append(Check("lan browses the printer over IPv6", any(l.startswith("=;lan;IPv6;") and f";{iot6};631;" in l
                                                                for l in lines), "\n".join(lines)[-300:]))
    checks.append(Check("nothing over IPv4", not any(";IPv4;" in l for l in lines), "\n".join(lines)[-300:]))
    r6 = lab.out("lanhost", "avahi-resolve-host-name -6 discowall-iothost.local").strip()
    checks.append(Check("lan resolves iothost.local over IPv6", r6.endswith(iot6), r6))
    lab.set_config()
    lab.restart_daemon()
    return checks


def case_upgrade(lab, args):
    """9. Upgrade: the previous .deb with saved rules, then the new one; rules and conffiles survive."""
    if not (args.prev_deb and args.deb):
        return [Check("skipped", True, "needs --prev-deb and --deb")]
    checks = []
    p = install_deb(lab, args.prev_deb)
    checks.append(Check("previous .deb installs", p.returncode == 0, (p.stdout + p.stderr)[-300:]))
    lab.set_config()
    lab.restart_daemon()
    v0 = lab.ctl_json("status")["version"]
    lab.set_rules(PRINTER_RULES + ["in guest kind query service _nothing._tcp drop comment upgrade-marker"])
    lab.ctl("save")
    p = install_deb(lab, args.deb)
    checks.append(Check("new .deb installs over it", p.returncode == 0, (p.stdout + p.stderr)[-300:]))
    lab.restart_daemon()
    v1 = lab.ctl_json("status")["version"]
    checks.append(Check("version changed", v0 != v1, f"{v0} -> {v1}"))
    rules = lab.ctl("rule", "list")
    checks.append(Check("saved rules survived (conffile kept)", "upgrade-marker" in rules, rules[-300:]))
    cfg = lab.out("router", "grep ^interfaces /etc/discowall/discowall.toml").strip()
    checks.append(Check("discowall.toml survived", '"guest"' in cfg, cfg))
    checks.append(Check("unit active after the upgrade", lab.out("router", "systemctl is-active discowalld").strip() == "active"))
    lines = browse(lab, "lanhost", "lan")
    checks.append(Check("browse works after the upgrade", any(l.startswith("=;lan;IPv4;") for l in lines)))
    return checks


CASES = [
    ("install", case_install),
    ("avahi", case_avahi),
    ("coexistence", case_coexistence),
    ("legacy", case_legacy),
    ("ssdp", case_ssdp),
    ("dynamics", case_dynamics),
    ("scale", case_scale),
    ("ipv6only", case_ipv6only),
    ("upgrade", case_upgrade),
]


# --- main ----------------------------------------------------------------------------------------------
def load_config(path):
    if tomllib is None:
        sys.exit("Python 3.11+ (tomllib) needed")
    with open(os.path.expanduser(path), "rb") as f:
        cfg = tomllib.load(f)
    for k in ("node", "containers", "networks"):
        if k not in cfg:
            sys.exit(f"{path}: missing [{k}]")
    return cfg


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--config", default="~/.config/discowall-lab/lab.toml")
    ap.add_argument("-v", "--verbose", action="store_true", help="print every command")
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("provision")
    sub.add_parser("list")
    run = sub.add_parser("run")
    run.add_argument("--deb", help="the .deb to install on the router first (case install / upgrade)")
    run.add_argument("--prev-deb", help="the previous .deb (case upgrade)")
    run.add_argument("--soak", type=int, default=60, help="seconds for the scale case (default 60)")
    run.add_argument("--json", help="write the checks to this file")
    run.add_argument("cases", nargs="*", help="which cases (default: all)")
    args = ap.parse_args()

    if args.cmd == "list":
        for name, fn in CASES:
            print(f"{name:12} {fn.__doc__.strip()}")
        return
    lab = Lab(load_config(args.config), verbose=args.verbose)
    if args.cmd == "provision":
        provision(lab)
        return
    for a in ("deb", "prev_deb"):
        v = getattr(args, a)
        if v:
            v = os.path.expanduser(v)
            if not os.path.exists(v):
                sys.exit(f"{v}: not found")
            setattr(args, a, v)
    wanted = args.cases or [n for n, _ in CASES]
    unknown = [c for c in wanted if c not in dict(CASES)]
    if unknown:
        sys.exit(f"unknown case(s): {unknown}; see `lab.py list`")
    lab.ensure_routes()
    results = []
    failed = 0
    for name, fn in CASES:
        if name not in wanted:
            continue
        t = time.time()
        try:
            checks = fn(lab, args)
        except Exception as e:  # a broken case is a failed case, not a crashed run
            checks = [Check("case ran", False, f"{type(e).__name__}: {e}")]
        bad = [c for c in checks if not c.ok]
        skipped = len(checks) == 1 and checks[0].name == "skipped"
        verdict = "SKIP" if skipped else ("FAIL" if bad else "PASS")
        failed += bool(bad)
        print(f"{verdict} {name:12} {len(checks) - len(bad)}/{len(checks)} checks  {time.time() - t:5.1f} s")
        for c in checks:
            if not c.ok or args.verbose:
                mark = "ok  " if c.ok else "FAIL"
                print(f"     {mark} {c.name}" + (f": {c.detail}" if c.detail else ""))
        results.append({"case": name, "verdict": verdict, "seconds": round(time.time() - t, 1),
                        "checks": [c.as_dict() for c in checks]})
    if args.json:
        with open(args.json, "w") as f:
            json.dump({"date": time.strftime("%Y-%m-%d %H:%M"), "results": results}, f, indent=1)
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
