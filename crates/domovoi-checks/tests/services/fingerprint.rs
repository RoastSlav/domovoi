//! The fingerprint rule: targets name things, never counts, measurements or times.

use std::collections::BTreeSet;
use std::sync::Arc;

use domovoi_checks::containers::{
    ContainerPolicy, ExpectedContainers, Health, NewContainers, RestartLoop, UnhealthyContainers,
};
use domovoi_checks::framework::{Check, CheckReport};
use domovoi_checks::security::{Fail2banBans, NewPorts, SshLogins, SudoFailures};
use domovoi_checks::smart::SmartHealth;
use domovoi_checks::units::{FailedUnits, WatchedUnits};
use jiff::SignedDuration;

use crate::harness::{FakeDocker, Harness, container, deps, output, read_fixture, stopped};

/// Every target in `reports` must be one of `allowed`, which lists the names, devices,
/// users, jails and ports in the fixtures. A count or a timestamp could never be in it.
fn assert_targets(check: &dyn Check, reports: &[CheckReport], allowed: &[&str]) {
    let allowed: BTreeSet<&str> = allowed.iter().copied().collect();
    let mut seen = 0;
    for report in reports {
        for obs in &report.observations {
            let target = obs.target.as_ref().map_or("", |t| t.as_str());
            assert!(
                allowed.contains(target),
                "{}: target {target:?} is not a name from the fixtures",
                check.key()
            );
            seen += 1;
        }
    }
    assert!(
        seen > 0,
        "{}: the fixtures produced no observations",
        check.key()
    );
}

async fn twice(h: &mut Harness, check: &dyn Check) -> Vec<CheckReport> {
    let first = h.run(check).await;
    h.advance(SignedDuration::from_mins(10));
    vec![first, h.run(check).await]
}

#[tokio::test]
async fn targets_never_contain_counts_or_timestamps() {
    // Containers: restart counts, health and state change between runs.
    let docker = Arc::new(FakeDocker::default());
    let mut sick = container("plex", true);
    sick.health = Some(Health::Unhealthy);
    sick.restart_count = 7;
    docker.set(vec![sick.clone(), container("db", true)]);
    let names = ["plex", "db", "adhoc"];
    let checks: Vec<Box<dyn Check>> = vec![
        Box::new(ExpectedContainers::new(deps(&docker, ContainerPolicy::default())).unwrap()),
        Box::new(UnhealthyContainers::new(deps(&docker, ContainerPolicy::default())).unwrap()),
        Box::new(RestartLoop::new(deps(&docker, ContainerPolicy::default())).unwrap()),
        Box::new(NewContainers::new(deps(&docker, ContainerPolicy::default())).unwrap()),
    ];
    for check in &checks {
        docker.set(vec![sick.clone(), container("db", true)]);
        let mut h = Harness::new("proc/ports_base").await;
        let mut reports = twice(&mut h, check.as_ref()).await;
        let mut restarted = sick.clone();
        restarted.restart_count = 12;
        docker.set(vec![
            stopped(restarted),
            container("db", true),
            container("adhoc", false),
        ]);
        h.advance(SignedDuration::from_mins(10));
        reports.push(h.run(check.as_ref()).await);
        assert_targets(check.as_ref(), &reports, &names);
    }

    // Units.
    let mut h = Harness::new("proc/ports_base").await;
    h.exec.expect(
        "/usr/bin/systemctl",
        &[
            "list-units",
            "--state=failed",
            "--no-legend",
            "--plain",
            "--no-pager",
            "--output=json",
        ],
        output(&read_fixture("systemd/failed.json")),
    );
    let check = FailedUnits::new().unwrap();
    let reports = twice(&mut h, &check).await;
    assert_targets(&check, &reports, &["backup.service", "certbot.timer"]);

    let mut h = Harness::new("proc/ports_base")
        .await
        .with_thresholds(serde_json::json!({ "units": ["docker.service"] }));
    h.exec.expect(
        "/usr/bin/systemctl",
        &[
            "show",
            "--no-pager",
            "--property=ActiveState,SubState,NRestarts,Result",
            "--",
            "docker.service",
        ],
        output("ActiveState=failed\nNRestarts=17\n"),
    );
    let check = WatchedUnits::new().unwrap();
    let reports = twice(&mut h, &check).await;
    assert_targets(
        &check,
        &reports,
        &["docker.service", "docker.service:restarts"],
    );

    // SMART: counters change, targets stay device:counter.
    let mut h = Harness::new("proc/ports_base").await;
    let smartctl = "/usr/sbin/smartctl";
    h.exec.expect(
        smartctl,
        &["--scan-open", "--json"],
        output(&read_fixture("smart/scan.json")),
    );
    h.exec.expect(
        smartctl,
        &["--all", "--json", "--nocheck=standby", "/dev/sda"],
        output(&read_fixture("smart/ata.json")),
    );
    h.exec.expect(
        smartctl,
        &["--all", "--json", "--nocheck=standby", "/dev/nvme0"],
        output(&read_fixture("smart/nvme_critical.json")),
    );
    let check = SmartHealth::new().unwrap();
    let mut reports = vec![h.run(&check).await];
    h.exec.expect(
        smartctl,
        &["--all", "--json", "--nocheck=standby", "/dev/sda"],
        output(&read_fixture("smart/ata_increased.json")),
    );
    reports.push(h.run(&check).await);
    assert_targets(
        &check,
        &reports,
        &[
            "/dev/sda",
            "/dev/sda:reallocated_sector_ct",
            "/dev/sda:reported_uncorrect",
            "/dev/sda:current_pending_sector",
            "/dev/sda:offline_uncorrectable",
            "/dev/sda:udma_crc_error_count",
            "/dev/nvme0",
            "/dev/nvme0:media_errors",
        ],
    );

    // Security: users, jails and listeners.
    let journal = "/usr/bin/journalctl";
    let since = [
        "--no-pager",
        "--quiet",
        "--output=cat",
        "--show-cursor",
        "--since=-15min",
    ];
    let mut h = Harness::new("proc/ports_base").await;
    let ssh: Vec<&str> = since
        .iter()
        .copied()
        .chain(["--unit", "ssh.service", "--unit", "sshd.service"])
        .collect();
    h.exec
        .expect(journal, &ssh, output(&read_fixture("journal/ssh.txt")));
    let check = SshLogins::new().unwrap();
    assert_targets(&check, &[h.run(&check).await], &["alice", "bob", "carol"]);

    let mut h = Harness::new("proc/ports_base").await;
    let sudo: Vec<&str> = since
        .iter()
        .copied()
        .chain(["--identifier", "sudo"])
        .collect();
    h.exec
        .expect(journal, &sudo, output(&read_fixture("journal/sudo.txt")));
    let check = SudoFailures::new().unwrap();
    assert_targets(&check, &[h.run(&check).await], &["dave"]);

    let mut h = Harness::new("proc/ports_base")
        .await
        .with_thresholds(serde_json::json!({ "jails": ["sshd"], "bans_per_hour": 0 }));
    h.exec.expect(
        "/usr/bin/fail2ban-client",
        &["status", "sshd"],
        output(&read_fixture("systemd/fail2ban_sshd.txt")),
    );
    let check = Fail2banBans::new().unwrap();
    let reports = twice(&mut h, &check).await;
    assert_targets(&check, &reports, &["sshd"]);

    let mut h = Harness::new("proc/ports_base").await;
    let check = NewPorts::new().unwrap();
    h.run(&check).await;
    h.use_fs("proc/ports_new");
    let reports = twice(&mut h, &check).await;
    assert_targets(&check, &reports, &["0.0.0.0:8080/tcp"]);
}
