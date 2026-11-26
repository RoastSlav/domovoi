//! Plan validation for `unit.restart`: the helper resolves the unit, the pinned subject decides
//! whether the model may act on it, and `container.restart` can never be planned.

use std::collections::BTreeMap;
use std::sync::Arc;

use domovoi_core::capability::{
    Availability, ContractIndex, EXECUTABLE_BUILTINS, builtin_contracts, unit_contracts,
};
use domovoi_core::meta::{ArgValue, PlanInput, PlanStepInput};
use domovoi_core::ports::HostRepo;
use domovoi_core::unit::UnitRefusal;
use domovoi_core::{CheckKey, ContractId, PinnedSubject, TargetKey};
use domovoi_engine::plan::{PlanRefusalCode, PlanScope, PlanValidator, PlanVerdict, is_executable};
use domovoi_testkit::build::host;
use domovoi_testkit::{FakeCapabilityRegistry, FakeTargetResolver, InMemoryStore, ListRedactor};

struct World {
    host: domovoi_core::Host,
    store: Arc<InMemoryStore>,
    registry: FakeCapabilityRegistry,
    resolver: FakeTargetResolver,
    redactor: ListRedactor,
}

async fn world() -> World {
    let host = host("nas").unwrap();
    let store = Arc::new(InMemoryStore::new());
    store.upsert_host(&host).await.unwrap();
    let mut contracts = builtin_contracts();
    contracts.extend(unit_contracts());
    let registry = FakeCapabilityRegistry::new(Arc::new(ContractIndex::new(contracts).unwrap()));
    registry.set_availability(
        host.id,
        &ContractId::parse("unit.restart").unwrap(),
        Availability::Available,
    );
    let resolver = FakeTargetResolver::new();
    resolver.set_unit(
        host.id,
        "example.service",
        "example.service",
        'c',
        "untested",
    );
    World {
        host,
        store,
        registry,
        resolver,
        redactor: ListRedactor::new(["hunter2"]),
    }
}

impl World {
    fn scope(&self, target: &str) -> PlanScope {
        PlanScope {
            operation: None,
            pinned: vec![PinnedSubject {
                host: self.host.id,
                check: CheckKey::parse("unit.failed").unwrap(),
                target: Some(TargetKey::parse(target).unwrap()),
            }],
            executions_left: None,
            prior: Vec::new(),
            cycle: 0,
        }
    }

    async fn run(&self, scope: &PlanScope, step: PlanStepInput) -> PlanVerdict {
        PlanValidator {
            registry: &self.registry,
            resolver: &self.resolver,
            hosts: self.store.as_ref(),
            redactor: &self.redactor,
        }
        .validate(scope, &[], &PlanInput { steps: vec![step] })
        .await
    }
}

fn step(contract: &str, unit: Option<&str>) -> PlanStepInput {
    PlanStepInput {
        contract: contract.to_owned(),
        args: unit
            .map(|u| BTreeMap::from([("unit".to_owned(), ArgValue::Text(u.to_owned()))]))
            .unwrap_or_default(),
        host: None,
        target: None,
        reason: "it keeps failing".to_owned(),
        retry_of: None,
    }
}

#[test]
fn only_unit_restart_and_the_block_are_executable_among_the_built_ins() {
    assert_eq!(
        EXECUTABLE_BUILTINS,
        ["unit.restart", "firewall.block_source"]
    );
    assert!(is_executable(&ContractId::parse("unit.restart").unwrap()));
    assert!(is_executable(
        &ContractId::parse("firewall.block_source").unwrap()
    ));
    assert!(!is_executable(
        &ContractId::parse("firewall.list_blocks").unwrap()
    ));
    assert!(is_executable(&ContractId::parse("skill.anything").unwrap()));
    assert!(!is_executable(
        &ContractId::parse("container.restart").unwrap()
    ));
    assert!(!is_executable(&ContractId::parse("unit.identity").unwrap()));
}

#[tokio::test]
async fn unit_step_is_validated_resolved_and_bound() {
    let w = world().await;
    let verdict = w
        .run(
            &w.scope("example.service"),
            step("unit.restart", Some("example.service")),
        )
        .await;
    assert!(verdict.refused.is_empty(), "{:?}", verdict.refused);
    let planned = &verdict.accepted[0];
    let unit = planned
        .unit
        .as_ref()
        .expect("a unit step carries its binding");
    assert_eq!(unit.id.as_str(), "example.service");
    assert_eq!(unit.identity_hash.to_string(), "c".repeat(64));
    assert!(
        unit.card
            .render("nas")
            .starts_with("Restart example.service on nas.")
    );
    assert_eq!(planned.target.as_ref().unwrap().as_str(), "example.service");
    assert_eq!(planned.args["unit"], "example.service");
}

#[tokio::test]
async fn alias_typed_name_binds_the_canonical_id_when_it_is_the_pinned_unit() {
    let w = world().await;
    // The owner pinned the canonical id; the model typed an alias that resolves to it.
    w.resolver.set_unit(
        w.host.id,
        "alias.service",
        "example.service",
        'c',
        "untested",
    );
    let verdict = w
        .run(
            &w.scope("example.service"),
            step("unit.restart", Some("alias.service")),
        )
        .await;
    assert!(verdict.refused.is_empty(), "{:?}", verdict.refused);
    assert_eq!(
        verdict.accepted[0].unit.as_ref().unwrap().id.as_str(),
        "example.service"
    );
}

#[tokio::test]
async fn unit_step_outside_pinned_subjects_is_refused() {
    let w = world().await;
    w.resolver
        .set_unit(w.host.id, "other.service", "other.service", 'a', "untested");
    let verdict = w
        .run(
            &w.scope("example.service"),
            step("unit.restart", Some("other.service")),
        )
        .await;
    assert_eq!(verdict.refused[0].code, PlanRefusalCode::TargetMismatch);
}

#[tokio::test]
async fn a_helper_refusal_becomes_a_unit_refused_plan_refusal_with_its_token() {
    let w = world().await;
    w.resolver.refuse_unit(
        w.host.id,
        "example.service",
        UnitRefusal::TargetNotAllowlisted,
    );
    let verdict = w
        .run(
            &w.scope("example.service"),
            step("unit.restart", Some("example.service")),
        )
        .await;
    let refusal = &verdict.refused[0];
    assert_eq!(refusal.code, PlanRefusalCode::UnitRefused);
    assert!(
        refusal.message.contains("target_not_allowlisted"),
        "{}",
        refusal.message
    );
    assert_eq!(refusal.code.as_str(), "unit_refused");
}

#[tokio::test]
async fn a_bad_unit_name_and_bad_arguments_are_refused_before_any_read() {
    let w = world().await;
    let scope = w.scope("example.service");
    for name in ["example.socket", "-rf.service", "worker@.service"] {
        let verdict = w.run(&scope, step("unit.restart", Some(name))).await;
        assert_eq!(
            verdict.refused[0].code,
            PlanRefusalCode::TargetInvalid,
            "{name}"
        );
        assert!(verdict.refused[0].message.contains("bad_unit_name"));
    }
    let none = w.run(&scope, step("unit.restart", None)).await;
    assert_eq!(none.refused[0].code, PlanRefusalCode::BadArguments);
    let mut extra = step("unit.restart", Some("example.service"));
    extra
        .args
        .insert("force".to_owned(), ArgValue::Text("yes".to_owned()));
    assert_eq!(
        w.run(&scope, extra).await.refused[0].code,
        PlanRefusalCode::BadArguments
    );
}

#[tokio::test]
async fn container_restart_is_refused_not_executable_and_identity_only_reads() {
    let w = world().await;
    let scope = w.scope("example.service");
    let container = w.run(&scope, step("container.restart", Some("plex"))).await;
    assert_eq!(container.refused[0].code, PlanRefusalCode::NotExecutable);
    let identity = w
        .run(&scope, step("unit.identity", Some("example.service")))
        .await;
    assert_eq!(identity.refused[0].code, PlanRefusalCode::NotAPlanStep);
}
