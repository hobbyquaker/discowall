//! Integration test in network namespaces: a router namespace runs discowalld between two veth
//! interfaces, a leaf namespace hangs on each. Needs user namespaces (`unshare -rn`), `ip`,
//! `nsenter` and `python3`; without them the test prints why and passes, so that a build without
//! the capability is not red.
//!
//! The test re-executes itself under `unshare -rnm` (the router namespace) with
//! `DISCOWALL_NETNS_INNER=1`; the inner run does the work.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const INNER: &str = "DISCOWALL_NETNS_INNER";

fn have(cmd: &str, args: &[&str]) -> bool {
    Command::new(cmd)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn sh(cmd: &str) {
    let st = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .status()
        .unwrap_or_else(|e| panic!("{cmd}: {e}"));
    assert!(st.success(), "failed: {cmd}");
}

/// A leaf namespace: a process parked in its own network and mount namespace; `ip`/`python3`/avahi
/// run inside with nsenter. The mount namespace gives each leaf a private `/run` for avahi.
struct Leaf {
    child: Child,
}

impl Leaf {
    fn new() -> Self {
        let child = Command::new("unshare")
            .args(["-nm", "sleep", "120"])
            .spawn()
            .expect("unshare -nm");
        std::thread::sleep(Duration::from_millis(100));
        Self { child }
    }
    fn pid(&self) -> u32 {
        self.child.id()
    }
    fn run(&self, args: &[&str]) -> Command {
        let mut c = Command::new("nsenter");
        c.arg("-t")
            .arg(self.pid().to_string())
            .args(["-n", "-m"])
            .args(args);
        c
    }
    fn output(&self, cmd: &str) -> String {
        let out = self.run(&["sh", "-c", cmd]).output().expect("nsenter");
        String::from_utf8_lossy(&out.stdout).to_string()
    }
    fn sh(&self, cmd: &str) {
        let st = self.run(&["sh", "-c", cmd]).status().expect("nsenter");
        assert!(st.success(), "failed in leaf: {cmd}");
    }
}

impl Drop for Leaf {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// discoctl is another crate's binary: next to this test's own binaries in target/<profile>/.
/// `cargo test -p discowalld` does not rebuild it; run `cargo build --all` first (CI does).
fn discoctl() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_BIN_EXE_discowalld"));
    p.set_file_name("discoctl");
    p
}

fn ctl_json(sock: &std::path::Path, args: &[&str]) -> String {
    let out = Command::new(discoctl())
        .arg("--socket")
        .arg(sock)
        .arg("--json")
        .args(args)
        .output()
        .expect("discoctl");
    assert!(
        out.status.success(),
        "discoctl {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn probe() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/mdns_probe.py")
}

#[test]
fn reflects_between_namespaces() {
    if std::env::var_os(INNER).is_some() {
        return inner();
    }
    if !have("unshare", &["-rn", "true"])
        || !have("ip", &["-V"])
        || !have("nsenter", &["-V"])
        || !have("python3", &["-c", ""])
    {
        eprintln!("skipped: needs user namespaces (unshare -rn), ip, nsenter and python3");
        return;
    }
    let exe = std::env::current_exe().unwrap();
    let st = Command::new("unshare")
        .args(["-rnm", "--"])
        .arg(&exe)
        .args(["--exact", "reflects_between_namespaces", "--nocapture"])
        .env(INNER, "1")
        .status()
        .expect("unshare");
    assert!(st.success(), "the namespaced run failed");
}

/// A child killed on drop, so that a failing assertion does not leave a daemon behind that holds
/// the test's stdout open and hangs `cargo test`.
struct Guard(Child);
impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The topology: this process is the router namespace with lo, a0 (10.1.0.1, fd00:1::1, to leaf A)
/// and b0 (10.2.0.1, fd00:2::1, to leaf B); leaf A has a1 = 10.1.0.2 / fd00:1::2, leaf B has
/// b1 = 10.2.0.2 / fd00:2::2. discowalld runs between a0 and b0 with the given rules file content.
struct Rig {
    a: Leaf,
    b: Leaf,
    dir: PathBuf,
    sock: PathBuf,
    rules: PathBuf,
    daemon: Guard,
}

impl Rig {
    fn new(rules_toml: &str) -> Self {
        sh("ip link set lo up");
        let a = Leaf::new();
        let b = Leaf::new();
        sh("ip link add a0 type veth peer name a1");
        sh("ip link add b0 type veth peer name b1");
        sh(&format!("ip link set a1 netns {}", a.pid()));
        sh(&format!("ip link set b1 netns {}", b.pid()));
        sh("ip addr add 10.1.0.1/24 dev a0 && ip link set a0 up");
        sh("ip addr add 10.2.0.1/24 dev b0 && ip link set b0 up");
        a.sh("ip link set lo up && ip addr add 10.1.0.2/24 dev a1 && ip link set a1 up");
        b.sh("ip link set lo up && ip addr add 10.2.0.2/24 dev b1 && ip link set b1 up");
        sh("ip -6 addr add fd00:1::1/64 dev a0 && ip -6 addr add fd00:2::1/64 dev b0");
        a.sh("ip -6 addr add fd00:1::2/64 dev a1");
        b.sh("ip -6 addr add fd00:2::2/64 dev b1");
        // Duplicate address detection must finish before the v6 addresses are usable.
        std::thread::sleep(Duration::from_millis(1500));

        let dir = std::env::temp_dir().join(format!("discowall-netns-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let sock = dir.join("control.sock");
        let cfg = dir.join("discowall.toml");
        let rules = dir.join("rules.toml");
        std::fs::write(&rules, rules_toml).unwrap();
        std::fs::write(
            &cfg,
            format!(
                "interfaces = [\"a0\", \"b0\"]\ncontrol_socket = {:?}\nrules_file = {:?}\n",
                sock.display(),
                rules.display()
            ),
        )
        .unwrap();
        let daemon = Guard(
            Command::new(env!("CARGO_BIN_EXE_discowalld"))
                .arg("--config")
                .arg(&cfg)
                .arg("--log")
                .arg("debug")
                .spawn()
                .expect("discowalld"),
        );
        let start = Instant::now();
        while !sock.exists() && start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(sock.exists(), "the daemon did not open its control socket");
        // Give the interface dump and the joins a moment.
        std::thread::sleep(Duration::from_millis(300));
        Self {
            a,
            b,
            dir,
            sock,
            rules,
            daemon,
        }
    }

    /// discoctl against this daemon, text output; panics when it fails.
    fn ctl(&self, args: &[&str]) -> String {
        let out = Command::new(discoctl())
            .arg("--socket")
            .arg(&self.sock)
            .args(args)
            .output()
            .expect("discoctl");
        assert!(
            out.status.success(),
            "discoctl {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).to_string()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = self.daemon.0.kill();
        let _ = self.daemon.0.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn inner() {
    let rig = Rig::new("default = \"accept\"\n");
    let (a, b) = (&rig.a, &rig.b);
    let sock = &rig.sock;
    let rules = &rig.rules;

    // B listens, A sends: the packet must arrive in B with the router's address on b0 as source.
    let recv_b = b
        .run(&["python3"])
        .arg(probe())
        .args(["recv", "10.2.0.2", "3"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    // A also listens for its own segment: nothing must come back there.
    let recv_a = a
        .run(&["python3"])
        .arg(probe())
        .args(["recv", "10.1.0.2", "1.5"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    a.sh(&format!(
        "python3 {} send 10.1.0.2 query _hap._tcp.local",
        probe().display()
    ));

    let out_b = recv_b.wait_with_output().unwrap();
    let got_b = String::from_utf8_lossy(&out_b.stdout).trim().to_string();
    assert_eq!(
        got_b, "10.2.0.1:5353 query _hap._tcp.local",
        "leaf B should get the reflected query"
    );
    let out_a = recv_a.wait_with_output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out_a.stdout).trim(),
        "",
        "nothing may come back to leaf A"
    );

    // Status over the control socket (discoctl lives in its own crate; the protocol is one JSON
    // line each way): both interfaces up and joined, the counters moved.
    let status = {
        use std::io::{BufRead, BufReader, Write};
        let mut stream = std::os::unix::net::UnixStream::connect(sock).unwrap();
        stream.write_all(b"{\"cmd\":\"status\"}\n").unwrap();
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).unwrap();
        serde_json::from_str::<serde_json::Value>(&line).unwrap()
    };
    assert_eq!(status["result"], "status");
    for i in status["interfaces"].as_array().unwrap() {
        assert_eq!(i["up"], true, "{i}");
        assert_eq!(i["joined_v4"], true, "{i}");
    }
    assert_eq!(status["counters"]["tx"], 1, "{status}");
    assert_eq!(status["counters"]["send_errors"], 0);

    // IPv6: the same query over ff02::fb, from leaf A, arrives in leaf B from the router's
    // link-local address on b0.
    let recv_b6 = b
        .run(&["python3"])
        .arg(probe())
        .args(["recv", "v6:b1", "3"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    a.sh(&format!(
        "python3 {} send v6:a1 query _hap._tcp.local",
        probe().display()
    ));
    let out_b6 = String::from_utf8_lossy(&recv_b6.wait_with_output().unwrap().stdout)
        .trim()
        .to_string();
    assert!(
        out_b6.starts_with("fe80::") && out_b6.ends_with(":5353 query _hap._tcp.local"),
        "v6 reflection: {out_b6:?}"
    );

    // A one-shot query from an ephemeral port in A: the daemon asks in B on A's behalf, a legacy
    // responder in B answers the daemon by unicast, and the daemon relays the answer to A by
    // unicast from port 5353.
    let responder = b
        .run(&["python3"])
        .arg(probe())
        .args([
            "respond",
            "10.2.0.2",
            "4",
            "Printer",
            "_ipp._tcp",
            "printer",
            "10.2.0.7",
        ])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let legacy = a
        .run(&["python3"])
        .arg(probe())
        .args(["legacy", "10.1.0.2", "_ipp._tcp.local", "3"])
        .output()
        .unwrap();
    let got = String::from_utf8_lossy(&legacy.stdout).trim().to_string();
    assert_eq!(
        got,
        "10.1.0.1:5353 response _ipp._tcp.local _ipp._tcp.local/12 Printer._ipp._tcp.local/33 Printer._ipp._tcp.local/16 printer.local/1",
        "the legacy answer must come back by unicast from the router"
    );
    let answered = String::from_utf8_lossy(&responder.wait_with_output().unwrap().stdout)
        .trim()
        .to_string();
    assert!(
        answered.starts_with("answered 10.2.0.1:"),
        "the responder saw the daemon's ephemeral query: {answered:?}"
    );
    let status: serde_json::Value = serde_json::from_str(&ctl_json(sock, &["status"])).unwrap();
    assert_eq!(status["counters"]["proxied_queries"], 1, "{status}");
    assert_eq!(status["counters"]["proxied_answers"], 1, "{status}");

    // Rules through discoctl: default drop, HomeKit from b0 may reach a0, only for host "bridge".
    let ctl = |args: &[&str]| rig.ctl(args);
    ctl(&["rule", "flush"]);
    ctl(&[
        "rule", "add", "in", "b0", "out", "a0", "kind", "announce", "host", "bridge", "accept",
    ]);
    ctl(&["save"]);
    let saved = std::fs::read_to_string(rules).unwrap();
    assert!(saved.contains("bridge"), "{saved}");
    // The default policy is in the file as written by `save`; set it to drop and apply.
    std::fs::write(
        rules,
        saved.replace("default = \"accept\"", "default = \"drop\""),
    )
    .unwrap();
    ctl(&["apply"]);
    let listed = ctl(&["rule", "list"]);
    assert!(listed.starts_with("default drop"), "{listed}");
    assert!(
        listed.contains("in b0 out a0 kind announce host bridge accept"),
        "{listed}"
    );
    assert_eq!(
        ctl(&["rule", "test", "in", "b0", "out", "a0", "kind", "announce", "host", "bridge"])
            .trim(),
        "accept (rule 1)"
    );
    assert_eq!(
        ctl(&["rule", "test", "in", "a0", "out", "b0", "kind", "query"]).trim(),
        "drop (default)"
    );

    // B announces the bridge and a camera; only the bridge's announcement reaches A. A watcher
    // sees both packets with their decisions.
    let mut watcher = Command::new(discoctl())
        .arg("--socket")
        .arg(sock)
        .args(["--json", "watch", "--iface", "b0"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let recv_a = a
        .run(&["python3"])
        .arg(probe())
        .args(["recv", "10.1.0.2", "2", "2"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    b.sh(&format!(
        "python3 {} send 10.2.0.2 announce Cam _hap._tcp camera 10.2.0.9",
        probe().display()
    ));
    b.sh(&format!(
        "python3 {} send 10.2.0.2 announce Bridge _hap._tcp bridge 10.2.0.5",
        probe().display()
    ));
    let out_a = recv_a.wait_with_output().unwrap();
    let got_a = String::from_utf8_lossy(&out_a.stdout).trim().to_string();
    assert_eq!(
        got_a,
        "10.1.0.1:5353 response _hap._tcp.local/12 Bridge._hap._tcp.local/33 Bridge._hap._tcp.local/16 bridge.local/1",
        "only the bridge's announcement may reach leaf A"
    );
    let hosts = ctl(&["hosts"]);
    assert!(
        hosts.contains("bridge.local") && hosts.contains("camera.local"),
        "{hosts}"
    );
    let hosts_json: serde_json::Value = serde_json::from_str(&ctl(&["--json", "hosts"])).unwrap();
    assert_eq!(hosts_json["result"], "hosts");
    let services = ctl(&["services"]);
    assert!(
        services.contains("_hap._tcp") && services.contains("bridge._hap._tcp.local"),
        "{services}"
    );
    let stats = ctl(&["stats"]);
    assert!(
        stats.contains("default")
            && stats.contains("in b0 out a0 kind announce host bridge accept"),
        "{stats}"
    );
    let stats_json: serde_json::Value = serde_json::from_str(&ctl(&["--json", "stats"])).unwrap();
    assert_eq!(stats_json["rules"][0]["hits"], 4, "{stats_json}");

    // SSDP: a renderer announces in B and is seen in A; a search from A is proxied into B and the
    // responder's unicast answer comes back to A from the router's port 1900.
    ctl(&[
        "rule",
        "add",
        "proto",
        "ssdp",
        "in",
        "b0",
        "out",
        "a0",
        "kind",
        "announce,response",
        "accept",
    ]);
    ctl(&[
        "rule", "add", "proto", "ssdp", "in", "a0", "out", "b0", "kind", "query", "accept",
    ]);
    let recv_a = a
        .run(&["python3"])
        .arg(probe())
        .args(["ssdp-recv", "10.1.0.2", "3"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    b.sh(&format!(
        "python3 {} ssdp-notify 10.2.0.2 uuid:renderer-1 urn:schemas-upnp-org:device:MediaRenderer:1 http://10.2.0.8:1400/xml",
        probe().display()
    ));
    let got = String::from_utf8_lossy(&recv_a.wait_with_output().unwrap().stdout)
        .trim()
        .to_string();
    assert_eq!(
        got,
        "10.1.0.1:1900 NOTIFY * HTTP/1.1 NTS=ssdp:alive NT=urn:schemas-upnp-org:device:MediaRenderer:1 USN=uuid:renderer-1::urn:schemas-upnp-org:device:MediaRenderer:1 LOCATION=http://10.2.0.8:1400/xml",
        "the alive must reach leaf A"
    );
    let responder = b
        .run(&["python3"])
        .arg(probe())
        .args([
            "ssdp-respond",
            "10.2.0.2",
            "4",
            "uuid:renderer-1",
            "upnp:rootdevice",
            "http://10.2.0.8:1400/xml",
        ])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let search = a
        .run(&["python3"])
        .arg(probe())
        .args(["ssdp-search", "10.1.0.2", "ssdp:all", "3"])
        .output()
        .unwrap();
    let got = String::from_utf8_lossy(&search.stdout).trim().to_string();
    assert_eq!(
        got,
        "10.1.0.1:1900 HTTP/1.1 200 OK ST=upnp:rootdevice USN=uuid:renderer-1::upnp:rootdevice LOCATION=http://10.2.0.8:1400/xml",
        "the search answer must come back by unicast from the router"
    );
    let answered = String::from_utf8_lossy(&responder.wait_with_output().unwrap().stdout)
        .trim()
        .to_string();
    assert!(answered.starts_with("answered 10.2.0.1:"), "{answered:?}");
    let services = ctl(&["services"]);
    assert!(
        services.contains("uuid:renderer-1") && services.contains("mediarenderer"),
        "{services}"
    );

    watcher.kill().unwrap();
    let watched = watcher.wait_with_output().unwrap();
    let lines: Vec<serde_json::Value> = String::from_utf8_lossy(&watched.stdout)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert!(
        lines.len() >= 2,
        "two mDNS announcements on b0 at least: {lines:?}"
    );
    assert_eq!(lines[0]["kind"], "announce");
    assert_eq!(lines[0]["hosts"][0], "camera.local");
    assert_eq!(lines[0]["out"][0]["result"], "dropped");
    assert_eq!(lines[1]["hosts"][0], "bridge.local");
    assert_eq!(lines[1]["out"][0]["result"], "sent");
    assert_eq!(lines[1]["out"][0]["rules"][0], 1);
}

// ---------------------------------------------------------------------------------------------
// Real avahi on both ends (task 12): avahi-daemon in each leaf with its own D-Bus bus, publishing
// in B, browsing in A through the rules; then avahi beside discowalld in the router namespace.
// Skips where avahi-daemon, avahi-utils or dbus-daemon are not installed.

/// A D-Bus system bus and an avahi-daemon inside one namespace (a leaf, or this process's own).
struct Avahi {
    bus: Guard,
    daemon: Guard,
    /// `DBUS_SYSTEM_BUS_ADDRESS` for the tools talking to this instance.
    bus_addr: String,
    dir: PathBuf,
}

impl Avahi {
    /// `leaf` is `None` for the router namespace (this process). `host` is avahi's host-name,
    /// `ifaces` its allow-interfaces. `/run` gets a private tmpfs first (avahi's pid file).
    fn start(leaf: Option<&Leaf>, host: &str, ifaces: &str, reflector: bool) -> Self {
        let dir =
            std::env::temp_dir().join(format!("discowall-avahi-{}-{host}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bus_path = dir.join("bus");
        let bus_addr = format!("unix:path={}", bus_path.display());
        let bus_conf = dir.join("dbus.conf");
        std::fs::write(
            &bus_conf,
            format!(
                "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\" \
                 \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">\n<busconfig>\n\
                 <type>system</type>\n<listen>{bus_addr}</listen>\n<auth>EXTERNAL</auth>\n\
                 <policy context=\"default\"><allow user=\"*\"/><allow own=\"*\"/>\
                 <allow send_destination=\"*\"/><allow receive_sender=\"*\"/></policy>\n</busconfig>\n"
            ),
        )
        .unwrap();
        let avahi_conf = dir.join("avahi-daemon.conf");
        std::fs::write(
            &avahi_conf,
            format!(
                "[server]\nhost-name={host}\nuse-ipv4=yes\nuse-ipv6=yes\nallow-interfaces={ifaces}\n\
                 enable-dbus=yes\nratelimit-interval-usec=1000000\nratelimit-burst=1000\n\
                 [publish]\npublish-hinfo=no\npublish-workstation=no\n\
                 [reflector]\nenable-reflector={}\n",
                if reflector { "yes" } else { "no" }
            ),
        )
        .unwrap();
        let run = |args: &[&str]| -> Command {
            match leaf {
                Some(l) => l.run(args),
                None => {
                    let mut c = Command::new(args[0]);
                    c.args(&args[1..]);
                    c
                }
            }
        };
        // avahi insists that /run/avahi-daemon is owned by the user `avahi`, and a user namespace
        // with one mapped uid cannot chown to anyone else: in this mount namespace `avahi` is uid 0.
        let passwd = dir.join("passwd");
        std::fs::write(
            &passwd,
            format!(
                "avahi:x:0:0:avahi:/run/avahi-daemon:/usr/sbin/nologin\n{}",
                std::fs::read_to_string("/etc/passwd").unwrap_or_default()
            ),
        )
        .unwrap();
        let group = dir.join("group");
        std::fs::write(
            &group,
            format!(
                "avahi:x:0:\n{}",
                std::fs::read_to_string("/etc/group").unwrap_or_default()
            ),
        )
        .unwrap();
        // Idempotent: a second start in the same namespace (a restart for a fresh cache) first
        // takes the previous mounts down.
        let out = run(&[
            "sh",
            "-c",
            &format!(
                "umount -l /etc/passwd /etc/group /run 2>/dev/null; \
                 mount --bind {} /etc/passwd && mount --bind {} /etc/group && \
                 mount -t tmpfs none /run && mkdir -p /run/avahi-daemon",
                passwd.display(),
                group.display()
            ),
        ])
        .output()
        .expect("mount");
        assert!(
            out.status.success(),
            "a private /run and an `avahi` user for avahi:\n{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let bus = Guard(
            run(&["dbus-daemon", "--nofork", "--nopidfile", "--config-file"])
                .arg(&bus_conf)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("dbus-daemon"),
        );
        let start = Instant::now();
        while !bus_path.exists() && start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(bus_path.exists(), "dbus-daemon did not open its socket");
        let log = std::fs::File::create(dir.join("avahi.log")).unwrap();
        let daemon = Guard(
            run(&["env"])
                .arg(format!("DBUS_SYSTEM_BUS_ADDRESS={bus_addr}"))
                .args([
                    "avahi-daemon",
                    "--no-drop-root",
                    "--no-chroot",
                    "--no-rlimits",
                    "-f",
                ])
                .arg(&avahi_conf)
                .stdout(Stdio::null())
                .stderr(log)
                .spawn()
                .expect("avahi-daemon"),
        );
        // Ready when it answers on the bus; probing of its own name takes about a second more.
        let start = Instant::now();
        loop {
            let ok = run(&["env"])
                .arg(format!("DBUS_SYSTEM_BUS_ADDRESS={bus_addr}"))
                .args(["avahi-daemon", "-c"])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if ok {
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "avahi-daemon ({host}) did not come up:\n{}",
                std::fs::read_to_string(dir.join("avahi.log")).unwrap_or_default()
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        std::thread::sleep(Duration::from_millis(1500));
        Self {
            bus,
            daemon,
            bus_addr,
            dir,
        }
    }

    /// An avahi tool against this instance, in its namespace; stdout.
    fn tool(&self, leaf: Option<&Leaf>, cmd: &str) -> String {
        let script = format!("DBUS_SYSTEM_BUS_ADDRESS='{}' {cmd}", self.bus_addr);
        match leaf {
            Some(l) => l.output(&script),
            None => {
                let out = Command::new("sh").args(["-c", &script]).output().unwrap();
                String::from_utf8_lossy(&out.stdout).to_string()
            }
        }
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.dir.join("avahi.log")).unwrap_or_default()
    }
}

impl Drop for Avahi {
    fn drop(&mut self) {
        let _ = self.daemon.0.kill();
        let _ = self.daemon.0.wait();
        let _ = self.bus.0.kill();
        let _ = self.bus.0.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn avahi_between_namespaces() {
    if std::env::var_os(INNER).is_some() {
        return inner_avahi();
    }
    if !have("unshare", &["-rn", "true"])
        || !have("ip", &["-V"])
        || !have("nsenter", &["-V"])
        || !have("python3", &["-c", ""])
    {
        eprintln!("skipped: needs user namespaces (unshare -rn), ip, nsenter and python3");
        return;
    }
    if !have("avahi-daemon", &["--version"])
        || !have("avahi-browse", &["--version"])
        || !have("avahi-publish", &["--version"])
        || !have("dbus-daemon", &["--version"])
    {
        eprintln!("skipped: needs avahi-daemon, avahi-utils and dbus-daemon");
        return;
    }
    let exe = std::env::current_exe().unwrap();
    let st = Command::new("unshare")
        .args(["-rnm", "--"])
        .arg(&exe)
        .args(["--exact", "avahi_between_namespaces", "--nocapture"])
        .env(INNER, "1")
        .status()
        .expect("unshare");
    assert!(st.success(), "the namespaced run failed");
}

fn inner_avahi() {
    // Default drop; the lab's printer rules: lan (a0) may ask iot (b0) for printers and addresses,
    // iot may answer with printers and the host leafb.
    let rig = Rig::new("default = \"drop\"\n");
    for rule in [
        "in a0 out b0 kind query service _ipp._tcp accept",
        "in b0 out a0 kind response,announce service _ipp._tcp accept",
        "in a0 out b0 kind query rrtype A,AAAA accept",
        "in b0 out a0 kind response host leafb accept",
    ] {
        let mut args = vec!["rule", "add"];
        args.extend(rule.split(' '));
        rig.ctl(&args);
    }
    let avahi_a = Avahi::start(Some(&rig.a), "leafa", "a1", false);
    let avahi_b = Avahi::start(Some(&rig.b), "leafb", "b1", false);

    // B publishes a printer; A browses it through the rules, over both families, and resolves the
    // host. avahi-browse sends its first query over IPv4 and IPv6 at the same moment (B-2).
    let _publish = Guard(
        rig.b
            .run(&["env"])
            .arg(format!("DBUS_SYSTEM_BUS_ADDRESS={}", avahi_b.bus_addr))
            .args([
                "avahi-publish",
                "-s",
                "Lab Printer",
                "_ipp._tcp",
                "631",
                "txtvers=1",
                "rp=ipp/print",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("avahi-publish"),
    );
    std::thread::sleep(Duration::from_millis(1500));
    let browse = avahi_a.tool(Some(&rig.a), "timeout 8 avahi-browse -rpt _ipp._tcp");
    let resolved: Vec<&str> = browse.lines().filter(|l| l.starts_with("=;a1;")).collect();
    assert!(
        resolved.iter().any(|l| l.contains(";IPv4;Lab\\032Printer;")
            && l.contains(";leafb.local;10.2.0.2;631;")
            && l.contains("\"rp=ipp/print\" \"txtvers=1\"")),
        "A should browse and resolve the printer over IPv4:\n{browse}\nA's avahi:\n{}\nB's avahi:\n{}",
        avahi_a.log(),
        avahi_b.log()
    );
    assert!(
        resolved
            .iter()
            .any(|l| l.contains(";IPv6;Lab\\032Printer;")
                && l.contains(";leafb.local;fd00:2::2;631;")),
        "A should browse and resolve the printer over IPv6:\n{browse}"
    );
    let resolve = avahi_a.tool(Some(&rig.a), "avahi-resolve-host-name -4 leafb.local");
    assert_eq!(resolve.trim(), "leafb.local\t10.2.0.2", "{resolve}");
    let hosts = rig.ctl(&["hosts"]);
    assert!(hosts.contains("leafb.local"), "{hosts}");

    // avahi beside discowalld in the router namespace, reflector off: both get the multicast — avahi
    // answers for the router's own name, discowall still forwards the printer (task 10 in the gate).
    let avahi_r = Avahi::start(None, "router", "a0,b0", false);
    let resolve = avahi_a.tool(Some(&rig.a), "avahi-resolve-host-name -4 router.local");
    assert_eq!(
        resolve.trim(),
        "router.local\t10.1.0.1",
        "avahi on the router should answer beside discowalld: {resolve}\n{}",
        avahi_r.log()
    );
    // A fresh cache in A (restart its avahi) so that the browse is forwarding, not memory.
    drop(avahi_a);
    let avahi_a = Avahi::start(Some(&rig.a), "leafa", "a1", false);
    let browse = avahi_a.tool(Some(&rig.a), "timeout 8 avahi-browse -rpt _ipp._tcp");
    assert!(
        browse
            .lines()
            .any(|l| l.starts_with("=;a1;IPv4;") && l.contains(";leafb.local;10.2.0.2;631;")),
        "the printer should still browse with avahi beside the daemon:\n{browse}"
    );
    let status: serde_json::Value =
        serde_json::from_str(&ctl_json(&rig.sock, &["status"])).unwrap();
    assert!(
        status["counters"]["dropped_own_source"].as_u64().unwrap() > 0,
        "avahi's own packets on a0/b0 are dropped as own-source: {status}"
    );
    assert_eq!(status["counters"]["send_errors"], 0, "{status}");
}
