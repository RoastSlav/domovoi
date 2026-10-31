//! The correlation rules, one by one and against each other.

use domovoi_core::{HostId, Incident, IncidentEvent, IncidentPolicy, apply};
use domovoi_engine::EnginePolicy;
use domovoi_engine::correlate::{Candidates, Target, correlate};
use domovoi_testkit::build::at;

use crate::harness::{Sig, kuma};

fn open(sig: &Sig, secs: i64) -> Incident {
    let signal = sig.clone().immediate().firing(secs);
    let mut incident = Incident::new_pending(&signal, at(secs));
    let event = IncidentEvent::Firing {
        severity: signal.severity(),
        class: signal.class(),
        observed_at: at(secs),
        value: None,
    };
    apply(&mut incident, event, &IncidentPolicy::default(), at(secs)).unwrap();
    incident
}

fn resolved(sig: &Sig, secs: i64) -> Incident {
    let mut incident = open(sig, 0);
    let event = IncidentEvent::Resolved {
        observed_at: at(secs),
    };
    apply(&mut incident, event, &IncidentPolicy::default(), at(secs)).unwrap();
    incident
}

fn pick(
    sig: &Sig,
    firing: bool,
    active: &[Incident],
    recent: Option<&Incident>,
    secs: i64,
) -> Target {
    let signal = if firing {
        sig.firing(secs)
    } else {
        sig.resolved(secs)
    };
    let candidates = Candidates {
        active,
        recent_resolved: recent,
    };
    correlate(&signal, &candidates, &EnginePolicy::default(), at(secs))
}

#[test]
fn each_rule_and_its_precedence() {
    let host = HostId::new();
    let disk = Sig::new(host, "disk.free", Some("/"));
    let web = Sig::new(host, "uptime.down", Some("https://plex.example:32400")).source(kuma());
    let container = Sig::new(host, "container.unhealthy", Some("media_plex_1"));

    // 5. Nothing to match: new. 4. A resolved signal with nothing active: ignore.
    assert_eq!(pick(&disk, true, &[], None, 10), Target::New);
    assert_eq!(pick(&disk, false, &[], None, 10), Target::Ignore);

    // 1. Same fingerprint, active: existing, for firing and resolved signals alike.
    let disk_open = open(&disk, 0);
    let active = [disk_open.clone()];
    assert_eq!(
        pick(&disk, true, &active, None, 10),
        Target::Existing(disk_open.id())
    );
    assert_eq!(
        pick(&disk, false, &active, None, 10),
        Target::Existing(disk_open.id())
    );

    // 2. Resolved within the reopen window (6 h): reopen; after it: new.
    let disk_done = resolved(&disk, 100);
    assert_eq!(
        pick(&disk, true, &[], Some(&disk_done), 100 + 3600),
        Target::Reopen(disk_done.id())
    );
    assert_eq!(
        pick(&disk, true, &[], Some(&disk_done), 100 + 6 * 3600),
        Target::Reopen(disk_done.id())
    );
    assert_eq!(
        pick(&disk, true, &[], Some(&disk_done), 101 + 6 * 3600),
        Target::New
    );
    // A resolved signal never reopens anything.
    assert_eq!(
        pick(&disk, false, &[], Some(&disk_done), 200),
        Target::Ignore
    );

    // 3. Same normalised target, other source, same host, within 5 minutes: grouped.
    let web_open = open(&web, 0);
    let active = [web_open.clone()];
    assert_eq!(
        pick(&container, true, &active, None, 60),
        Target::Existing(web_open.id())
    );
    assert_eq!(
        pick(&container, true, &active, None, 301),
        Target::New,
        "outside the window"
    );
    // A resolved signal does not join a group it was never part of.
    assert_eq!(pick(&container, false, &active, None, 60), Target::Ignore);
    // The same source does not group: two agent checks are separate problems.
    let restart = Sig::new(host, "container.restart_loop", Some("plex"));
    let agent_open = open(&container, 0);
    assert_eq!(pick(&restart, true, &[agent_open], None, 60), Target::New);
    // Another host does not group.
    let elsewhere = Sig::new(HostId::new(), "container.unhealthy", Some("plex"));
    assert_eq!(pick(&elsewhere, true, &active, None, 60), Target::New);

    // 1 before 3: an active incident of its own wins over grouping.
    let own = open(&container, 30);
    let both = [web_open.clone(), own.clone()];
    assert_eq!(
        pick(&container, true, &both, None, 60),
        Target::Existing(own.id())
    );
    // 2 before 3: reopening its own incident wins over grouping.
    let own_done = resolved(&container, 50);
    assert_eq!(
        pick(&container, true, &active, Some(&own_done), 60),
        Target::Reopen(own_done.id())
    );
    // An incident stored before sources were kept never groups.
    let mut legacy = serde_json::to_value(&web_open).unwrap();
    legacy.as_object_mut().unwrap().remove("source");
    let legacy: Incident = serde_json::from_value(legacy).unwrap();
    assert_eq!(pick(&container, true, &[legacy], None, 60), Target::New);
}
