//! An agent in a container runs the tools in its own filesystem: smartctl and
//! fail2ban-client work, while journal and systemd checks are unavailable, not errors.

use domovoi_checks::commands::{CommandError, Commands, ToolHost};
use domovoi_checks::framework::{Check, CheckError};
use domovoi_checks::security::{SshLogins, SudoFailures};
use domovoi_checks::smart::SmartHealth;
use domovoi_checks::system::KernelErrors;
use domovoi_checks::units::FailedUnits;
use domovoi_testkit::FixtureHostFs;

use crate::harness::{Harness, fixture, output};

async fn assert_unavailable(h: &mut Harness, check: &dyn Check, tool: &str) {
    let err = h.try_run(check).await.unwrap_err();
    assert!(err.is_unavailable(), "{err:?}");
    assert!(
        matches!(&err, CheckError::Command(CommandError::Unavailable { tool: t, .. }) if *t == tool),
        "{err:?}"
    );
    assert!(err.to_string().contains("container"), "{err}");
}

#[tokio::test]
async fn journal_and_systemd_checks_are_unavailable_and_run_nothing() {
    let mut h = Harness::in_container("proc/ports_base").await;
    assert_unavailable(&mut h, &SshLogins::new().unwrap(), "journalctl").await;
    assert_unavailable(&mut h, &SudoFailures::new().unwrap(), "journalctl").await;
    assert_unavailable(&mut h, &KernelErrors::new().unwrap(), "journalctl").await;
    assert_unavailable(&mut h, &FailedUnits::new().unwrap(), "systemctl").await;
    assert!(h.exec.calls().is_empty());
}

#[tokio::test]
async fn smartctl_in_the_agent_filesystem_runs() {
    let mut h = Harness::in_container("proc/ports_base").await;
    h.exec.expect(
        "/usr/sbin/smartctl",
        &["--scan-open", "--json"],
        output(r#"{"devices": []}"#),
    );
    let report = h.run(&SmartHealth::new().unwrap()).await;
    assert!(report.observations.is_empty());
    assert_eq!(h.exec.calls().len(), 1);
}

#[tokio::test]
async fn tools_are_looked_up_in_the_filesystem_given_not_the_host() {
    // The host tree has no tools; the agent's own tree has them all.
    let host_only = FixtureHostFs::new(fixture("proc/ports_base"));
    let own = FixtureHostFs::new(fixture("system/tools"));

    let missing = Commands::resolve(&host_only, ToolHost::Container).await;
    let err = missing.smartctl_scan().unwrap_err();
    assert_eq!(err, CommandError::NotInstalled("smartctl"));
    assert!(err.is_unavailable());

    let found = Commands::resolve(&own, ToolHost::Container).await;
    assert_eq!(found.smartctl_scan().unwrap().program, "/usr/sbin/smartctl");
    assert!(matches!(
        found.systemctl_failed(),
        Err(CommandError::Unavailable {
            tool: "systemctl",
            ..
        })
    ));

    let on_host = Commands::resolve(&own, ToolHost::Host).await;
    assert_eq!(
        on_host.systemctl_failed().unwrap().program,
        "/usr/bin/systemctl"
    );
    assert_eq!(
        on_host.journal(None, 10, 10, true).unwrap().program,
        "/usr/bin/journalctl"
    );
}
