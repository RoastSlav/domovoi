//! SSH logins, sudo failures, fail2ban and listening ports.

use std::net::IpAddr;

use domovoi_checks::security::{
    Fail2banBans, Network, NewPorts, SshLogins, SudoFailures, classify_ip, parse_accepted,
    parse_sudo_failure,
};
use domovoi_core::Severity;
use jiff::SignedDuration;

use crate::harness::{
    Harness, assert_firing, assert_ok, find, is_immediate, number, output, read_fixture, targets,
    text,
};

const JOURNAL: &str = "/usr/bin/journalctl";
const FAIL2BAN: &str = "/usr/bin/fail2ban-client";

fn journal_args<'a>(scope: &[&'a str]) -> Vec<&'a str> {
    let mut args = vec![
        "--no-pager",
        "--quiet",
        "--output=cat",
        "--show-cursor",
        "--since=-15min",
    ];
    args.extend_from_slice(scope);
    args
}

#[tokio::test]
async fn wan_ssh_login_is_warning_and_lan_is_info() {
    let check = SshLogins::new().unwrap();
    let mut h = Harness::new("proc/ports_base").await;
    h.exec.expect(
        JOURNAL,
        &journal_args(&["--unit", "ssh.service", "--unit", "sshd.service"]),
        output(&read_fixture("journal/ssh.txt")),
    );
    let report = h.run(&check).await;
    assert_eq!(targets(&report), ["alice", "bob", "carol"]);
    assert_firing(&report, "alice", Severity::Info);
    assert_firing(&report, "carol", Severity::Info);
    assert_firing(&report, "bob", Severity::Warning);

    let bob = find(&report, "bob");
    assert_eq!(text(bob, "ip"), "203.0.113.7");
    assert_eq!(text(bob, "method"), "password");
    assert_eq!(text(bob, "network"), "wan");
    assert_eq!(number(bob, "port"), 40022.0);
    assert_eq!(number(bob, "count"), 2.0);
    assert!(is_immediate(bob, &check));
    assert_eq!(text(find(&report, "alice"), "network"), "lan");
    assert_eq!(h.state.get("cursor"), Some("s=ssh;i=10"));

    assert_eq!(
        parse_accepted("Accepted publickey for alice from 10.1.2.3 port 22 ssh2")
            .unwrap()
            .user,
        "alice"
    );
    assert!(parse_accepted("Failed password for root from 1.2.3.4 port 22 ssh2").is_none());
}

#[tokio::test]
async fn sudo_failure_detected() {
    let check = SudoFailures::new().unwrap();
    let mut h = Harness::new("proc/ports_base").await;
    h.exec.expect(
        JOURNAL,
        &journal_args(&["--identifier", "sudo"]),
        output(&read_fixture("journal/sudo.txt")),
    );
    let report = h.run(&check).await;
    assert_eq!(targets(&report), ["dave"]);
    assert_firing(&report, "dave", Severity::Warning);
    assert_eq!(number(find(&report, "dave"), "count"), 2.0);
    assert_eq!(h.state.get("cursor"), Some("s=sudo;i=4"));

    assert_eq!(
        parse_sudo_failure("sudo: frank : 1 incorrect password attempt ; TTY=pts/2"),
        Some("frank".to_owned())
    );
    assert_eq!(
        parse_sudo_failure("erin : TTY=pts/1 ; PWD=/home/erin ; COMMAND=/bin/true"),
        None
    );
}

#[tokio::test]
async fn fail2ban_counts_are_evidence_only() {
    let check = Fail2banBans::new().unwrap();
    let mut h = Harness::new("proc/ports_base")
        .await
        .with_thresholds(serde_json::json!({ "jails": ["sshd"] }));
    let status = read_fixture("systemd/fail2ban_sshd.txt");
    h.exec
        .expect(FAIL2BAN, &["status", "sshd"], output(&status));
    let first = h.run(&check).await;
    assert_ok(&first, "sshd");
    assert_eq!(number(find(&first, "sshd"), "total_banned"), 100.0);
    assert_eq!(number(find(&first, "sshd"), "currently_banned"), 4.0);

    h.advance(SignedDuration::from_mins(5));
    h.exec.expect(
        FAIL2BAN,
        &["status", "sshd"],
        output(&status.replace("Total banned:\t100", "Total banned:\t130")),
    );
    let busy = h.run(&check).await;
    assert_eq!(targets(&busy), ["sshd"]);
    assert_firing(&busy, "sshd", Severity::Info);
    assert_eq!(number(find(&busy, "sshd"), "bans_last_hour"), 30.0);

    // Without jails the check reads nothing.
    let mut h = Harness::new("proc/ports_base").await;
    assert!(h.run(&check).await.observations.is_empty());
    assert!(h.exec.calls().is_empty());
}

#[tokio::test]
async fn new_listening_port_fires_after_baseline() {
    let check = NewPorts::new().unwrap();
    let mut h = Harness::new("proc/ports_base").await;
    assert!(h.run(&check).await.observations.is_empty());
    h.advance(SignedDuration::from_mins(1));
    assert!(h.run(&check).await.observations.is_empty());

    h.use_fs("proc/ports_new");
    h.advance(SignedDuration::from_mins(1));
    let report = h.run(&check).await;
    assert_eq!(targets(&report), ["0.0.0.0:8080/tcp"]);
    let obs = find(&report, "0.0.0.0:8080/tcp");
    assert_firing(&report, "0.0.0.0:8080/tcp", Severity::Warning);
    assert!(is_immediate(obs, &check));
    assert_eq!(number(obs, "port"), 8080.0);

    h.advance(SignedDuration::from_mins(1));
    assert!(h.run(&check).await.observations.is_empty());
}

#[tokio::test]
async fn port_baseline_expires_after_expiry_days() {
    let check = NewPorts::new().unwrap();
    let mut h = Harness::new("proc/ports_new").await;
    h.run(&check).await;
    h.use_fs("proc/ports_base");
    h.advance(SignedDuration::from_hours(24 * 8));
    assert!(h.run(&check).await.observations.is_empty());
    h.use_fs("proc/ports_new");
    h.advance(SignedDuration::from_mins(1));
    assert_eq!(targets(&h.run(&check).await), ["0.0.0.0:8080/tcp"]);
}

#[tokio::test]
async fn listeners_skip_loopback_and_ephemeral_udp() {
    let found = domovoi_checks::security::ports::listeners(&domovoi_testkit::FixtureHostFs::new(
        crate::harness::fixture("proc/ports_base"),
    ))
    .await;
    let names: Vec<String> = found.iter().map(ToString::to_string).collect();
    assert_eq!(names, ["0.0.0.0:22/tcp", "0.0.0.0:53/udp", "[::]:443/tcp"]);
}

#[test]
fn ip_classification_table() {
    let cases = [
        ("10.0.0.1", Network::Lan),
        ("172.16.5.4", Network::Lan),
        ("172.32.0.1", Network::Wan),
        ("192.168.1.20", Network::Lan),
        ("127.0.0.1", Network::Lan),
        ("169.254.10.1", Network::Lan),
        ("100.64.0.1", Network::Lan),
        ("100.127.255.254", Network::Lan),
        ("100.128.0.1", Network::Wan),
        ("8.8.8.8", Network::Wan),
        ("203.0.113.7", Network::Wan),
        ("::1", Network::Lan),
        ("fe80::1", Network::Lan),
        ("fd12:3456::7", Network::Lan),
        ("fc00::1", Network::Lan),
        ("::ffff:192.168.1.5", Network::Lan),
        ("::ffff:8.8.4.4", Network::Wan),
        ("2001:db8::1", Network::Wan),
        ("2a00:1450:4001::200e", Network::Wan),
    ];
    for (ip, expected) in cases {
        let parsed: IpAddr = ip.parse().unwrap();
        assert_eq!(classify_ip(parsed), expected, "{ip}");
    }
}
