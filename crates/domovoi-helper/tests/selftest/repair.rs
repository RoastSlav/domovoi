//! `helper repair`: planned from faults, applied only after confirmation, never at start.

use std::cell::RefCell;

use domovoi_core::identity::{PathFacts, PathKind, RefusalToken};
use domovoi_helper::repair::{
    Change, Confirm, RefuseReason, RepairEntry, RepairOps, RepairOutcome, RepairPlan, render_plan,
    run_repair,
};

use super::support::*;

struct Answer {
    terminal: bool,
    yes: bool,
}

impl Confirm for Answer {
    fn has_terminal(&self) -> bool {
        self.terminal
    }
    fn confirmed(&self, _plan: &RepairPlan) -> bool {
        self.yes
    }
}

#[derive(Default)]
struct Spy {
    root: bool,
    applied: RefCell<Vec<RepairEntry>>,
}

impl RepairOps for Spy {
    fn is_root(&self) -> bool {
        self.root
    }
    fn apply(&self, entry: &RepairEntry) -> Result<(), String> {
        self.applied.borrow_mut().push(entry.clone());
        Ok(())
    }
}

fn root_spy() -> Spy {
    Spy {
        root: true,
        ..Spy::default()
    }
}

fn wrong_mode_host() -> domovoi_testkit::FakeHostInspect {
    healthy_host().path(dirs().helper_dir, dir_facts(0o755, 0, 0))
}

fn file_facts(mode: u32) -> PathFacts {
    PathFacts {
        kind: Some(PathKind::File),
        symlink: false,
        mode,
        uid: 0,
        gid: 0,
    }
}

// covers: confirmation_requires_terminal
#[test]
fn helper_repair_requires_confirmation_and_prints_the_plan() {
    let host = wrong_mode_host();
    let d = dirs();
    let spy = root_spy();
    let no = Answer {
        terminal: true,
        yes: false,
    };
    let declined = run_repair(&host, &d, &no, &spy).expect("run");
    assert_eq!(declined.outcome, RepairOutcome::Declined);
    assert!(
        spy.applied.borrow().is_empty(),
        "nothing changes without the typed word"
    );
    let plan = render_plan(&declined.plan);
    assert!(plan.contains("/var/lib/domovoi-helper"), "{plan}");
    assert!(
        plan.contains("0755") && plan.contains("0700"),
        "found and expected mode: {plan}"
    );

    let yes = Answer {
        terminal: true,
        yes: true,
    };
    let confirmed = run_repair(&host, &d, &yes, &spy).expect("run");
    assert!(matches!(confirmed.outcome, RepairOutcome::Applied { .. }));
    let applied = spy.applied.borrow();
    assert_eq!(applied.len(), 1, "only the listed path changes");
    assert_eq!(applied[0].change, Change::Chmod(0o700));
}

#[test]
fn helper_repair_refuses_without_a_terminal() {
    let spy = root_spy();
    let answer = Answer {
        terminal: false,
        yes: true,
    };
    let report = run_repair(&wrong_mode_host(), &dirs(), &answer, &spy).expect("run");
    assert_eq!(
        report.outcome,
        RepairOutcome::NoTerminal(RefusalToken::ConfirmationRequiresTerminal)
    );
    assert!(spy.applied.borrow().is_empty());
}

#[test]
fn helper_repair_refuses_when_not_root() {
    let spy = Spy::default();
    let answer = Answer {
        terminal: true,
        yes: true,
    };
    let report = run_repair(&wrong_mode_host(), &dirs(), &answer, &spy).expect("run");
    assert_eq!(report.outcome, RepairOutcome::NotRoot);
    assert!(spy.applied.borrow().is_empty());
}

#[test]
fn helper_repair_refuses_symlinks_and_unexpected_types_and_reruns_the_checks() {
    let d = dirs();
    let mut linked_key = file_facts(0o600);
    linked_key.symlink = true;
    let host = wrong_mode_host()
        .path(d.helper_dir.join("attest.key"), linked_key)
        .path(
            d.helper_dir.join("state"),
            PathFacts {
                kind: Some(PathKind::Dir),
                ..file_facts(0o600)
            },
        );
    let spy = root_spy();
    let answer = Answer {
        terminal: true,
        yes: true,
    };
    let report = run_repair(&host, &d, &answer, &spy).expect("run");
    let refused: Vec<RefuseReason> = report.plan.refused.iter().map(|(_, why)| *why).collect();
    assert!(refused.contains(&RefuseReason::Symlink), "{refused:?}");
    assert!(refused.contains(&RefuseReason::WrongType), "{refused:?}");
    let applied = spy.applied.borrow();
    assert!(
        applied
            .iter()
            .all(|e| !e.path.ends_with("attest.key") && !e.path.ends_with("state")),
        "a symlink or a wrong type is never touched"
    );
    assert!(
        !report.remaining.is_empty(),
        "the checks run again and report what is left"
    );
}
