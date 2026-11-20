//! The plan validator: every refusal code, its exact wording and the paths that accept.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use domovoi_core::capability::{
    Availability, NotAllowedOnHost, TargetName, TargetUnresolved, UnavailableReason,
};
use domovoi_core::meta::{ArgValue, PlanInput, PlanStepInput, RetryOfInput};
use domovoi_core::ports::{CapabilityRegistry, HostRepo, TargetResolver};
use domovoi_core::{
    CapabilityContract, CheckKey, ContractId, Host, HostId, ParamKind, ParamName, ParamSpec,
    PinnedSubject, ResolvedTarget, Risk, StepId, StepOutcome, TargetError, TargetKey, TargetKind,
    TargetQuery, ValidationError,
};
use domovoi_engine::plan::{
    PlanRefusalCode, PlanScope, PlanValidator, PlanVerdict, PriorRun, is_executable,
    resolve_named_target,
};
use domovoi_testkit::build::{host, manifest};
use domovoi_testkit::{FakeCapabilityRegistry, FakeTargetResolver, InMemoryStore, ListRedactor};

const SECRET: &str = "hunter2";

/// A resolver that follows the testkit's host answers and lets a test script the rest.
struct Scripted {
    inner: FakeTargetResolver,
    host_error: Mutex<Option<TargetError>>,
    container: Mutex<Option<Result<ResolvedTarget, TargetError>>>,
}

#[async_trait]
impl TargetResolver for Scripted {
    async fn resolve(&self, query: TargetQuery) -> Result<ResolvedTarget, TargetError> {
        match &query {
            TargetQuery::Host { .. } => {
                if let Some(error) = self.host_error.lock().unwrap().clone() {
                    return Err(error);
                }
                self.inner.resolve(query).await
            }
            _ => self
                .container
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| panic!("no container answer scripted")),
        }
    }
}

fn param(name: &str, required: bool, kind: ParamKind) -> ParamSpec {
    ParamSpec {
        name: ParamName::parse(name).unwrap(),
        description: format!("The {name}."),
        required,
        kind,
    }
}

fn summary(
    name: &str,
    risk: Risk,
    params: Vec<ParamSpec>,
    applies_to: &[&str],
) -> domovoi_core::SkillSummary {
    let mut manifest = manifest(name, risk).unwrap();
    manifest.params = params;
    manifest.applies_to = applies_to
        .iter()
        .map(|check| CheckKey::parse(check).unwrap())
        .collect();
    manifest.summary(domovoi_core::ScriptDigest::of_bytes(b"fixture entry"))
}

struct World {
    host: Host,
    store: Arc<InMemoryStore>,
    registry: FakeCapabilityRegistry,
    resolver: Scripted,
    redactor: ListRedactor,
}

impl World {
    async fn new() -> Self {
        let host = host("nas").unwrap();
        let store = Arc::new(InMemoryStore::new());
        store.upsert_host(&host).await.unwrap();
        let registry = FakeCapabilityRegistry::empty();
        registry.set_skills(
            host.id,
            vec![
                summary(
                    "restart_container",
                    Risk::Write,
                    vec![param(
                        "container",
                        true,
                        ParamKind::String {
                            pattern: "^[a-z0-9_-]+$".to_owned(),
                            max_len: 64,
                        },
                    )],
                    &["docker.container"],
                ),
                summary("disk_report", Risk::Read, Vec::new(), &[]),
                summary(
                    "prune_logs",
                    Risk::Write,
                    vec![
                        param("keep", true, ParamKind::Integer { min: 1, max: 100 }),
                        param("force", false, ParamKind::Boolean),
                    ],
                    &[],
                ),
            ],
        );
        Self {
            host,
            store,
            registry,
            resolver: Scripted {
                inner: FakeTargetResolver::new(),
                host_error: Mutex::new(None),
                container: Mutex::new(None),
            },
            redactor: ListRedactor::new([SECRET]),
        }
    }

    async fn add_host(&self, name: &str) -> Host {
        let other = host(name).unwrap();
        self.store.upsert_host(&other).await.unwrap();
        other
    }

    fn validator(&self) -> PlanValidator<'_> {
        PlanValidator {
            registry: &self.registry,
            resolver: &self.resolver,
            hosts: self.store.as_ref(),
            redactor: &self.redactor,
        }
    }

    fn subject(&self, check: &str, target: Option<&str>) -> PinnedSubject {
        PinnedSubject {
            host: self.host.id,
            check: CheckKey::parse(check).unwrap(),
            target: target.map(|t| TargetKey::parse(t).unwrap()),
        }
    }

    fn scope(&self) -> PlanScope {
        PlanScope {
            operation: None,
            pinned: vec![self.subject("docker.container", Some("plex"))],
            executions_left: None,
            prior: Vec::new(),
            cycle: 0,
        }
    }

    async fn run(&self, scope: &PlanScope, steps: Vec<PlanStepInput>) -> PlanVerdict {
        self.validator()
            .validate(scope, &[], &PlanInput { steps })
            .await
    }

    async fn one(&self, scope: &PlanScope, step: PlanStepInput) -> PlanVerdict {
        self.run(scope, vec![step]).await
    }

    fn prior(&self, step: StepId, container: &str, outcome: StepOutcome) -> PriorRun {
        PriorRun {
            step,
            contract: ContractId::parse("skill.restart_container").unwrap(),
            host: self.host.id,
            check: CheckKey::parse("docker.container").unwrap(),
            target: Some(TargetKey::parse("plex").unwrap()),
            args: BTreeMap::from([("container".to_owned(), container.to_owned())]),
            outcome,
        }
    }
}

fn step(contract: &str) -> PlanStepInput {
    PlanStepInput {
        contract: contract.to_owned(),
        args: BTreeMap::new(),
        host: None,
        target: None,
        reason: "because".to_owned(),
        retry_of: None,
    }
}

fn restart(container: &str) -> PlanStepInput {
    PlanStepInput {
        args: BTreeMap::from([("container".to_owned(), ArgValue::Text(container.to_owned()))]),
        ..step("skill.restart_container")
    }
}

fn retry(step_id: &str, changed: &str) -> Option<RetryOfInput> {
    Some(RetryOfInput {
        step: step_id.to_owned(),
        changed: changed.to_owned(),
    })
}

/// Asserts the verdict holds exactly one refusal and returns it.
fn only_refusal(verdict: &PlanVerdict) -> &domovoi_engine::plan::PlanRefusal {
    assert!(
        verdict.accepted.is_empty(),
        "unexpectedly accepted: {:?}",
        verdict.accepted
    );
    assert_eq!(verdict.refused.len(), 1, "{:?}", verdict.refused);
    &verdict.refused[0]
}

fn assert_refused(verdict: &PlanVerdict, code: PlanRefusalCode, message: &str) {
    let refusal = only_refusal(verdict);
    assert_eq!(refusal.code, code);
    assert_eq!(refusal.message, message);
}

#[tokio::test]
async fn unknown_contract_for_a_skill_keeps_the_old_wording() {
    let world = World::new().await;
    let verdict = world.one(&world.scope(), step("skill.wipe_disk")).await;
    let refusal = only_refusal(&verdict);
    assert_eq!(refusal.code, PlanRefusalCode::UnknownContract);
    assert_eq!(refusal.message, "not a skill on this host");
    assert_eq!(refusal.hint.as_deref(), Some("call list_capabilities"));
    assert_eq!(
        refusal.dropped_line(),
        "wipe_disk: not a skill on this host"
    );
    assert_eq!(refusal.contract, "skill.wipe_disk");
    assert_eq!(refusal.index, 0);
}

#[tokio::test]
async fn an_unknown_builtin_contract_names_the_capability() {
    let world = World::new().await;
    let verdict = world.one(&world.scope(), step("container.nope")).await;
    assert_refused(
        &verdict,
        PlanRefusalCode::UnknownContract,
        "no capability `container.nope` on this host",
    );
}

#[tokio::test]
async fn a_bare_skill_name_gets_a_did_you_mean_hint() {
    let world = World::new().await;
    let verdict = world.one(&world.scope(), step("restart_container")).await;
    let refusal = only_refusal(&verdict);
    assert_eq!(refusal.code, PlanRefusalCode::UnknownContract);
    assert_eq!(
        refusal.message,
        "no capability `restart_container` on this host"
    );
    assert_eq!(
        refusal.hint.as_deref(),
        Some("did you mean `skill.restart_container`?")
    );
}

#[tokio::test]
async fn a_read_contract_is_not_a_plan_step() {
    let world = World::new().await;
    let skill = world.one(&world.scope(), step("skill.disk_report")).await;
    assert_refused(
        &skill,
        PlanRefusalCode::NotAPlanStep,
        "`skill.disk_report` only reads; get it with request_evidence",
    );
    let builtin = world.one(&world.scope(), step("container.list")).await;
    assert_refused(
        &builtin,
        PlanRefusalCode::NotAPlanStep,
        "`container.list` only reads; get it with request_evidence",
    );
}

#[test]
fn only_the_skill_family_is_executable() {
    assert!(is_executable(
        &ContractId::parse("skill.restart_container").unwrap()
    ));
    assert!(!is_executable(
        &ContractId::parse("container.list").unwrap()
    ));
    assert!(!is_executable(&ContractId::parse("host.overview").unwrap()));
}

#[tokio::test]
async fn unknown_host_and_unpinned_host_are_refused() {
    let world = World::new().await;
    world.add_host("other").await;
    let scope = world.scope();

    let ghost = PlanStepInput {
        host: Some("ghost".to_owned()),
        ..restart("plex")
    };
    assert_refused(
        &world.one(&scope, ghost).await,
        PlanRefusalCode::UnknownHost,
        "no host named `ghost`",
    );

    let malformed = PlanStepInput {
        host: Some("Not A Host!".to_owned()),
        ..restart("plex")
    };
    assert_refused(
        &world.one(&scope, malformed).await,
        PlanRefusalCode::UnknownHost,
        "no host named `Not A Host!`",
    );

    let unpinned = PlanStepInput {
        host: Some("other".to_owned()),
        ..restart("plex")
    };
    assert_refused(
        &world.one(&scope, unpinned).await,
        PlanRefusalCode::HostNotInScope,
        "host `other` is not part of this investigation",
    );
}

#[tokio::test]
async fn an_omitted_host_is_refused_when_several_hosts_are_pinned() {
    let world = World::new().await;
    let other = world.add_host("other").await;
    let mut scope = world.scope();
    scope.pinned.push(PinnedSubject {
        host: other.id,
        check: CheckKey::parse("docker.container").unwrap(),
        target: None,
    });
    assert_refused(
        &world.one(&scope, restart("plex")).await,
        PlanRefusalCode::UnknownHost,
        "name the host: several are part of this investigation",
    );
}

#[tokio::test]
async fn omitted_host_and_target_resolve_to_the_pinned_subject() {
    let world = World::new().await;
    let verdict = world.one(&world.scope(), restart("plex")).await;
    assert!(verdict.refused.is_empty(), "{:?}", verdict.refused);
    let [planned] = verdict.accepted.as_slice() else {
        panic!("expected one accepted step");
    };
    assert_eq!(planned.host, world.host.id);
    assert_eq!(planned.check.as_str(), "docker.container");
    assert_eq!(planned.target.as_ref().map(TargetKey::as_str), Some("plex"));
    assert_eq!(planned.contract.as_str(), "skill.restart_container");
    assert_eq!(planned.args["container"], "plex");
    assert_eq!(planned.reason, "because");
    assert_eq!(planned.note, None);
    assert_eq!(planned.retry_of, None);
    let expected = world
        .resolver
        .resolve(TargetQuery::Host {
            host: world.host.id,
        })
        .await
        .unwrap();
    assert_eq!(planned.identity, expected.identity);
}

#[tokio::test]
async fn a_named_host_and_the_pinned_target_are_accepted() {
    let world = World::new().await;
    let named = PlanStepInput {
        host: Some("nas".to_owned()),
        target: Some("plex".to_owned()),
        ..restart("plex")
    };
    let verdict = world.one(&world.scope(), named).await;
    assert!(verdict.refused.is_empty(), "{:?}", verdict.refused);
    assert_eq!(verdict.accepted.len(), 1);
}

#[tokio::test]
async fn a_named_target_that_is_not_the_pinned_one_is_refused() {
    let world = World::new().await;
    let other = PlanStepInput {
        target: Some("redis".to_owned()),
        ..restart("plex")
    };
    assert_refused(
        &world.one(&world.scope(), other).await,
        PlanRefusalCode::TargetMismatch,
        "target `redis` is not this investigation's target (`plex`)",
    );

    let mut targetless = world.scope();
    targetless.pinned = vec![world.subject("docker.container", None)];
    let named = PlanStepInput {
        target: Some("redis".to_owned()),
        ..restart("plex")
    };
    assert_refused(
        &world.one(&targetless, named).await,
        PlanRefusalCode::TargetMismatch,
        "target `redis` is not this investigation's target (`none`)",
    );
}

#[tokio::test]
async fn several_subjects_on_one_host_need_a_target_to_choose() {
    let world = World::new().await;
    let mut scope = world.scope();
    scope
        .pinned
        .push(world.subject("docker.container", Some("redis")));
    assert_refused(
        &world.one(&scope, restart("plex")).await,
        PlanRefusalCode::TargetMismatch,
        "name which target you mean",
    );
    let chosen = PlanStepInput {
        target: Some("redis".to_owned()),
        ..restart("redis")
    };
    let verdict = world.one(&scope, chosen).await;
    assert_eq!(verdict.accepted.len(), 1, "{:?}", verdict.refused);
    assert_eq!(
        verdict.accepted[0].target.as_ref().map(TargetKey::as_str),
        Some("redis")
    );
}

#[tokio::test]
async fn host_target_errors_map_to_fixed_texts() {
    let world = World::new().await;
    let query = TargetQuery::Host {
        host: world.host.id,
    };
    let unresolved = |reason| TargetError::Unresolved {
        query: query.clone(),
        reason,
    };
    let cases = [
        (
            unresolved(TargetUnresolved::HostIdentityUnproven),
            PlanRefusalCode::TargetUnresolved,
            "host `nas` has no enrolled agent key, so a plan cannot be bound to it",
        ),
        (
            unresolved(TargetUnresolved::ContainerVanished),
            PlanRefusalCode::TargetUnresolved,
            "no container named `nas` on that host now",
        ),
        (
            unresolved(TargetUnresolved::ContainerIdentityUnproven),
            PlanRefusalCode::TargetUnresolved,
            "container `nas` has no provable full id",
        ),
        (
            unresolved(TargetUnresolved::UnitIdentityUnproven),
            PlanRefusalCode::TargetUnresolved,
            "systemd units cannot be targeted yet",
        ),
        (
            TargetError::Ambiguous {
                query: query.clone(),
                count: 2,
            },
            PlanRefusalCode::TargetAmbiguous,
            "2 containers are named `nas`; none was chosen",
        ),
        (
            TargetError::InvalidName {
                query: query.clone(),
                reason: ValidationError::new("target name", "must not be empty"),
            },
            PlanRefusalCode::TargetInvalid,
            "must not be empty",
        ),
        (
            TargetError::HostUnavailable {
                host: world.host.id,
                state: Availability::HostUnknown,
            },
            PlanRefusalCode::HostOffline,
            "this host has never connected to this hub",
        ),
    ];
    for (error, code, message) in cases {
        *world.resolver.host_error.lock().unwrap() = Some(error);
        let verdict = world.one(&world.scope(), restart("plex")).await;
        assert_refused(&verdict, code, message);
    }
}

#[tokio::test]
async fn a_host_without_an_enrolled_key_is_unresolved() {
    let world = World::new().await;
    world.resolver.inner.fail(world.host.id);
    assert_refused(
        &world.one(&world.scope(), restart("plex")).await,
        PlanRefusalCode::TargetUnresolved,
        "host `nas` has no enrolled agent key, so a plan cannot be bound to it",
    );
}

fn container_contract(world: &World) -> CapabilityContract {
    let contract = world
        .registry
        .contracts()
        .get(&ContractId::parse("container.logs").unwrap())
        .unwrap()
        .clone();
    assert_eq!(contract.target_kind, TargetKind::Container);
    assert_eq!(contract.target_arg.as_deref(), Some("container"));
    contract
}

fn args_of(container: &str) -> BTreeMap<String, String> {
    BTreeMap::from([("container".to_owned(), container.to_owned())])
}

#[tokio::test]
async fn named_container_resolution_goes_through_the_resolver_and_the_grammar() {
    let world = World::new().await;
    let contract = container_contract(&world);
    let host_proof = world
        .resolver
        .resolve(TargetQuery::Host {
            host: world.host.id,
        })
        .await
        .unwrap();
    let plex = ResolvedTarget {
        name: TargetName::parse("plex").unwrap(),
        kind: TargetKind::Container,
        ..host_proof
    };
    let resolver = &world.resolver;
    let host_id: HostId = world.host.id;

    *resolver.container.lock().unwrap() = Some(Ok(plex.clone()));
    let resolved = resolve_named_target(resolver, host_id, &contract, &args_of("plex"), None)
        .await
        .unwrap();
    assert_eq!(resolved, plex);
    let same_target =
        resolve_named_target(resolver, host_id, &contract, &args_of("plex"), Some("plex"))
            .await
            .unwrap();
    assert_eq!(same_target, plex);

    // The grammar refuses before the resolver is asked: nothing is scripted for this name.
    *resolver.container.lock().unwrap() = None;
    let invalid = resolve_named_target(resolver, host_id, &contract, &args_of("-plex"), None).await;
    assert_eq!(
        invalid.unwrap_err(),
        (
            PlanRefusalCode::TargetInvalid,
            "must start with a letter or a digit".to_owned()
        )
    );

    let query = TargetQuery::Container {
        host: host_id,
        name: TargetName::parse("plex").unwrap(),
    };
    *resolver.container.lock().unwrap() = Some(Err(TargetError::Ambiguous {
        query: query.clone(),
        count: 2,
    }));
    let ambiguous =
        resolve_named_target(resolver, host_id, &contract, &args_of("plex"), None).await;
    assert_eq!(
        ambiguous.unwrap_err(),
        (
            PlanRefusalCode::TargetAmbiguous,
            "2 containers are named `plex`; none was chosen".to_owned()
        )
    );

    *resolver.container.lock().unwrap() = Some(Err(TargetError::Unresolved {
        query,
        reason: TargetUnresolved::ContainerVanished,
    }));
    let vanished = resolve_named_target(resolver, host_id, &contract, &args_of("plex"), None).await;
    assert_eq!(
        vanished.unwrap_err(),
        (
            PlanRefusalCode::TargetUnresolved,
            "no container named `plex` on that host now".to_owned()
        )
    );
}

#[tokio::test]
async fn named_target_resolution_refuses_a_missing_argument_and_a_mismatched_target() {
    let world = World::new().await;
    let contract = container_contract(&world);
    let resolver = &world.resolver;
    let host_id = world.host.id;

    let missing = resolve_named_target(resolver, host_id, &contract, &BTreeMap::new(), None).await;
    assert_eq!(
        missing.unwrap_err(),
        (
            PlanRefusalCode::BadArguments,
            "bad arguments: missing required `container`".to_owned()
        )
    );

    let mismatch = resolve_named_target(
        resolver,
        host_id,
        &contract,
        &args_of("plex"),
        Some("redis"),
    )
    .await;
    assert_eq!(
        mismatch.unwrap_err(),
        (
            PlanRefusalCode::TargetMismatch,
            "target `redis` does not match `container` (`plex`)".to_owned()
        )
    );

    let hostwide = world
        .registry
        .contracts()
        .get(&ContractId::parse("container.list").unwrap())
        .unwrap()
        .clone();
    let none = resolve_named_target(resolver, host_id, &hostwide, &args_of("plex"), None).await;
    assert_eq!(
        none.unwrap_err(),
        (
            PlanRefusalCode::TargetInvalid,
            "the capability names no target argument".to_owned()
        )
    );
}

#[tokio::test]
async fn a_skill_that_does_not_apply_is_refused_with_the_old_wording() {
    let world = World::new().await;
    let mut scope = world.scope();
    scope.pinned = vec![world.subject("disk.usage", None)];
    assert_refused(
        &world.one(&scope, restart("plex")).await,
        PlanRefusalCode::NotApplicable,
        "does not apply to disk.usage",
    );
}

#[tokio::test]
async fn bad_skill_arguments_use_the_manifest_error_text() {
    let world = World::new().await;
    let scope = world.scope();

    let missing = world.one(&scope, step("skill.restart_container")).await;
    assert_refused(
        &missing,
        PlanRefusalCode::BadArguments,
        "bad arguments: missing required argument `container`",
    );

    let unknown = PlanStepInput {
        args: BTreeMap::from([
            ("container".to_owned(), ArgValue::Text("plex".to_owned())),
            ("force".to_owned(), ArgValue::Flag(true)),
        ]),
        ..step("skill.restart_container")
    };
    assert_refused(
        &world.one(&scope, unknown).await,
        PlanRefusalCode::BadArguments,
        "bad arguments: unknown argument `force`",
    );

    let invalid = world.one(&scope, restart("Plex Server")).await;
    let refusal = only_refusal(&invalid);
    assert_eq!(refusal.code, PlanRefusalCode::BadArguments);
    assert!(
        refusal
            .message
            .starts_with("bad arguments: argument `container`:"),
        "{}",
        refusal.message
    );
}

#[tokio::test]
async fn integer_and_boolean_arguments_are_normalised_before_the_check() {
    let world = World::new().await;
    let prune = PlanStepInput {
        args: BTreeMap::from([
            ("keep".to_owned(), ArgValue::Int(14)),
            ("force".to_owned(), ArgValue::Flag(true)),
        ]),
        ..step("skill.prune_logs")
    };
    let verdict = world.one(&world.scope(), prune).await;
    assert!(verdict.refused.is_empty(), "{:?}", verdict.refused);
    assert_eq!(
        verdict.accepted[0].args,
        BTreeMap::from([
            ("force".to_owned(), "true".to_owned()),
            ("keep".to_owned(), "14".to_owned()),
        ])
    );

    let out_of_range = PlanStepInput {
        args: BTreeMap::from([("keep".to_owned(), ArgValue::Int(500))]),
        ..step("skill.prune_logs")
    };
    assert_refused(
        &world.one(&world.scope(), out_of_range).await,
        PlanRefusalCode::BadArguments,
        "bad arguments: argument `keep`: must be between 1 and 100",
    );
}

#[tokio::test]
async fn an_offline_host_is_refused_as_offline() {
    let world = World::new().await;
    let id = ContractId::parse("skill.restart_container").unwrap();
    for state in [
        Availability::HostUnknown,
        Availability::HostAbsent {
            since: domovoi_testkit::build::T0,
        },
    ] {
        world.registry.set_availability(world.host.id, &id, state);
        assert_refused(
            &world.one(&world.scope(), restart("plex")).await,
            PlanRefusalCode::HostOffline,
            "the host's agent is not connected; propose again after it reconnects",
        );
    }
}

#[tokio::test]
async fn disabled_incompatible_and_not_installed_are_refused() {
    let world = World::new().await;
    let id = ContractId::parse("skill.restart_container").unwrap();
    for state in [
        Availability::DisabledByHub,
        Availability::HostIncompatible {
            reported: "1.1".to_owned(),
        },
        Availability::NotInstalled,
    ] {
        let described = state.describe();
        world.registry.set_availability(world.host.id, &id, state);
        assert_refused(
            &world.one(&world.scope(), restart("plex")).await,
            PlanRefusalCode::Unavailable,
            &described,
        );
    }
}

#[tokio::test]
async fn not_allowed_on_host_and_unavailable_are_accepted_with_a_note() {
    let world = World::new().await;
    let id = ContractId::parse("skill.restart_container").unwrap();
    for state in [
        Availability::NotAllowedOnHost {
            reason: NotAllowedOnHost::CeilingAbsent,
        },
        Availability::Unavailable {
            reason: UnavailableReason::SkillRunnerMissing,
        },
    ] {
        let described = state.describe();
        world.registry.set_availability(world.host.id, &id, state);
        let verdict = world.one(&world.scope(), restart("plex")).await;
        assert!(verdict.refused.is_empty(), "{:?}", verdict.refused);
        assert_eq!(
            verdict.accepted[0].note.as_deref(),
            Some(described.as_str())
        );
    }
}

#[tokio::test]
async fn execution_budget_spent_refuses() {
    let world = World::new().await;
    let mut scope = world.scope();
    scope.executions_left = Some(1);
    let first = world.one(&scope, restart("plex")).await;
    assert_eq!(first.accepted.len(), 1, "{:?}", first.refused);

    let second = world
        .validator()
        .validate(
            &scope,
            &first.accepted,
            &PlanInput {
                steps: vec![restart("plex-2")],
            },
        )
        .await;
    assert_refused(
        &second,
        PlanRefusalCode::BudgetSpent,
        "this operation has no execution budget left",
    );

    scope.executions_left = Some(0);
    assert_refused(
        &world.one(&scope, restart("plex")).await,
        PlanRefusalCode::BudgetSpent,
        "this operation has no execution budget left",
    );
}

#[tokio::test]
async fn an_open_identical_step_is_already_proposed() {
    let world = World::new().await;
    for outcome in [StepOutcome::Pending, StepOutcome::Running] {
        let mut scope = world.scope();
        scope.prior = vec![world.prior(StepId::new(), "plex", outcome)];
        let verdict = world.one(&scope, restart("plex")).await;
        assert_refused(
            &verdict,
            PlanRefusalCode::AlreadyProposed,
            "the same step is already waiting in this operation",
        );
        assert!(verdict.refused[0].code.is_duplicate());
    }
    // A different argument is a different step.
    let mut scope = world.scope();
    scope.prior = vec![world.prior(StepId::new(), "plex", StepOutcome::Pending)];
    assert_eq!(world.one(&scope, restart("plex-2")).await.accepted.len(), 1);
}

#[tokio::test]
async fn a_duplicate_step_in_one_call_is_refused() {
    let world = World::new().await;
    let verdict = world
        .run(&world.scope(), vec![restart("plex"), restart("plex")])
        .await;
    assert_eq!(verdict.accepted.len(), 1);
    assert_eq!(verdict.refused.len(), 1);
    let refusal = &verdict.refused[0];
    assert_eq!(refusal.index, 1);
    assert_eq!(refusal.code, PlanRefusalCode::DuplicateStep);
    assert_eq!(refusal.message, "the same step is already in your plan");
    assert!(refusal.code.is_duplicate());

    // The same step staged earlier in the turn counts too.
    let staged = verdict.accepted;
    let again = world
        .validator()
        .validate(
            &world.scope(),
            &staged,
            &PlanInput {
                steps: vec![restart("plex")],
            },
        )
        .await;
    assert_refused(
        &again,
        PlanRefusalCode::DuplicateStep,
        "the same step is already in your plan",
    );
}

#[tokio::test]
async fn a_blind_retry_is_refused() {
    let world = World::new().await;
    for outcome in [StepOutcome::Failed, StepOutcome::OutcomeUnknown] {
        let mut scope = world.scope();
        scope.prior = vec![world.prior(StepId::new(), "plex", outcome)];
        assert_refused(
            &world.one(&scope, restart("plex")).await,
            PlanRefusalCode::BlindRetry,
            "the same run failed earlier; say what changed with retry_of",
        );
    }
}

#[tokio::test]
async fn a_valid_retry_of_a_failed_step_passes_and_is_carried() {
    let world = World::new().await;
    for outcome in [StepOutcome::Failed, StepOutcome::OutcomeUnknown] {
        let failed = StepId::new();
        let mut scope = world.scope();
        scope.prior = vec![world.prior(failed, "plex", outcome)];
        let retried = PlanStepInput {
            retry_of: retry(&failed.to_string(), "  freed the port first  "),
            ..restart("plex")
        };
        let verdict = world.one(&scope, retried).await;
        assert!(verdict.refused.is_empty(), "{:?}", verdict.refused);
        let carried = verdict.accepted[0].retry_of.as_ref().unwrap();
        assert_eq!(carried.step, failed);
        assert_eq!(carried.changed, "freed the port first");
    }
}

#[tokio::test]
async fn a_succeeded_identical_run_is_not_a_blind_retry() {
    let world = World::new().await;
    let mut scope = world.scope();
    scope.prior = vec![world.prior(StepId::new(), "plex", StepOutcome::Succeeded)];
    assert_eq!(world.one(&scope, restart("plex")).await.accepted.len(), 1);
}

#[tokio::test]
async fn a_retry_claim_that_does_not_hold_is_refused() {
    let world = World::new().await;
    let failed = StepId::new();
    let succeeded = StepId::new();
    let mut scope = world.scope();
    scope.prior = vec![
        world.prior(failed, "plex", StepOutcome::Failed),
        world.prior(succeeded, "other", StepOutcome::Succeeded),
    ];
    let message = "retry_of must name a failed or unknown-outcome step of this operation and say what changed";
    let long = "x".repeat(281);
    let claims = [
        (succeeded.to_string(), "freed the port".to_owned()),
        (StepId::new().to_string(), "freed the port".to_owned()),
        ("not-a-step-id".to_owned(), "freed the port".to_owned()),
        (failed.to_string(), "   ".to_owned()),
        (failed.to_string(), String::new()),
        (failed.to_string(), long),
    ];
    for (id, changed) in claims {
        let claimed = PlanStepInput {
            retry_of: retry(&id, &changed),
            ..restart("plex")
        };
        assert_refused(
            &world.one(&scope, claimed).await,
            PlanRefusalCode::BadRetry,
            message,
        );
    }
}

#[tokio::test]
async fn a_fourth_step_is_refused() {
    let world = World::new().await;
    let steps = ["web-1", "web-2", "web-3", "web-4"]
        .into_iter()
        .map(restart)
        .collect();
    let verdict = world.run(&world.scope(), steps).await;
    assert_eq!(verdict.accepted.len(), 3);
    assert_eq!(verdict.refused.len(), 1);
    let refusal = &verdict.refused[0];
    assert_eq!(refusal.index, 3);
    assert_eq!(refusal.code, PlanRefusalCode::TooManySteps);
    assert_eq!(
        refusal.message,
        "at most 3 steps per investigation; this one was not taken"
    );

    // Steps staged earlier in the turn count against the same limit.
    let staged = verdict.accepted;
    let late = world
        .validator()
        .validate(
            &world.scope(),
            &staged,
            &PlanInput {
                steps: vec![restart("web-5")],
            },
        )
        .await;
    assert_refused(
        &late,
        PlanRefusalCode::TooManySteps,
        "at most 3 steps per investigation; this one was not taken",
    );
}

#[tokio::test]
async fn validation_text_is_redacted() {
    let world = World::new().await;
    let scope = world.scope();
    let hosted = PlanStepInput {
        host: Some(SECRET.to_owned()),
        ..restart("plex")
    };
    let targeted = PlanStepInput {
        target: Some(SECRET.to_owned()),
        ..restart("plex")
    };
    let keyed = PlanStepInput {
        args: BTreeMap::from([
            ("container".to_owned(), ArgValue::Text("plex".to_owned())),
            (SECRET.to_owned(), ArgValue::Text(SECRET.to_owned())),
        ]),
        ..step("skill.restart_container")
    };
    let verdict = world
        .run(
            &scope,
            vec![
                hosted,
                targeted,
                keyed,
                step(&format!("container.{SECRET}")),
                step(&format!("skill.{SECRET}")),
            ],
        )
        .await;
    assert_eq!(verdict.refused.len(), 5, "{:?}", verdict.refused);
    for refusal in &verdict.refused {
        let mut texts = vec![
            refusal.contract.clone(),
            refusal.message.clone(),
            refusal.dropped_line(),
        ];
        texts.extend(refusal.hint.clone());
        for text in texts {
            assert!(!text.contains(SECRET), "leaked in {text:?}");
        }
    }
    assert_eq!(verdict.refused[0].message, "no host named `[REDACTED]`");
    assert_eq!(
        verdict.refused[1].message,
        "target `[REDACTED]` is not this investigation's target (`plex`)"
    );
    assert_eq!(
        verdict.refused[2].message,
        "bad arguments: unknown argument `[REDACTED]`"
    );
    assert_eq!(
        verdict.refused[3].message,
        "no capability `container.[REDACTED]` on this host"
    );
    assert_eq!(verdict.refused[3].contract, "container.[REDACTED]");
    assert_eq!(verdict.refused[4].contract, "skill.[REDACTED]");
}
