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

/// A leaf namespace: a process parked in its own netns; `ip`/`python3` run inside with nsenter.
struct Leaf {
    child: Child,
}

impl Leaf {
    fn new() -> Self {
        let child = Command::new("unshare")
            .args(["-n", "sleep", "60"])
            .spawn()
            .expect("unshare -n");
        std::thread::sleep(Duration::from_millis(100));
        Self { child }
    }
    fn pid(&self) -> u32 {
        self.child.id()
    }
    fn run(&self, args: &[&str]) -> Command {
        let mut c = Command::new("nsenter");
        c.arg("-t").arg(self.pid().to_string()).arg("-n").args(args);
        c
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

fn inner() {
    // Router namespace: lo, a0 (to leaf A), b0 (to leaf B).
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

    let dir = std::env::temp_dir().join(format!("discowall-netns-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("control.sock");
    let cfg = dir.join("discowall.toml");
    let rules = dir.join("rules.toml");
    std::fs::write(&rules, "default = \"accept\"\n").unwrap();
    std::fs::write(
        &cfg,
        format!(
            "interfaces = [\"a0\", \"b0\"]\nipv6 = false\ncontrol_socket = {:?}\nrules_file = {:?}\n",
            sock.display(),
            rules.display()
        ),
    )
    .unwrap();
    // Killed on drop, so that a failing assertion does not leave a daemon behind that holds the
    // test's stdout open and hangs `cargo test`.
    struct Daemon(Child);
    impl Drop for Daemon {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut daemon = Daemon(
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
        let mut stream = std::os::unix::net::UnixStream::connect(&sock).unwrap();
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

    // Rules through discoctl: default drop, HomeKit from b0 may reach a0, only for host "bridge".
    let ctl = |args: &[&str]| -> String {
        let out = Command::new(discoctl())
            .arg("--socket")
            .arg(&sock)
            .args(args)
            .output()
            .expect("discoctl");
        assert!(
            out.status.success(),
            "discoctl {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    ctl(&["rule", "flush"]);
    ctl(&[
        "rule", "add", "in", "b0", "out", "a0", "kind", "announce", "host", "bridge", "accept",
    ]);
    ctl(&["save"]);
    let saved = std::fs::read_to_string(&rules).unwrap();
    assert!(saved.contains("bridge"), "{saved}");
    // The default policy is in the file as written by `save`; set it to drop and apply.
    std::fs::write(
        &rules,
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

    // B announces the bridge and a camera; only the bridge's announcement reaches A.
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

    daemon.0.kill().unwrap();
    daemon.0.wait().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}
