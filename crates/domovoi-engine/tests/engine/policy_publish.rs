//! Publication review: what an owner must acknowledge before a draft becomes the policy.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use domovoi_core::HostId;
use domovoi_core::capability::{CeilingEntry, CeilingSummary, CeilingTargets};
use domovoi_engine::policy::{
    FindingKind, Outcome, Outcome::Allow, Outcome::Deny, Outcome::RequireApproval,
    PublicationReport, Rule, validate_publication,
};

use crate::policy_fixtures::*;

fn review(fx: &Fx, rules: &[Rule]) -> PublicationReport {
    validate_publication(rules, &fx.index, &BTreeMap::new()).unwrap()
}

fn summary(contract: &str, targets: CeilingTargets, enabled: bool) -> CeilingSummary {
    CeilingSummary {
        enabled,
        entries: vec![CeilingEntry {
            contract: cid(contract),
            targets,
            max_per_hour: 4,
            max_active: None,
            max_ttl: None,
            args: BTreeMap::new(),
        }],
        hash: None,
    }
}

fn pair(fx: &Fx, second: Outcome) -> [Rule; 2] {
    [
        rule("a", Allow).contract(RESTART).named(&["plex"]),
        rule("b", second).on_host(fx.host).category(LIFECYCLE),
    ]
}

#[test]
fn publish_reports_incomparable_conflicts() {
    let fx = Fx::new();
    let report = review(&fx, &pair(&fx, Deny));
    assert_eq!(report.findings.len(), 1);
    let f = &report.findings[0];
    assert_eq!(f.kind, FindingKind::IncomparableConflict);
    assert_eq!((f.first.clone(), f.second.clone()), (rid("a"), rid("b")));
    assert_eq!(f.resolution, Some(Deny));
    assert!(!f.shadow);
    assert!(f.requires_acknowledgement);
    assert!(report.is_blocked());
    assert_eq!(report.blocking().count(), 1);

    let report = review(&fx, &pair(&fx, RequireApproval));
    assert_eq!(report.findings[0].resolution, Some(RequireApproval));

    let [a, mut b] = pair(&fx, Deny);
    b.shadow = true;
    let report = review(&fx, &[a.clone(), b]);
    assert_eq!(report.findings.len(), 1);
    assert!(report.findings[0].shadow);
    assert!(
        !report.is_blocked(),
        "a shadow pair needs no acknowledgement"
    );

    let [a, mut b] = pair(&fx, Deny);
    b.outcome = Allow;
    assert!(review(&fx, &[a, b]).findings.is_empty(), "same outcome");
}

#[test]
fn publish_ignores_pairs_that_cannot_conflict() {
    let fx = Fx::new();
    let cases = [
        (
            "nested",
            vec![
                rule("a", Allow).contract(RESTART).named(&["plex"]),
                rule("b", Deny).category(LIFECYCLE),
            ],
        ),
        (
            "disjoint targets",
            vec![
                rule("a", Allow).contract(RESTART).named(&["plex"]),
                rule("b", Deny)
                    .category(LIFECYCLE)
                    .on_host(fx.host)
                    .named(&["radarr"]),
            ],
        ),
        (
            "different hosts",
            vec![
                rule("a", Allow).on_host(fx.host),
                rule("b", Deny).on_host(fx.other),
            ],
        ),
        (
            "disjoint capabilities",
            vec![
                rule("a", Allow).category(LIFECYCLE),
                rule("b", Deny).category("container.destroy"),
            ],
        ),
        (
            "disabled",
            vec![
                rule("a", Allow).contract(RESTART),
                rule("b", Deny).contract(RESTART).off(),
            ],
        ),
    ];
    for (label, rules) in cases {
        assert!(review(&fx, &rules).findings.is_empty(), "{label}");
    }
}

#[test]
fn publish_reports_equal_scope_conflicts() {
    let fx = Fx::new();
    let rules = [
        rule("a", Allow).contract(RESTART).on_host(fx.host),
        rule("b", RequireApproval)
            .contract(RESTART)
            .on_host(fx.host),
        rule("c", Allow).contract(RESTART).on_host(fx.host),
    ];
    let report = review(&fx, &rules);
    let pairs: Vec<_> = report
        .findings
        .iter()
        .map(|f| (f.kind, f.first.clone(), f.second.clone(), f.resolution))
        .collect();
    assert_eq!(
        pairs,
        vec![
            (
                FindingKind::EqualScopeConflict,
                rid("a"),
                rid("b"),
                Some(Deny)
            ),
            (
                FindingKind::EqualScopeConflict,
                rid("b"),
                rid("c"),
                Some(Deny)
            ),
        ]
    );
    assert!(report.is_blocked());
    assert!(
        review(&fx, &[rule("a", Allow), rule("b", Allow)])
            .findings
            .is_empty(),
        "equal scope, same outcome"
    );
}

#[test]
fn publish_reports_rules_ineffective_under_a_lock() {
    let fx = Fx::new();
    let rules = [
        rule("lock", Deny).locked().on_host(fx.host),
        rule("allow1", Allow).contract(RESTART).on_host(fx.host),
        rule("approve1", RequireApproval)
            .category(LIFECYCLE)
            .on_host(fx.host),
        rule("deny1", Deny).contract(GATED).on_host(fx.host),
        rule("wide", Allow),
    ];
    let report = review(&fx, &rules);
    let found: Vec<_> = report
        .findings
        .iter()
        .map(|f| (f.kind, f.first.clone(), f.second.clone(), f.resolution))
        .collect();
    assert_eq!(
        found,
        vec![
            (
                FindingKind::IneffectiveUnderLock,
                rid("lock"),
                rid("allow1"),
                Some(Deny)
            ),
            (
                FindingKind::IneffectiveUnderLock,
                rid("lock"),
                rid("approve1"),
                Some(Deny)
            ),
        ],
        "a deny under the lock and a rule wider than it are not reported"
    );
    assert!(report.is_blocked());
}

#[test]
fn publish_notes_allows_outside_every_ceiling() {
    let fx = Fx::new();
    let rules = [rule("a", Allow)
        .contract(RESTART)
        .named(&["plex"])
        .on_host(fx.host)];
    let none = BTreeMap::new();
    assert!(
        validate_publication(&rules, &fx.index, &none)
            .unwrap()
            .findings
            .is_empty()
    );

    let note_for = |ceilings: BTreeMap<HostId, CeilingSummary>, rules: &[Rule]| {
        validate_publication(rules, &fx.index, &ceilings)
            .unwrap()
            .findings
    };
    let outside = BTreeMap::from([(
        fx.host,
        summary(RESTART, CeilingTargets::Names(names(&["radarr"])), true),
    )]);
    let findings = note_for(outside, &rules);
    assert_eq!(findings.len(), 1);
    let f = &findings[0];
    assert_eq!(f.kind, FindingKind::TargetOutsideEveryCeiling);
    assert_eq!(f.first, f.second);
    assert_eq!(f.resolution, None);
    assert!(!f.requires_acknowledgement);
    assert!(f.note.contains("plex"), "{}", f.note);
    assert!(!PublicationReport { findings }.is_blocked());

    let covered = BTreeMap::from([(
        fx.host,
        summary(RESTART, CeilingTargets::Names(names(&["plex"])), true),
    )]);
    assert!(note_for(covered, &rules).is_empty());
    let any = BTreeMap::from([(fx.host, summary(RESTART, CeilingTargets::Any, true))]);
    assert!(note_for(any, &rules).is_empty());

    let disabled = BTreeMap::from([(fx.host, summary(RESTART, CeilingTargets::Any, false))]);
    assert_eq!(note_for(disabled, &rules).len(), 1);
    let elsewhere = BTreeMap::from([(fx.other, summary(RESTART, CeilingTargets::Any, true))]);
    assert_eq!(
        note_for(elsewhere.clone(), &rules).len(),
        1,
        "another host's ceiling is not this host's"
    );
    let wide = [rule("a", Allow).contract(RESTART).named(&["plex"])];
    assert!(
        note_for(elsewhere, &wide).is_empty(),
        "an all-hosts rule is covered by any host"
    );
    let other_contract = BTreeMap::from([(fx.host, summary(READ, CeilingTargets::Any, true))]);
    assert_eq!(note_for(other_contract, &rules).len(), 1);

    let not_allow = [
        rule("d", Deny).contract(RESTART).named(&["plex"]),
        rule("any", Allow).contract(RESTART),
    ];
    let outside = BTreeMap::from([(
        fx.host,
        summary(RESTART, CeilingTargets::Names(names(&["x"])), true),
    )]);
    assert!(
        note_for(outside, &not_allow).is_empty(),
        "only a named Allow is noted"
    );
}

#[test]
fn publication_report_is_deterministic() {
    let fx = Fx::new();
    let rules = vec![
        rule("e1", Allow).contract(RESTART).on_host(fx.host),
        rule("e2", Deny).contract(RESTART).on_host(fx.host),
        rule("i1", Allow).contract(RESTART).named(&["plex"]),
        rule("i2", RequireApproval)
            .category(LIFECYCLE)
            .on_host(fx.host),
        rule("lock", Deny).locked().on_host(fx.host),
        rule("n1", Allow)
            .contract(RESTART)
            .named(&["plex", "radarr"])
            .on_host(fx.host),
    ];
    let ceilings = BTreeMap::from([(
        fx.host,
        summary(RESTART, CeilingTargets::Names(names(&["radarr"])), true),
    )]);
    let reference = validate_publication(&rules, &fx.index, &ceilings).unwrap();
    assert!(reference.findings.len() >= 5, "{:#?}", reference.findings);
    let kinds: Vec<_> = reference.findings.iter().map(|f| f.kind).collect();
    let mut sorted = kinds.clone();
    sorted.sort();
    assert_eq!(kinds, sorted, "worst kind first");
    assert_eq!(kinds.first(), Some(&FindingKind::EqualScopeConflict));
    assert_eq!(kinds.last(), Some(&FindingKind::TargetOutsideEveryCeiling));

    let mut reversed = rules.clone();
    reversed.reverse();
    let mut rotated = rules.clone();
    rotated.rotate_left(3);
    for variant in [rules.clone(), reversed, rotated] {
        assert_eq!(
            validate_publication(&variant, &fx.index, &ceilings).unwrap(),
            reference
        );
    }
}

fn scan(dir: &Path, forbidden: &[String], files: &mut usize) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            scan(&path, forbidden, files);
        } else if path.extension().is_some_and(|e| e == "rs") {
            *files += 1;
            let text = fs::read_to_string(&path).unwrap();
            for word in forbidden {
                assert!(
                    !text.contains(word.as_str()),
                    "{} mentions {word}",
                    path.display()
                );
            }
        }
    }
}

#[test]
fn evaluate_constructs_no_authorization() {
    let forbidden = [
        concat!("Human", "Approval").to_owned(),
        concat!("Policy", "Permit").to_owned(),
        concat!("Author", "ization").to_owned(),
    ];
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = 0;
    scan(&src.join("policy"), &forbidden, &mut files);
    assert!(files >= 10, "only {files} files scanned");
    let top = fs::read_to_string(src.join("policy.rs")).unwrap();
    for word in &forbidden {
        assert!(!top.contains(word.as_str()), "policy.rs mentions {word}");
    }
}
