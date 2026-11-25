use domovoi_core::helper::read::{MAX_LISTENERS, ReadToken, WireProto};
use domovoi_core::identity::HostDirs;
use domovoi_helper::reads::tools::{ToolEnv, net};
use domovoi_testkit::FakeProcessExecutor;

use crate::support::{exited, healthy, ok};

const SS: &str = "/usr/bin/ss";
const ARGS: [&str; 6] = [
    "--no-header",
    "--listening",
    "--numeric",
    "--tcp",
    "--udp",
    "--processes",
];

async fn run(
    output: domovoi_core::ports::ProcessOutput,
) -> Result<
    (Vec<domovoi_core::helper::read::ListenerWire>, bool),
    domovoi_core::helper::read::ReadFailure,
> {
    let inspect = healthy(&HostDirs::default());
    let exec = FakeProcessExecutor::new();
    exec.expect(SS, &ARGS, output);
    net::listeners(ToolEnv {
        inspect: &inspect,
        exec: &exec,
    })
    .await
}

#[tokio::test]
async fn net_listeners_parse_tcp_udp_ipv4_ipv6_and_users() {
    let text = "\
udp UNCONN 0 0 127.0.0.53%lo:53 0.0.0.0:* users:((\"systemd-resolve\",pid=612,fd=13))
tcp LISTEN 0 4096 0.0.0.0:22 0.0.0.0:* users:((\"sshd\",pid=1,fd=3),(\"other\",pid=2,fd=4))
tcp LISTEN 0 128 [::]:443 [::]:*
tcp LISTEN 0 4096 [::1]:631 [::]:* users:((\"cupsd\",pid=77,fd=7))
";
    let (listeners, truncated) = run(ok(text)).await.expect("parse");
    assert!(!truncated);
    insta::assert_debug_snapshot!("net_listeners", listeners);
    assert_eq!(listeners[1].proto, WireProto::Tcp);
    assert_eq!(listeners[1].pid, Some(1));
    assert_eq!(listeners[0].addr, "127.0.0.53");
    assert_eq!(listeners[2].process, None);
}

#[tokio::test]
async fn net_listeners_sanitise_process_names() {
    let long = "x".repeat(100);
    let text = format!(
        "tcp LISTEN 0 1 0.0.0.0:80 0.0.0.0:* users:((\"caf\u{e9}\",pid=5,fd=3))\n\
         tcp LISTEN 0 1 0.0.0.0:81 0.0.0.0:* users:((\"{long}\",pid=6,fd=3))\n"
    );
    let (listeners, _) = run(ok(text)).await.expect("parse");
    assert_eq!(listeners[0].process.as_deref(), Some("caf??"));
    assert_eq!(listeners[1].process.as_deref().map(str::len), Some(64));
}

#[tokio::test]
async fn net_listeners_skip_unparsable_lines_and_fail_when_none_parse() {
    let text = "garbage\ntcp LISTEN 0 1 0.0.0.0:80 0.0.0.0:*\ntcp LISTEN 0 1 nonsense 0.0.0.0:*\n";
    let (listeners, _) = run(ok(text)).await.expect("one parses");
    assert_eq!(listeners.len(), 1);
    let err = run(ok("garbage\nmore garbage\n")).await.expect_err("fails");
    assert_eq!(err.token, ReadToken::OutputMalformed);
    assert!(run(ok("")).await.expect("empty is fine").0.is_empty());
    let err = run(exited(1, "", "ss: failed")).await.expect_err("fails");
    assert_eq!(err.token, ReadToken::CommandFailed);
}

#[tokio::test]
async fn net_listeners_are_capped_and_marked_truncated() {
    let text: String = (0..=MAX_LISTENERS)
        .map(|i| format!("tcp LISTEN 0 1 0.0.0.0:{} 0.0.0.0:*\n", 1 + i % 60000))
        .collect();
    let (listeners, truncated) = run(ok(text)).await.expect("parse");
    assert_eq!(listeners.len(), MAX_LISTENERS);
    assert!(truncated);
}
