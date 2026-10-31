//! Command templates, the runtime bound, the registry and the real host filesystem.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use domovoi_checks::commands::{
    CommandError, Commands, DevicePath, JailName, JournalCursor, JournalScope, SyslogIdentifier,
    UnitName, UnitProperty,
};
use domovoi_checks::containers::{ContainerDeps, ContainerPolicy, DockerSource};
use domovoi_checks::framework::{
    Check, CheckContext, CheckError, CheckReport, CheckSettings, HostDeps, Registry,
};
use domovoi_checks::hostfs::RealHostFs;
use domovoi_checks::system::RealFsStats;
use domovoi_core::ports::{FsError, HostFs};
use domovoi_core::{CheckKey, SignalClass};
use jiff::SignedDuration;

use crate::harness::{Harness, args};

#[tokio::test]
async fn templates_put_every_slot_in_its_own_argv_element_and_reject_invalid_slots() {
    let h = Harness::new("tools").await;
    let c = &h.commands;
    let unit = UnitName::parse("docker.service").unwrap();

    let spec = c.journal(Some(&unit), 30, 100, false).unwrap();
    assert_eq!(spec.program, "/usr/bin/journalctl");
    assert_eq!(
        args(&spec),
        [
            "--no-pager",
            "--quiet",
            "--output=short-iso",
            "--since=-30min",
            "--lines=100",
            "--unit",
            "docker.service"
        ]
    );
    assert!(spec.env.is_empty() && spec.cwd.is_none() && spec.run_as.is_none());
    assert_eq!(spec.max_output_bytes, 2 * 1024 * 1024);

    let cursor = JournalCursor::parse("s=abc;i=1f;b=x").unwrap();
    let spec = c
        .journal_since_cursor(
            Some(&cursor),
            JournalScope::Units(&[unit.clone(), UnitName::parse("ssh@1.service").unwrap()]),
        )
        .unwrap();
    assert_eq!(
        args(&spec),
        [
            "--no-pager",
            "--quiet",
            "--output=cat",
            "--show-cursor",
            "--after-cursor=s=abc;i=1f;b=x",
            "--unit",
            "docker.service",
            "--unit",
            "ssh@1.service"
        ]
    );

    let device = DevicePath::parse("/dev/nvme0n1").unwrap();
    assert_eq!(
        args(&c.smartctl_all(&device).unwrap()),
        ["--all", "--json", "--nocheck=standby", "/dev/nvme0n1"]
    );
    assert_eq!(
        c.smartctl_all(&device).unwrap().program,
        "/usr/sbin/smartctl"
    );
    assert_eq!(args(&c.smartctl_scan().unwrap()), ["--scan-open", "--json"]);
    let props = [
        UnitProperty::parse("ActiveState").unwrap(),
        UnitProperty::parse("SubState").unwrap(),
    ];
    assert_eq!(
        args(&c.systemctl_show(&unit, &props).unwrap()),
        [
            "show",
            "--no-pager",
            "--property=ActiveState,SubState",
            "--",
            "docker.service"
        ]
    );
    assert_eq!(
        args(&c.systemctl_failed().unwrap()),
        [
            "list-units",
            "--state=failed",
            "--no-legend",
            "--plain",
            "--no-pager"
        ]
    );
    let jail = JailName::parse("sshd").unwrap();
    assert_eq!(args(&c.fail2ban_status(&jail).unwrap()), ["status", "sshd"]);

    for bad in ["", "-x", "a b", "a;b", "a/b", "$(id)", &"a".repeat(129)] {
        assert!(UnitName::parse(bad).is_err(), "unit {bad:?}");
    }
    for bad in ["", "-f", "a.b", "a b"] {
        assert!(JailName::parse(bad).is_err(), "jail {bad:?}");
    }
    for bad in [
        "/dev/",
        "/dev/SDA",
        "/dev/../sda",
        "/dev/sda1;x",
        "sda",
        "/tmp/x",
    ] {
        assert!(DevicePath::parse(bad).is_err(), "device {bad:?}");
    }
    for bad in ["", "a b", "-x"] {
        assert!(JournalCursor::parse(bad).is_err(), "cursor {bad:?}");
    }
    assert!(UnitProperty::parse("Active=State").is_err());
    assert!(matches!(
        c.journal(None, 0, 10, true),
        Err(CommandError::InvalidSlot { .. })
    ));
    assert!(matches!(
        c.journal(None, 10, 501, true),
        Err(CommandError::InvalidSlot { .. })
    ));
    assert!(c.systemctl_show(&unit, &[]).is_err());

    let none = Commands::default();
    assert_eq!(
        none.smartctl_scan(),
        Err(CommandError::NotInstalled("smartctl"))
    );
    assert_eq!(
        none.fail2ban_status(&jail),
        Err(CommandError::NotInstalled("fail2ban-client"))
    );
}

#[tokio::test]
async fn journal_scopes_never_mix_units_and_identifiers() {
    let h = Harness::new("tools").await;
    let c = &h.commands;
    let sudo = SyslogIdentifier::parse("sudo").unwrap();
    let spec = c
        .journal_since_cursor(None, JournalScope::Identifiers(&[sudo]))
        .unwrap();
    assert_eq!(
        args(&spec),
        [
            "--no-pager",
            "--quiet",
            "--output=cat",
            "--show-cursor",
            "--since=-15min",
            "--identifier",
            "sudo"
        ]
    );
    let kernel = c.journal_since_cursor(None, JournalScope::Kernel).unwrap();
    assert_eq!(args(&kernel).last(), Some(&"--dmesg"));
    assert!(
        c.journal_since_cursor(None, JournalScope::Units(&[]))
            .is_err()
    );
    assert!(
        c.journal_since_cursor(None, JournalScope::Identifiers(&[]))
            .is_err()
    );
    for bad in ["", "-t", "a b", "a/b", &"a".repeat(65)] {
        assert!(SyslogIdentifier::parse(bad).is_err(), "identifier {bad:?}");
    }
    assert_eq!(
        args(&c.systemctl_failed_json().unwrap()).last(),
        Some(&"--output=json")
    );
}

struct Slow;

#[async_trait]
impl Check for Slow {
    fn key(&self) -> CheckKey {
        CheckKey::parse("test.slow").unwrap()
    }
    fn default_cadence(&self) -> SignedDuration {
        SignedDuration::from_secs(60)
    }
    fn class(&self) -> SignalClass {
        SignalClass::Normal
    }
    async fn run(&self, _ctx: &mut CheckContext<'_>) -> Result<CheckReport, CheckError> {
        std::future::pending().await
    }
}

#[tokio::test]
async fn slow_check_times_out_on_the_fake_clock() {
    let mut h = Harness::new("healthy").await;
    let clock = h.clock.clone();
    let (result, ()) = tokio::join!(h.try_run(&Slow), async move {
        tokio::task::yield_now().await;
        clock.advance(SignedDuration::from_secs(19));
        tokio::task::yield_now().await;
        clock.advance(SignedDuration::from_secs(2));
    });
    assert_eq!(result, Err(CheckError::Timeout));
}

#[test]
fn registry_builds_enabled_checks_in_key_order() {
    let deps = HostDeps {
        fs_stats: Arc::new(RealFsStats::new("/")),
        containers: None,
    };
    let all = Registry::build(&BTreeMap::new(), deps.clone()).unwrap();
    let keys: Vec<String> = all.checks().map(|c| c.key().to_string()).collect();
    assert_eq!(
        keys,
        [
            "disk.free",
            "kernel.errors",
            "memory.available",
            "power.supply",
            "security.fail2ban",
            "security.new_port",
            "security.ssh_login",
            "security.sudo_failure",
            "smart.health",
            "system.reboot",
            "system.reboot_required",
            "temperature.cpu",
            "unit.failed",
            "unit.watch"
        ]
    );
    let off = CheckSettings {
        enabled: false,
        ..CheckSettings::default()
    };
    let memory = CheckKey::parse("memory.available").unwrap();
    let some = Registry::build(&BTreeMap::from([(memory.clone(), off)]), deps.clone()).unwrap();
    assert_eq!(some.checks().count(), 13);
    assert!(some.get(&memory).is_none());
    assert!(some.get(&CheckKey::parse("disk.free").unwrap()).is_some());

    // With Docker enabled, the four container checks join in key order.
    let docker = DockerSource::connect("tcp://127.0.0.1:2375").unwrap();
    let with_docker = HostDeps {
        containers: Some(ContainerDeps {
            source: Arc::new(docker),
            policy: ContainerPolicy::default(),
        }),
        ..deps
    };
    let all = Registry::build(&BTreeMap::new(), with_docker).unwrap();
    let keys: Vec<String> = all.checks().map(|c| c.key().to_string()).collect();
    assert_eq!(keys.len(), 18);
    assert_eq!(
        &keys[..4],
        [
            "container.expected_down",
            "container.new",
            "container.restart_loop",
            "container.unhealthy"
        ]
    );
}

#[tokio::test]
async fn real_hostfs_rejects_relative_and_dotdot_paths() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("proc")).unwrap();
    std::fs::write(dir.path().join("proc/meminfo"), "MemTotal: 1 kB\n").unwrap();
    std::fs::write(dir.path().join("big"), vec![b'a'; 2 * 1024 * 1024 + 1]).unwrap();
    let fs = RealHostFs::new(dir.path());

    assert_eq!(
        fs.read_to_string("/proc/meminfo").await.unwrap(),
        "MemTotal: 1 kB\n"
    );
    assert_eq!(fs.read_dir("/").await.unwrap(), ["big", "proc"]);
    assert!(fs.exists("/proc/meminfo").await);
    assert_eq!(
        fs.read_to_string("proc/meminfo").await,
        Err(FsError::InvalidPath)
    );
    assert_eq!(
        fs.read_to_string("/proc/../big").await,
        Err(FsError::InvalidPath)
    );
    assert!(!fs.exists("/proc/../big").await);
    assert_eq!(fs.read_to_string("/missing").await, Err(FsError::NotFound));
    assert!(matches!(fs.read_to_string("/big").await, Err(FsError::Io(m)) if m.contains("larger")));
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn real_fs_stats_reads_the_root_filesystem() {
    use domovoi_checks::system::FsStats;

    let usage = RealFsStats::new("/").usage("/").await.unwrap();
    assert!(usage.total_bytes > 0);
    assert!(usage.available_bytes <= usage.total_bytes);
    assert!(
        RealFsStats::new("/")
            .usage("/nonexistent/mount")
            .await
            .is_err()
    );
}
