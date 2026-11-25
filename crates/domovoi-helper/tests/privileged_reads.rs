//! The read operations against the installed helper unit, as the real `domovoi-agent` account.
//!
//! Never runs by default: every test is `#[ignore]`d and also needs `DOMOVOI_PRIVILEGED_TESTS=1`,
//! root, `runuser`, `python3` and a host laid out by `install.sh` (the helper socket at
//! `/run/domovoi-helper.sock` and its units installed). `deploy/test/identities.sh` sets that
//! host up and runs these after its own rows. Run them only on a disposable machine:
//!
//! ```text
//! sudo DOMOVOI_PRIVILEGED_TESTS=1 cargo test -p domovoi-helper --test privileged_reads -- --ignored
//! ```
//!
//! Each test sends one raw request frame and checks the typed answer, so a missing capability or
//! device in the shipped unit shows as a `read_failed` answer or a failed unit instead of a pass.
#![cfg(unix)]

use std::process::Command;

use serde_json::Value;

/// The framing of the helper protocol, written out on purpose: a 4-byte big-endian length, then
/// the JSON envelope. It prints the `response` of the answer, or `connect-error` / `closed`.
const CLIENT: &str = r#"
import json, socket, struct, sys, uuid
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
try:
    s.connect("/run/domovoi-helper.sock")
except OSError as e:
    print("connect-error", e.errno)
    sys.exit(3)
request = {"op": sys.argv[1]}
if len(sys.argv) > 2:
    request.update(json.loads(sys.argv[2]))
body = json.dumps({"v": 1, "id": str(uuid.uuid4()), "request": request}).encode()
s.sendall(struct.pack(">I", len(body)) + body)
head = b""
while len(head) < 4:
    chunk = s.recv(4 - len(head))
    if not chunk:
        print("closed")
        sys.exit(4)
    head += chunk
size = struct.unpack(">I", head)[0]
buf = b""
while len(buf) < size:
    chunk = s.recv(size - len(buf))
    if not chunk:
        break
    buf += chunk
print(json.dumps(json.loads(buf)["response"]))
"#;

fn enabled() -> bool {
    std::env::var("DOMOVOI_PRIVILEGED_TESTS").is_ok_and(|v| v == "1")
        && nix::unistd::geteuid().is_root()
}

/// Runs `args` as the `domovoi-agent` account and returns its standard output.
fn as_agent(args: &[&str]) -> (bool, String) {
    let out = Command::new("runuser")
        .args(["-u", "domovoi-agent", "--"])
        .args(args)
        .output()
        .expect("runuser starts");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).trim().to_owned(),
    )
}

/// The helper's typed answer to one request sent as the agent account.
fn ask(op: &str, extra: &str) -> Value {
    let mut args = vec!["python3", "-c", CLIENT, op];
    if !extra.is_empty() {
        args.push(extra);
    }
    let (_, text) = as_agent(&args);
    serde_json::from_str(&text).unwrap_or_else(|_| panic!("not a typed answer: {text}"))
}

fn result_of(answer: &Value) -> &str {
    answer["result"].as_str().unwrap_or_default()
}

fn failed_instances() -> String {
    let out = Command::new("systemctl")
        .args([
            "list-units",
            "--failed",
            "--plain",
            "--no-legend",
            "domovoi-helper@*",
        ])
        .output()
        .expect("systemctl starts");
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

#[test]
#[ignore = "needs root, useradd and systemd as PID 1"]
fn helper_unit_starts_under_the_widened_directives() {
    if !enabled() {
        return;
    }
    assert_eq!(result_of(&ask("health", "")), "health");
    assert_eq!(failed_instances(), "", "an instance is left failed");
}

#[test]
#[ignore = "needs root, useradd and systemd as PID 1"]
fn helper_smart_report_runs_in_the_shipped_unit() {
    if !enabled() {
        return;
    }
    let answer = ask("smart_scan", "");
    match result_of(&answer) {
        "smart_scan" => {}
        "read_failed" => assert_eq!(answer["token"], "tool_missing", "{answer}"),
        other => panic!("smart_scan answered {other}: {answer}"),
    }
    assert_eq!(failed_instances(), "");
}

#[test]
#[ignore = "needs root, useradd and systemd as PID 1"]
fn helper_kernel_log_reads_with_dmesg_restrict_on() {
    if !enabled() {
        return;
    }
    let old = std::fs::read_to_string("/proc/sys/kernel/dmesg_restrict").unwrap_or_default();
    std::fs::write("/proc/sys/kernel/dmesg_restrict", "1").expect("set dmesg_restrict");
    let (agent_can_read, _) = as_agent(&["dmesg"]);
    let answer = ask("kernel_log", r#"{"since_minutes":1440,"lines":50}"#);
    let _ = std::fs::write("/proc/sys/kernel/dmesg_restrict", old.trim());
    assert!(
        !agent_can_read,
        "the agent account reads the ring buffer itself"
    );
    assert_eq!(result_of(&answer), "kernel_log", "{answer}");
}

#[test]
#[ignore = "needs root, useradd and systemd as PID 1"]
fn helper_net_listeners_answer_with_the_listening_table() {
    if !enabled() {
        return;
    }
    // The owning process of another user's listener is measured by the shell rows of
    // deploy/test/identities.sh, which create the probe account; this checks the typed answer.
    let answer = ask("net_listeners", "");
    assert_eq!(result_of(&answer), "net_listeners", "{answer}");
}

#[test]
#[ignore = "needs root, useradd and systemd as PID 1"]
fn helper_docker_read_works_through_the_real_socket_as_domovoi_agent() {
    if !enabled() {
        return;
    }
    if !std::path::Path::new("/var/run/docker.sock").exists() {
        eprintln!("skipped: no Docker socket on this host");
        return;
    }
    let (opened, _) = as_agent(&[
        "python3",
        "-c",
        "import socket\ns = socket.socket(socket.AF_UNIX)\ns.connect('/var/run/docker.sock')",
    ]);
    assert!(!opened, "the agent account can open the Docker socket");
    assert_eq!(result_of(&ask("docker_list", "")), "docker_containers");
}
