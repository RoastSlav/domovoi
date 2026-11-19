//! Builders shared by the shadow recording tests: rules, candidates and a fake environment.

use std::collections::BTreeMap;

use domovoi_core::capability::{ContractId, TargetName};
use domovoi_core::policy_store::ShadowCandidate;
use domovoi_core::{CheckKey, ExecutionId, HostId, OperationId, PinnedSubject, Risk, TargetKey};
use domovoi_engine::policy::{CapabilityScope, HostScope, Outcome, Rule, RuleId, TargetScope};
use domovoi_testkit::build::at;
use jiff::Timestamp;
use uuid::Uuid;

pub use crate::shadow_env::{FakeShadowEnvironment, skill};

/// The skill every test host announces.
pub const SKILL: &str = "restart_plex";
/// The contract id of [`SKILL`].
pub const SKILL_CONTRACT: &str = "skill.restart_plex";

/// A fixed host, so identities and snapshots are stable.
pub fn host() -> HostId {
    HostId::from_uuid(Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_00a1))
}

/// An environment where [`host`] announces [`SKILL`] (a mutating skill).
pub fn env() -> FakeShadowEnvironment {
    let env = FakeShadowEnvironment::new();
    env.add_skill(&skill(SKILL, Risk::Write));
    env
}

/// A rule that matches everything and says `outcome`.
pub fn rule(id: &str, outcome: Outcome) -> Rule {
    Rule {
        id: RuleId::parse(id).unwrap(),
        enabled: true,
        shadow: false,
        locked: false,
        host: HostScope::All,
        capability: CapabilityScope::Any,
        targets: TargetScope::Any,
        conditions: Vec::new(),
        outcome,
        note: String::new(),
        version_id: None,
    }
}

/// A shadow rule saying `outcome`.
pub fn shadow_rule(id: &str, outcome: Outcome) -> Rule {
    Rule {
        shadow: true,
        ..rule(id, outcome)
    }
}

/// A rule for exactly the skill contract.
pub fn skill_rule(id: &str, outcome: Outcome) -> Rule {
    Rule {
        capability: CapabilityScope::Contract(ContractId::parse(SKILL_CONTRACT).unwrap()),
        ..rule(id, outcome)
    }
}

/// A rule for the host named `name`.
pub fn named_rule(id: &str, name: &str, outcome: Outcome) -> Rule {
    Rule {
        targets: TargetScope::Exact([TargetName::parse(name).unwrap()].into()),
        ..rule(id, outcome)
    }
}

/// A step of the skill on [`host`], created at `secs`, in an operation pinned to the host.
pub fn candidate(contract: &str, secs: i64) -> ShadowCandidate {
    ShadowCandidate {
        execution: ExecutionId::new(),
        operation: OperationId::new(),
        host: host(),
        contract: ContractId::parse(contract).unwrap(),
        contract_version: 1,
        check: CheckKey::parse("container.down").unwrap(),
        target: None,
        args: BTreeMap::from([("container".to_owned(), "plex".to_owned())]),
        pinned: vec![PinnedSubject {
            host: host(),
            check: CheckKey::parse("container.down").unwrap(),
            target: None,
        }],
        created_at: at(secs),
    }
}

/// The skill step at second 0.
pub fn skill_candidate() -> ShadowCandidate {
    candidate(SKILL_CONTRACT, 0)
}

/// `at(secs)` re-exported under a short name.
pub fn t(secs: i64) -> Timestamp {
    at(secs)
}

/// A pinned container subject, as an investigation of one container pins.
pub fn container_pin() -> PinnedSubject {
    PinnedSubject {
        host: host(),
        check: CheckKey::parse("container.down").unwrap(),
        target: Some(TargetKey::parse("plex").unwrap()),
    }
}
