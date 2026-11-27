//! The capability registry: every precedence row of the availability table, driven through
//! a scripted agent, a real in-memory store and the hub's own contract index.

use std::collections::BTreeMap;
use std::sync::Arc;

use domovoi_core::capability::{
    Availability, CapabilityContract, CatalogImplementation, CatalogPageData, CatalogueGap,
    CatalogueState, ContractCategory, ContractId, ContractIndex, ContractSource, ContractVersion,
    EffectClass, ExecutionPath, Idempotency, NotAllowedOnHost, READ_OUTPUT_LIMIT, Reversibility,
    TargetKind, UnavailableReason, VerificationHint, builtin_contracts, catalog_hash,
};
use domovoi_core::ports::{
    AgentLink, CapabilityRegistry, HelperState, HostRepo, HostWords, Presence, SettingsRepo,
};
use domovoi_core::{Host, HostId, Risk, ScriptDigest, SkillName, SkillSummary};
use domovoi_hub::capability::{HubRegistry, RegistryDeps, host_skills_key};
use domovoi_testkit::build::{host, manifest};
use domovoi_testkit::{FakeAgentLink, FakeClock, InMemoryStore};
use jiff::{SignedDuration, Timestamp};
use serde_json::{Value, json};

use crate::api_harness::api;

const T0: Timestamp = Timestamp::constant(1_700_000_000, 0);
const T1: Timestamp = Timestamp::constant(1_700_000_600, 0);

fn id(text: &str) -> ContractId {
    match ContractId::parse(text) {
        Ok(id) => id,
        Err(err) => panic!("{text} is not a contract id: {err}"),
    }
}

fn name(text: &str) -> SkillName {
    match SkillName::parse(text) {
        Ok(name) => name,
        Err(err) => panic!("{text} is not a skill name: {err}"),
    }
}

fn skill_id(text: &str) -> ContractId {
    ContractId::skill(&name(text))
}

fn summary(text: &str, risk: Risk) -> SkillSummary {
    match manifest(text, risk) {
        Ok(manifest) => manifest.summary(ScriptDigest::of_bytes(b"fixture entry")),
        Err(err) => panic!("{text} has no manifest: {err}"),
    }
}

/// A contract built for one test, the way the capability module's own tests build them.
fn crafted(text: &str, effect: EffectClass, execution: ExecutionPath) -> CapabilityContract {
    CapabilityContract {
        id: id(text),
        version: match ContractVersion::new(1) {
            Ok(version) => version,
            Err(err) => panic!("{err}"),
        },
        title: "Crafted".to_owned(),
        description: "A contract built for a test.".to_owned(),
        input_schema: json!({
            "type": "object",
            "properties": {},
            "required": [],
            "additionalProperties": false,
        }),
        effect,
        category: match ContractCategory::parse(match execution {
            ExecutionPath::SkillRunner => "skill.mutate",
            _ => "container.mutate",
        }) {
            Ok(category) => category,
            Err(err) => panic!("{err}"),
        },
        target_kind: TargetKind::Host,
        target_arg: None,
        reversibility: if effect == EffectClass::Read {
            Reversibility::NotApplicable
        } else {
            Reversibility::Reversible
        },
        idempotency: if effect == EffectClass::Read {
            Idempotency::Idempotent
        } else {
            Idempotency::NotIdempotent
        },
        timeout: SignedDuration::from_secs(15),
        output_limit_bytes: READ_OUTPUT_LIMIT,
        verification: if effect == EffectClass::Read {
            VerificationHint::None
        } else {
            VerificationHint::Recheck
        },
        constrainable_args: Vec::new(),
        safety_facts: Vec::new(),
        execution,
        source: ContractSource::BuiltIn,
    }
}

fn mutable(text: &str) -> CapabilityContract {
    crafted(text, EffectClass::Mutate, ExecutionPath::Agent)
}

/// The hash an agent would announce for `entries`.
fn hash_of(entries: &[(&str, u32)]) -> String {
    let map: BTreeMap<ContractId, ContractVersion> = entries
        .iter()
        .map(|(contract, at)| {
            let version = match ContractVersion::new(*at) {
                Ok(version) => version,
                Err(err) => panic!("{err}"),
            };
            (id(contract), version)
        })
        .collect();
    catalog_hash(&map)
}

fn page(entries: &[(&str, u32)], next: Option<u32>) -> CatalogPageData {
    CatalogPageData {
        entries: entries
            .iter()
            .map(|(contract, at)| CatalogImplementation {
                contract: id(contract),
                version: match ContractVersion::new(*at) {
                    Ok(version) => version,
                    Err(err) => panic!("{err}"),
                },
            })
            .collect(),
        next_page: next,
    }
}

/// A ceiling summary the agent could announce for `entries`, optionally hashed.
fn ceiling_with(entries: &[(&str, u32)], hash: Option<&str>) -> String {
    let entries = entries
        .iter()
        .map(|(contract, rate)| {
            format!(
                "{{\"contract\":\"{contract}\",\"targets\":{{\"kind\":\"any\"}},\"max_per_hour\":{rate},\"args\":{{}}}}"
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    match hash {
        None => format!("{{\"enabled\":true,\"entries\":[{entries}]}}"),
        Some(hash) => format!("{{\"enabled\":true,\"entries\":[{entries}],\"hash\":\"{hash}\"}}"),
    }
}

/// A ceiling summary the agent could announce for `entries`.
fn ceiling(entries: &[(&str, u32)]) -> String {
    ceiling_with(entries, None)
}

/// A connected host, its scripted agent and the registry over both.
struct Fixture {
    host: Host,
    link: Arc<FakeAgentLink>,
    registry: HubRegistry,
}

impl Fixture {
    async fn new(extra: Vec<CapabilityContract>) -> Self {
        Self::with_settings(extra, Vec::new()).await
    }

    async fn with_settings(extra: Vec<CapabilityContract>, saved: Vec<(String, Value)>) -> Self {
        let nas = match host("nas") {
            Ok(nas) => nas,
            Err(err) => panic!("{err}"),
        };
        let store = Arc::new(InMemoryStore::new());
        let hosts: Arc<dyn HostRepo> = store.clone();
        let settings: Arc<dyn SettingsRepo> = store.clone();
        hosts
            .upsert_host(&nas)
            .await
            .unwrap_or_else(|err| panic!("{err}"));
        for (key, value) in saved {
            settings
                .set(&key, &value)
                .await
                .unwrap_or_else(|err| panic!("{err}"));
        }
        let link = Arc::new(FakeAgentLink::new());
        link.set_presence(
            nas.id,
            Presence::Connected {
                since: T0,
                version: "0.1.0".to_owned(),
            },
        );
        // A host with a working container runtime, unless a test says otherwise.
        link.set_host_words(
            nas.id,
            HostWords {
                container_runtime: true,
                ..HostWords::default()
            },
        );
        let mut contracts = builtin_contracts();
        contracts.extend(extra);
        let index = match ContractIndex::new(contracts) {
            Ok(index) => Arc::new(index),
            Err(err) => panic!("the index is invalid: {err}"),
        };
        let agent: Arc<dyn AgentLink> = link.clone();
        let registry = HubRegistry::new(RegistryDeps {
            contracts: index,
            agent: Arc::new(domovoi_hub::runtime::caps::LinkCapabilities::new(agent)),
            hosts: hosts.clone(),
            store: settings,
            clock: Arc::new(FakeClock::new(T0)),
        })
        .await
        .unwrap_or_else(|err| panic!("the registry was refused: {err}"));
        Self {
            host: nas,
            link,
            registry,
        }
    }

    fn host_id(&self) -> HostId {
        self.host.id
    }

    /// Announces a complete catalogue holding `entries`.
    fn complete(&self, entries: &[(&str, u32)]) {
        self.link.set_catalog_hash(self.host.id, hash_of(entries));
        self.link
            .set_catalog(self.host.id, vec![page(entries, None)]);
    }

    /// Announces a hash and nothing else: the walk fails at its first page.
    fn unreadable(&self) {
        self.link
            .set_catalog_hash(self.host.id, hash_of(&[("container.logs", 1)]));
        self.link.on_catalog(|_host, _query, _deadline| {
            Err(domovoi_core::ports::LinkError::Rejected(
                "the page never arrived".to_owned(),
            ))
        });
    }

    fn words(&self, words: HostWords) {
        self.link.set_host_words(self.host.id, words);
    }

    async fn availability(&self, contract: &str) -> Availability {
        self.registry
            .availability(self.host.id, &id(contract))
            .await
    }

    async fn skill_state(&self, text: &str) -> Availability {
        self.registry
            .availability(self.host.id, &skill_id(text))
            .await
    }

    async fn row(&self, contract: &str) -> domovoi_core::capability::AvailabilityEntry {
        self.registry
            .inventory(self.host.id)
            .await
            .into_iter()
            .find(|entry| entry.contract == id(contract))
            .unwrap_or_else(|| panic!("{contract} has no row"))
    }
}

fn absent(reason: NotAllowedOnHost) -> Availability {
    Availability::NotAllowedOnHost { reason }
}

fn missing(reason: UnavailableReason) -> Availability {
    Availability::Unavailable { reason }
}

#[tokio::test]
async fn availability_is_host_unknown_before_any_session() {
    let fixture = Fixture::new(Vec::new()).await;
    fixture.link.set_presence(
        fixture.host_id(),
        Presence::Disconnected { last_seen: None },
    );

    assert_eq!(
        fixture.availability("container.logs").await,
        Availability::HostUnknown
    );
}

#[tokio::test]
async fn availability_is_host_absent_after_a_session_ends() {
    let fixture = Fixture::new(Vec::new()).await;
    fixture.link.set_presence(
        fixture.host_id(),
        Presence::Disconnected {
            last_seen: Some(T1),
        },
    );

    assert_eq!(
        fixture.availability("container.logs").await,
        Availability::HostAbsent { since: T1 }
    );
}

#[tokio::test]
async fn availability_is_host_incompatible_for_another_protocol() {
    let fixture = Fixture::new(Vec::new()).await;
    fixture.link.set_availability(
        fixture.host_id(),
        domovoi_core::ports::HostAvailability::Incompatible {
            reported: "1.1".to_owned(),
        },
    );

    assert_eq!(
        fixture.availability("container.logs").await,
        Availability::HostIncompatible {
            reported: "1.1".to_owned()
        }
    );
}

#[tokio::test]
async fn available_contract_is_available() {
    let fixture = Fixture::new(Vec::new()).await;
    fixture.complete(&[("container.logs", 1)]);

    assert_eq!(
        fixture.availability("container.logs").await,
        Availability::Available
    );
}

#[tokio::test]
async fn not_installed_requires_a_complete_catalogue() {
    let fixture = Fixture::new(Vec::new()).await;
    fixture.complete(&[("container.logs", 1)]);

    assert_eq!(
        fixture.availability("container.inspect").await,
        Availability::NotInstalled
    );

    // The same contract on a catalogue that never finished proves nothing about what is
    // missing, so it is unavailable instead of not installed.
    let again = Fixture::new(Vec::new()).await;
    again.unreadable();
    assert_eq!(
        again.availability("container.inspect").await,
        missing(UnavailableReason::CatalogueIncomplete)
    );
}

#[tokio::test]
async fn disabled_skill_is_disabled_by_hub() {
    let fixture = Fixture::with_settings(
        Vec::new(),
        vec![(
            host_skills_key("nas"),
            json!({ "disabled": ["prune_logs"] }),
        )],
    )
    .await;
    fixture.link.set_skills(
        fixture.host_id(),
        vec![
            summary("prune_logs", Risk::Read),
            summary("zpool_status", Risk::Read),
        ],
    );
    fixture.complete(&[("skill.prune_logs", 1), ("skill.zpool_status", 1)]);
    fixture.words(HostWords {
        runs_as_root: true,
        ..HostWords::default()
    });

    assert_eq!(
        fixture.skill_state("prune_logs").await,
        Availability::DisabledByHub
    );
    assert_eq!(
        fixture.skill_state("zpool_status").await,
        Availability::Available
    );
}

#[tokio::test]
async fn mutating_contract_without_a_ceiling_is_not_allowed_on_host() {
    let fixture = Fixture::new(vec![mutable("firewall.block_source")]).await;
    fixture.complete(&[("container.logs", 1), ("firewall.block_source", 1)]);

    assert_eq!(
        fixture.availability("firewall.block_source").await,
        absent(NotAllowedOnHost::CeilingAbsent)
    );
    assert_eq!(
        fixture.availability("container.logs").await,
        Availability::Available
    );
}

#[tokio::test]
async fn ceiling_entry_must_match_the_exact_contract() {
    let fixture = Fixture::new(vec![mutable("firewall.block_source")]).await;
    fixture.complete(&[("firewall.block_source", 1)]);
    fixture.words(HostWords {
        ceiling_summary: Some(ceiling(&[("firewall.block_source", 3)])),
        ..HostWords::default()
    });
    assert_eq!(
        fixture.availability("firewall.block_source").await,
        Availability::Available
    );
    fixture.words(HostWords {
        ceiling_summary: Some(ceiling(&[("unit.restart", 3)])),
        ..HostWords::default()
    });
    assert_eq!(
        fixture.availability("firewall.block_source").await,
        absent(NotAllowedOnHost::CeilingEntryMissing)
    );
}

#[tokio::test]
async fn a_mutating_contract_a_person_approves_needs_no_ceiling_entry() {
    // Package contracts and the like run only on a person's approval: the ceiling limits
    // unattended runs, so its absence must not make them unavailable.
    let fixture = Fixture::new(vec![mutable("agent.package.install")]).await;
    fixture.complete(&[("agent.package.install", 1)]);
    assert_eq!(
        fixture.availability("agent.package.install").await,
        Availability::Available
    );
}

#[tokio::test]
async fn unprivileged_agent_without_a_runner_cannot_run_skills() {
    let fixture = Fixture::new(Vec::new()).await;
    fixture
        .link
        .set_skills(fixture.host_id(), vec![summary("prune_logs", Risk::Read)]);
    fixture.complete(&[("skill.prune_logs", 1)]);

    assert_eq!(
        fixture.skill_state("prune_logs").await,
        missing(UnavailableReason::SkillRunnerMissing)
    );
}

#[tokio::test]
async fn root_agent_without_a_runner_keeps_skills_available() {
    let fixture = Fixture::new(Vec::new()).await;
    fixture
        .link
        .set_skills(fixture.host_id(), vec![summary("prune_logs", Risk::Read)]);
    fixture.complete(&[("skill.prune_logs", 1)]);
    fixture.words(HostWords {
        runs_as_root: true,
        ..HostWords::default()
    });

    assert_eq!(
        fixture.skill_state("prune_logs").await,
        Availability::Available
    );
    let row = fixture.row("skill.prune_logs").await;
    assert!(row.root_agent, "the row lost the root label");
    assert!(
        !row.runner_installed,
        "the row claims a runner it has not got"
    );
}

#[tokio::test]
async fn helper_status_does_not_change_the_registry() {
    let fixture = Fixture::new(Vec::new()).await;
    fixture.complete(&[
        ("container.logs", 1),
        ("container.inspect", 1),
        ("host.overview", 1),
    ]);
    let without = fixture.registry.inventory(fixture.host_id()).await;

    for helper in [HelperState::Absent, HelperState::Failed, HelperState::Ready] {
        fixture.words(HostWords {
            helper,
            container_runtime: true,
            ..HostWords::default()
        });
        let with = fixture.registry.inventory(fixture.host_id()).await;
        let states = |rows: &[domovoi_core::capability::AvailabilityEntry]| {
            rows.iter()
                .map(|row| (row.contract.clone(), row.state.clone()))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            states(&with),
            states(&without),
            "helper {helper:?} moved a row"
        );
    }
}

#[tokio::test]
async fn container_runtime_unavailable_is_reported() {
    let fixture = Fixture::new(Vec::new()).await;
    fixture.complete(&[("container.logs", 1)]);
    fixture.words(HostWords {
        container_runtime: false,
        ..HostWords::default()
    });

    assert_eq!(
        fixture.availability("container.logs").await,
        missing(UnavailableReason::ContainerRuntimeUnavailable)
    );
}

#[tokio::test]
async fn precedence_host_state_outranks_everything() {
    let fixture = Fixture::with_settings(
        Vec::new(),
        vec![(
            host_skills_key("nas"),
            json!({ "disabled": ["prune_logs"] }),
        )],
    )
    .await;
    fixture
        .link
        .set_skills(fixture.host_id(), vec![summary("prune_logs", Risk::Read)]);
    fixture.unreadable();
    fixture.link.set_presence(
        fixture.host_id(),
        Presence::Disconnected {
            last_seen: Some(T1),
        },
    );

    assert_eq!(
        fixture.skill_state("prune_logs").await,
        Availability::HostAbsent { since: T1 }
    );
}

#[tokio::test]
async fn precedence_disable_outranks_not_installed() {
    let fixture = Fixture::with_settings(
        Vec::new(),
        vec![(
            host_skills_key("nas"),
            json!({ "disabled": ["prune_logs"] }),
        )],
    )
    .await;
    fixture.complete(&[]);

    assert_eq!(
        fixture.skill_state("prune_logs").await,
        Availability::DisabledByHub
    );
}

#[tokio::test]
async fn precedence_not_installed_outranks_ceiling() {
    let fixture = Fixture::new(vec![mutable("container.prune")]).await;
    fixture.complete(&[("container.logs", 1)]);

    assert_eq!(
        fixture.availability("container.prune").await,
        Availability::NotInstalled
    );
}

#[tokio::test]
async fn partial_catalogue_is_never_available() {
    // Nothing announced at all.
    let silent = Fixture::new(Vec::new()).await;
    assert_eq!(
        silent.availability("container.logs").await,
        missing(UnavailableReason::CatalogueIncomplete)
    );

    // A hash the pages do not match.
    let mismatched = Fixture::new(Vec::new()).await;
    mismatched
        .link
        .set_catalog_hash(mismatched.host_id(), "a".repeat(64));
    mismatched.link.set_catalog(
        mismatched.host_id(),
        vec![page(&[("container.logs", 1)], None)],
    );
    assert_eq!(
        mismatched.availability("container.logs").await,
        missing(UnavailableReason::CatalogueHashMismatch)
    );

    // An agent that cannot page at all.
    let unsupported = Fixture::new(Vec::new()).await;
    unsupported
        .link
        .set_catalog_hash(unsupported.host_id(), hash_of(&[]));
    unsupported.link.on_catalog(|_host, _query, _deadline| {
        Err(domovoi_core::ports::LinkError::Rejected(
            domovoi_core::ports::CATALOGUE_UNSUPPORTED.to_owned(),
        ))
    });
    assert_eq!(
        unsupported.availability("container.logs").await,
        missing(UnavailableReason::CatalogueIncomplete)
    );

    // A page that failed.
    let failed = Fixture::new(Vec::new()).await;
    failed.unreadable();
    assert_eq!(
        failed.availability("container.logs").await,
        missing(UnavailableReason::CatalogueIncomplete)
    );
    assert_ne!(
        failed.availability("container.logs").await,
        Availability::NotInstalled
    );
}

#[tokio::test]
async fn unknown_contract_is_recorded_and_never_offered() {
    let fixture = Fixture::new(Vec::new()).await;
    fixture.complete(&[("container.logs", 1), ("container.destroy", 1)]);

    let rows = fixture.registry.inventory(fixture.host_id()).await;
    assert!(
        rows.iter()
            .all(|row| row.contract != id("container.destroy")),
        "an id the hub did not compile in became a row"
    );
    assert!(
        fixture
            .registry
            .unknown_contracts(fixture.host_id())
            .contains(&"container.destroy".to_owned()),
        "the unknown id was not recorded"
    );
}

#[tokio::test]
async fn matching_hello_skill_and_catalogue_echo_produces_skill_contract() {
    let fixture = Fixture::new(Vec::new()).await;
    let announced = summary("prune_logs", Risk::Read);
    fixture
        .link
        .set_skills(fixture.host_id(), vec![announced.clone()]);
    fixture.complete(&[("skill.prune_logs", 1)]);
    fixture.words(HostWords {
        runs_as_root: true,
        ..HostWords::default()
    });

    let rows = fixture.registry.inventory(fixture.host_id()).await;
    let matching = rows
        .iter()
        .filter(|row| row.contract == skill_id("prune_logs"))
        .collect::<Vec<_>>();
    assert_eq!(matching.len(), 1, "the skill produced more than one row");
    let row = matching[0];
    let built = domovoi_core::capability::skill_contract(&announced, None);
    assert_eq!(row.state, Availability::Available);
    assert_eq!(row.title, built.title, "the row's title is not the skill's");
    assert_eq!(
        row.effect, built.effect,
        "the row's class is not the skill's"
    );
    assert!(row.from_hello_skills, "the row says it came from a page");
}

#[tokio::test]
async fn skill_catalogue_entry_without_hello_summary_is_diagnostic_only() {
    let fixture = Fixture::new(Vec::new()).await;
    fixture.complete(&[("skill.ghost", 1)]);

    let rows = fixture.registry.inventory(fixture.host_id()).await;
    assert!(
        rows.iter().all(|row| row.contract != skill_id("ghost")),
        "a catalogue echo created a skill contract"
    );
    assert!(
        fixture
            .registry
            .unknown_contracts(fixture.host_id())
            .contains(&"skill.ghost".to_owned()),
        "the echo was not kept as a diagnostic"
    );
}

#[tokio::test]
async fn hello_skill_missing_from_complete_catalogue_is_unavailable() {
    let fixture = Fixture::new(Vec::new()).await;
    fixture
        .link
        .set_skills(fixture.host_id(), vec![summary("prune_logs", Risk::Read)]);
    fixture.complete(&[("container.logs", 1)]);
    fixture.words(HostWords {
        runs_as_root: true,
        ..HostWords::default()
    });

    assert_eq!(
        fixture.skill_state("prune_logs").await,
        missing(UnavailableReason::CatalogueInconsistent)
    );
}

#[tokio::test]
async fn hub_executed_contracts_ignore_the_catalogue() {
    let fixture = Fixture::new(Vec::new()).await;
    fixture.unreadable();

    for contract in ["incident.history", "notes.search"] {
        assert_eq!(
            fixture.availability(contract).await,
            Availability::Available,
            "{contract} depended on the agent's catalogue"
        );
    }
}

#[tokio::test]
async fn hub_executed_contracts_are_not_in_agent_catalogue() {
    let agent_path = builtin_contracts()
        .into_iter()
        .filter(|contract| contract.execution == ExecutionPath::Agent)
        .map(|contract| contract.id.to_string())
        .collect::<Vec<_>>();
    let hub_path = builtin_contracts()
        .into_iter()
        .filter(|contract| contract.execution == ExecutionPath::Hub)
        .map(|contract| contract.id.to_string())
        .collect::<Vec<_>>();

    assert_eq!(hub_path, vec!["incident.history", "notes.search"]);
    for hub in &hub_path {
        assert!(
            !agent_path.contains(hub),
            "{hub} is both a hub and an agent contract"
        );
    }
    assert_eq!(agent_path.len(), 9, "the agent path is not nine contracts");
}

#[tokio::test]
async fn agent_catalogue_cannot_create_a_contract() {
    let fixture = Fixture::new(Vec::new()).await;
    let before = fixture.registry.contracts().len();
    fixture.complete(&[("container.logs", 1), ("container.destroy", 1)]);

    let _ = fixture.registry.inventory(fixture.host_id()).await;

    assert_eq!(fixture.registry.contracts().len(), before);
}

#[tokio::test]
async fn disabled_skill_hidden_from_enabled_skills() {
    let fixture = Fixture::with_settings(
        Vec::new(),
        vec![(
            host_skills_key("nas"),
            json!({ "disabled": ["prune_logs"] }),
        )],
    )
    .await;
    fixture.link.set_skills(
        fixture.host_id(),
        vec![
            summary("prune_logs", Risk::Read),
            summary("zpool_status", Risk::Read),
        ],
    );

    let offered = fixture.registry.enabled_skills(fixture.host_id());
    assert_eq!(
        offered
            .iter()
            .map(|summary| summary.name.to_string())
            .collect::<Vec<_>>(),
        vec!["zpool_status".to_owned()]
    );
    assert_eq!(
        fixture.link.skills(fixture.host_id()).len(),
        2,
        "the agent's own list was filtered"
    );
}

#[tokio::test]
async fn effective_effect_comes_from_contracts_only() {
    let fixture = Fixture::new(Vec::new()).await;
    fixture
        .link
        .set_skills(fixture.host_id(), vec![summary("prune_logs", Risk::Read)]);
    let skill = name("prune_logs");

    assert_eq!(
        fixture.registry.effective_effect(fixture.host_id(), &skill),
        Some(EffectClass::Read),
        "the declared class was not used"
    );

    fixture
        .registry
        .set_floor(&fixture.host, skill.clone(), Some(EffectClass::Destructive))
        .await
        .unwrap_or_else(|err| panic!("{err}"));

    assert_eq!(
        fixture.registry.effective_effect(fixture.host_id(), &skill),
        Some(EffectClass::Destructive),
        "the floor did not raise the class"
    );
}

#[tokio::test]
async fn malformed_ceiling_is_not_permissive() {
    let malformed = [
        "not json at all",
        "{}",
        "{\"enabled\":true,\"entries\":[{\"contract\":\"not an id\",\"targets\":{\"kind\":\"any\"},\"max_per_hour\":3,\"args\":{}}]}",
        "{\"enabled\":true,\"entries\":[{\"contract\":\"container.restart\",\"targets\":{\"kind\":\"names\",\"names\":[\" plex\"]},\"max_per_hour\":3,\"args\":{}}]}",
        "{\"enabled\":true,\"entries\":[{\"contract\":\"container.restart\",\"targets\":{\"kind\":\"any\"},\"max_per_hour\":-1,\"args\":{}}]}",
    ];
    for summary in malformed {
        let fixture = Fixture::new(vec![mutable("firewall.block_source")]).await;
        fixture.complete(&[("firewall.block_source", 1)]);
        fixture.words(HostWords {
            ceiling_summary: Some(summary.to_owned()),
            ..HostWords::default()
        });
        assert_eq!(
            fixture.availability("firewall.block_source").await,
            absent(NotAllowedOnHost::CeilingAbsent),
            "{summary} was read as permission"
        );
    }
}

#[tokio::test]
async fn ceiling_hash_is_opaque() {
    let plain = ceiling(&[("container.restart", 3)]);
    let hashed = ceiling_with(
        &[("container.restart", 3)],
        Some("00000000000000000000000000000000"),
    );
    assert_ne!(plain, hashed, "the two summaries are the same text");

    for summary in [plain, hashed] {
        let fixture = Fixture::new(vec![mutable("container.restart")]).await;
        fixture.complete(&[("container.restart", 1)]);
        fixture.words(HostWords {
            ceiling_summary: Some(summary),
            ..HostWords::default()
        });
        assert_eq!(
            fixture.availability("container.restart").await,
            Availability::Available
        );
    }
}

#[tokio::test]
async fn inventory_reports_the_catalogue_state_per_row() {
    let fixture = Fixture::new(Vec::new()).await;
    fixture.complete(&[("container.logs", 1)]);

    let row = fixture.row("container.logs").await;
    assert_eq!(
        row.catalogue,
        CatalogueState::Complete {
            hash: hash_of(&[("container.logs", 1)]),
        }
    );
    assert_eq!(row.execution, ExecutionPath::Agent);
    assert_eq!(row.target_kind, TargetKind::Container);
}

#[tokio::test]
async fn inventory_of_a_disconnected_host_reports_no_reading() {
    let fixture = Fixture::new(Vec::new()).await;
    fixture.link.set_presence(
        fixture.host_id(),
        Presence::Disconnected { last_seen: None },
    );

    let row = fixture.row("container.logs").await;
    assert_eq!(row.state, Availability::HostUnknown);
    assert_eq!(
        row.catalogue,
        CatalogueState::Partial {
            gap: CatalogueGap::NotWalked,
        }
    );
    assert!(
        fixture.link.catalog_calls().is_empty(),
        "an unknown host was walked"
    );
}

#[tokio::test]
async fn a_real_agent_catalogue_walks_to_complete() {
    // The agent builds its list from the compiled contracts and the loaded skills; the list
    // is copied here because the agent crate is not a hub dev-dependency.
    let entries = builtin_contracts()
        .into_iter()
        .filter(|contract| contract.execution == ExecutionPath::Agent)
        .map(|contract| (contract.id.to_string(), contract.version.get()))
        .chain([("skill.prune_logs".to_owned(), 1)])
        .collect::<Vec<(String, u32)>>();
    let listed = entries
        .iter()
        .map(|(contract, at)| (contract.as_str(), *at))
        .collect::<Vec<(&str, u32)>>();

    let fixture = Fixture::new(Vec::new()).await;
    fixture.complete(&listed);

    let snapshot = fixture.registry.catalog(fixture.host_id()).await;

    assert_eq!(
        snapshot.state,
        CatalogueState::Complete {
            hash: hash_of(&listed)
        }
    );
    assert_eq!(snapshot.pages, 1);
    assert_eq!(snapshot.entries.len(), listed.len());
}

#[tokio::test]
async fn capability_endpoint_requires_auth() {
    let a = api().await;
    let reply = a
        .send(
            axum::http::Method::GET,
            "/api/v1/hosts/nas/capabilities",
            None,
            None,
        )
        .await;
    assert_eq!(reply.status, axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn capability_endpoint_404s_on_an_unknown_host() {
    let a = api().await;
    let login = a.admin().await;
    let reply = a.get("/api/v1/hosts/nope/capabilities", &login).await;
    assert_eq!(reply.status, axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn capability_dto_covers_every_field() {
    let a = api().await;
    let login = a.admin().await;
    a.link.set_presence(
        a.nas.id,
        domovoi_core::ports::Presence::Connected {
            since: a.now(),
            version: "0.6.0".to_owned(),
        },
    );
    a.link.set_host_words(
        a.nas.id,
        domovoi_core::ports::HostWords {
            container_runtime: true,
            ..domovoi_core::ports::HostWords::default()
        },
    );
    a.link
        .set_catalog_hash(a.nas.id, hash_of(&[("container.logs", 1)]));
    a.link
        .set_catalog(a.nas.id, vec![page(&[("container.logs", 1)], None)]);

    let reply = a.get("/api/v1/hosts/nas/capabilities", &login).await;

    assert_eq!(reply.status, axum::http::StatusCode::OK, "{}", reply.text);
    let entries = reply.body["entries"].as_array().expect("entries");
    assert!(!entries.is_empty());
    let states = Availability::all()
        .iter()
        .map(Availability::code)
        .collect::<Vec<_>>();
    for entry in entries {
        for field in [
            "contract",
            "title",
            "category",
            "effect",
            "state",
            "catalogue",
            "target_kind",
            "execution",
            "root_agent",
            "runner_installed",
        ] {
            assert!(!entry[field].is_null(), "{field} is missing from {entry}");
        }
        let state = entry["state"].as_str().expect("a state code");
        assert!(states.contains(&state), "{state} is not one of the eight");
    }
    let logs = entries
        .iter()
        .find(|entry| entry["contract"] == "container.logs")
        .expect("the container.logs row");
    assert_eq!(logs["state"], "available");
}

#[tokio::test]
async fn capability_endpoint_reports_unknown_contracts() {
    let a = api().await;
    let login = a.admin().await;
    a.link.set_presence(
        a.nas.id,
        domovoi_core::ports::Presence::Connected {
            since: a.now(),
            version: "0.6.0".to_owned(),
        },
    );
    a.link.set_catalog_hash(
        a.nas.id,
        hash_of(&[("container.logs", 1), ("container.destroy", 1)]),
    );
    a.link.set_catalog(
        a.nas.id,
        vec![page(
            &[("container.logs", 1), ("container.destroy", 1)],
            None,
        )],
    );

    let reply = a.get("/api/v1/hosts/nas/capabilities", &login).await;

    assert_eq!(reply.status, axum::http::StatusCode::OK, "{}", reply.text);
    let unknown = reply.body["unknown_contracts"]
        .as_array()
        .expect("unknown_contracts");
    assert!(unknown.iter().any(|id| id == "container.destroy"));
    let entries = reply.body["entries"].as_array().expect("entries");
    assert!(
        entries
            .iter()
            .all(|entry| entry["contract"] != "container.destroy")
    );
}

/// A host whose availability and view are scripted, so a test can force a session that
/// cannot be read.
struct ScriptedHost {
    availabilities:
        std::sync::Mutex<std::collections::VecDeque<domovoi_core::ports::HostAvailability>>,
    last: std::sync::Mutex<domovoi_core::ports::HostAvailability>,
    view: Option<domovoi_core::ports::HostView>,
}

impl ScriptedHost {
    fn new(
        availabilities: Vec<domovoi_core::ports::HostAvailability>,
        view: Option<domovoi_core::ports::HostView>,
    ) -> Self {
        let last = availabilities
            .last()
            .cloned()
            .unwrap_or(domovoi_core::ports::HostAvailability::Unknown);
        Self {
            availabilities: std::sync::Mutex::new(availabilities.into()),
            last: std::sync::Mutex::new(last),
            view,
        }
    }
}

#[async_trait::async_trait]
impl domovoi_hub::capability::HostCapabilities for ScriptedHost {
    async fn availability(&self, _host: HostId) -> domovoi_core::ports::HostAvailability {
        let next = self
            .availabilities
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pop_front();
        match next {
            Some(state) => {
                *self
                    .last
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = state.clone();
                state
            }
            None => self
                .last
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone(),
        }
    }

    fn catalog_hash(&self, _host: HostId) -> Option<String> {
        None
    }

    async fn catalog_page(
        &self,
        _host: HostId,
        _query: domovoi_core::capability::CatalogQuery,
        _deadline: Timestamp,
    ) -> Result<domovoi_core::capability::CatalogPageData, domovoi_core::ports::LinkError> {
        Err(domovoi_core::ports::LinkError::NotConnected)
    }

    fn view(&self, _host: HostId) -> Option<domovoi_core::ports::HostView> {
        self.view.clone()
    }
}

/// A registry over `agent` for the `nas` host, with no settings.
async fn registry_over(agent: Arc<ScriptedHost>) -> (HubRegistry, Host) {
    let nas = match host("nas") {
        Ok(host) => host,
        Err(err) => panic!("{err}"),
    };
    let store = Arc::new(InMemoryStore::new());
    let hosts: Arc<dyn HostRepo> = store.clone();
    hosts
        .upsert_host(&nas)
        .await
        .unwrap_or_else(|err| panic!("{err}"));
    let contracts = match ContractIndex::new(builtin_contracts()) {
        Ok(index) => Arc::new(index),
        Err(err) => panic!("{err}"),
    };
    let registry = HubRegistry::new(RegistryDeps {
        contracts,
        agent,
        hosts: hosts.clone(),
        store: store.clone(),
        clock: Arc::new(FakeClock::new(T0)),
    })
    .await
    .unwrap_or_else(|err| panic!("{err}"));
    (registry, nas)
}

#[tokio::test]
async fn a_live_session_that_cannot_be_read_proves_nothing() {
    // Availability says connected, but the link cannot describe the session. Reading that as
    // a default view would claim "runner absent, no ceiling, no container runtime"; the
    // honest answer is that nothing about this host is proved.
    let agent = Arc::new(ScriptedHost::new(
        vec![
            domovoi_core::ports::HostAvailability::Known { since: T0 },
            domovoi_core::ports::HostAvailability::Known { since: T0 },
        ],
        None,
    ));
    let (registry, nas) = registry_over(agent).await;

    let rows = registry.inventory(nas.id).await;

    assert!(!rows.is_empty());
    for row in &rows {
        assert_eq!(row.state, Availability::HostUnknown, "{}", row.contract);
        assert_eq!(
            row.catalogue,
            CatalogueState::Partial {
                gap: CatalogueGap::NotWalked,
            }
        );
    }
}

#[tokio::test]
async fn a_session_that_ends_during_collection_reports_absent() {
    // The first read sees a live session, the view fails, and the re-read learns the session
    // ended: the answer is what the host is now, never a report of absence for the facts.
    let agent = Arc::new(ScriptedHost::new(
        vec![
            domovoi_core::ports::HostAvailability::Known { since: T0 },
            domovoi_core::ports::HostAvailability::Absent { since: T1 },
        ],
        None,
    ));
    let (registry, nas) = registry_over(agent).await;

    let rows = registry.inventory(nas.id).await;

    for row in &rows {
        assert_eq!(row.state, Availability::HostAbsent { since: T1 });
    }
}

#[tokio::test]
async fn a_reconnect_changes_the_skills_the_inventory_reports() {
    let fixture = Fixture::new(Vec::new()).await;
    fixture
        .link
        .set_skills(fixture.host_id(), vec![summary("prune_logs", Risk::Read)]);
    fixture.complete(&[("skill.prune_logs", 1)]);
    fixture.words(HostWords {
        runs_as_root: true,
        ..HostWords::default()
    });
    assert!(
        fixture
            .registry
            .inventory(fixture.host_id())
            .await
            .iter()
            .any(|row| row.contract == skill_id("prune_logs"))
    );

    fixture.link.new_session(fixture.host_id());
    fixture
        .link
        .set_skills(fixture.host_id(), vec![summary("zpool_status", Risk::Read)]);
    fixture.complete(&[("skill.zpool_status", 1)]);

    let rows = fixture.registry.inventory(fixture.host_id()).await;
    assert!(
        rows.iter()
            .all(|row| row.contract != skill_id("prune_logs")),
        "the new session's inventory kept the old session's skill"
    );
    assert!(
        rows.iter()
            .any(|row| row.contract == skill_id("zpool_status"))
    );
}

#[tokio::test]
async fn a_reconnect_changes_the_words_the_answer_uses() {
    let fixture = Fixture::new(Vec::new()).await;
    fixture
        .link
        .set_skills(fixture.host_id(), vec![summary("prune_logs", Risk::Read)]);
    fixture.complete(&[("skill.prune_logs", 1)]);
    fixture.words(HostWords {
        runs_as_root: true,
        ..HostWords::default()
    });
    assert_eq!(
        fixture.skill_state("prune_logs").await,
        Availability::Available
    );

    fixture.link.new_session(fixture.host_id());
    fixture.words(HostWords::default());

    assert_eq!(
        fixture.skill_state("prune_logs").await,
        missing(UnavailableReason::SkillRunnerMissing),
        "the answer used the previous session's root label"
    );
}

#[tokio::test]
async fn partial_catalogue_keeps_an_announced_skill_available() {
    let fixture = Fixture::new(Vec::new()).await;
    fixture
        .link
        .set_skills(fixture.host_id(), vec![summary("prune_logs", Risk::Read)]);
    fixture.words(HostWords {
        runs_as_root: true,
        ..HostWords::default()
    });
    fixture.unreadable();

    let state = fixture.skill_state("prune_logs").await;

    assert_eq!(state, Availability::Available);
    let row = fixture.row("skill.prune_logs").await;
    assert!(
        !row.catalogue.is_complete(),
        "the row hid the unverified catalogue"
    );
}

#[tokio::test]
async fn a_disabled_skill_is_disabled_even_when_the_catalogue_echoes_it() {
    let fixture = Fixture::with_settings(
        Vec::new(),
        vec![(
            host_skills_key("nas"),
            json!({ "disabled": ["prune_logs"] }),
        )],
    )
    .await;
    fixture
        .link
        .set_skills(fixture.host_id(), vec![summary("prune_logs", Risk::Read)]);
    fixture.complete(&[("skill.prune_logs", 1)]);

    assert_eq!(
        fixture.skill_state("prune_logs").await,
        Availability::DisabledByHub
    );
}

#[tokio::test]
async fn the_inventory_reports_the_floored_effect() {
    let fixture = Fixture::new(Vec::new()).await;
    fixture
        .link
        .set_skills(fixture.host_id(), vec![summary("prune_logs", Risk::Read)]);
    fixture.complete(&[("skill.prune_logs", 1)]);
    fixture.words(HostWords {
        runs_as_root: true,
        ..HostWords::default()
    });
    let skill = name("prune_logs");
    fixture
        .registry
        .set_floor(&fixture.host, skill.clone(), Some(EffectClass::Destructive))
        .await
        .unwrap_or_else(|err| panic!("{err}"));

    let row = fixture.row("skill.prune_logs").await;

    assert_eq!(row.effect, EffectClass::Destructive);
    assert_eq!(
        fixture.registry.effective_effect(fixture.host_id(), &skill),
        Some(EffectClass::Destructive),
        "the inventory and the effective-effect answer disagree"
    );
}

/// A host whose helper is `state` and offers `ops`, with a ceiling covering the unit contracts.
#[cfg(test)]
fn helper_words(state: HelperState, ops: &[&str]) -> HostWords {
    HostWords {
        helper: state,
        helper_ops: ops.iter().map(|op| (*op).to_owned()).collect(),
        ceiling_summary: Some(ceiling(&[("unit.restart", 2)])),
        container_runtime: true,
        ..HostWords::default()
    }
}

#[tokio::test]
async fn registry_requires_the_nft_ops_in_the_helper_status() {
    let fixture =
        Fixture::new(domovoi_core::capability::containment::containment_contracts()).await;
    let words = |ops: &[&str]| HostWords {
        helper: HelperState::Ready,
        helper_ops: ops.iter().map(|op| (*op).to_owned()).collect(),
        ceiling_summary: Some(ceiling(&[("firewall.block_source", 2)])),
        container_runtime: true,
        ..HostWords::default()
    };
    // A ready helper with no nft group: neither contract is installed here.
    fixture
        .link
        .set_host_words(fixture.host_id(), words(&["unit.restart"]));
    for id in ["firewall.block_source", "firewall.list_blocks"] {
        assert_eq!(
            fixture.availability(id).await,
            missing(UnavailableReason::HelperNftGroupMissing)
        );
    }
    // Part of the group is not enough: a block has three phases.
    fixture.link.set_host_words(
        fixture.host_id(),
        words(&["firewall.apply", "firewall.confirm"]),
    );
    assert_eq!(
        fixture.availability("firewall.block_source").await,
        missing(UnavailableReason::HelperNftGroupMissing)
    );
    // The whole group makes both available.
    fixture.link.set_host_words(
        fixture.host_id(),
        words(&domovoi_core::containment::ops::NFT_OPS),
    );
    assert_eq!(
        fixture.availability("firewall.list_blocks").await,
        Availability::Available
    );
    assert_eq!(
        fixture.availability("firewall.block_source").await,
        Availability::Available
    );
    // No helper at all is still said as it is today.
    fixture
        .link
        .set_host_words(fixture.host_id(), helper_words(HelperState::Absent, &[]));
    assert_eq!(
        fixture.availability("firewall.block_source").await,
        missing(UnavailableReason::HelperMissing)
    );
}

#[tokio::test]
async fn registry_reads_helper_ops() {
    let fixture = Fixture::new(domovoi_core::capability::unit_contracts()).await;
    // A ready helper that does not list the operation: not installed here.
    fixture.link.set_host_words(
        fixture.host_id(),
        helper_words(HelperState::Ready, &["systemd.read"]),
    );
    assert_eq!(
        fixture.availability("unit.restart").await,
        missing(UnavailableReason::HelperSystemdGroupMissing)
    );
    assert_eq!(
        fixture.availability("unit.identity").await,
        missing(UnavailableReason::HelperSystemdGroupMissing)
    );
    // The reason names the file and the setting, not the agent.
    let said = UnavailableReason::HelperSystemdGroupMissing.describe();
    assert!(said.contains("/etc/domovoi/helper.toml") && said.contains("groups"));
    assert!(!said.contains("agent does not implement"));
    // Once the helper lists them, they are available.
    fixture.link.set_host_words(
        fixture.host_id(),
        helper_words(HelperState::Ready, &["unit.identity", "unit.restart"]),
    );
    assert_eq!(
        fixture.availability("unit.restart").await,
        Availability::Available
    );
    assert_eq!(
        fixture.availability("unit.identity").await,
        Availability::Available
    );
    // No helper, or a failing one, is said as it is today.
    fixture
        .link
        .set_host_words(fixture.host_id(), helper_words(HelperState::Absent, &[]));
    assert_eq!(
        fixture.availability("unit.restart").await,
        missing(UnavailableReason::HelperMissing)
    );
    fixture
        .link
        .set_host_words(fixture.host_id(), helper_words(HelperState::Failed, &[]));
    assert_eq!(
        fixture.availability("unit.restart").await,
        missing(UnavailableReason::HelperError)
    );
    // No ceiling entry only means unattended restarts are off: a person can still approve it.
    let mut no_entry = helper_words(HelperState::Ready, &["unit.restart"]);
    no_entry.ceiling_summary = Some(ceiling(&[("unit.other", 1)]));
    fixture.link.set_host_words(fixture.host_id(), no_entry);
    assert_eq!(
        fixture.availability("unit.restart").await,
        Availability::Available
    );
}

#[tokio::test]
async fn container_restart_is_unavailable_for_both_labels() {
    let fixture = Fixture::new(domovoi_core::capability::unit_contracts()).await;
    // Whatever the helper reports and whatever the ceiling says, there is no Docker write path.
    let mut words = helper_words(HelperState::Ready, &["container.restart", "docker.write"]);
    words.ceiling_summary = Some(ceiling(&[("container.restart", 5)]));
    fixture.link.set_host_words(fixture.host_id(), words);
    assert_eq!(
        fixture.availability("container.restart").await,
        missing(UnavailableReason::DockerWriteUnsupported)
    );
    fixture
        .link
        .set_host_words(fixture.host_id(), helper_words(HelperState::Absent, &[]));
    assert_eq!(
        fixture.availability("container.restart").await,
        missing(UnavailableReason::DockerWriteUnsupported)
    );
}

fn container_words(helper: HelperState) -> HostWords {
    HostWords {
        in_container: true,
        helper,
        ..HostWords::default()
    }
}

#[tokio::test]
async fn container_agent_helper_contract_is_refused_with_agent_in_container() {
    let fixture = Fixture::new(domovoi_core::capability::unit_contracts()).await;
    fixture
        .link
        .set_host_words(fixture.host_id(), container_words(HelperState::Absent));
    assert_eq!(
        fixture.availability("unit.restart").await,
        missing(UnavailableReason::AgentInContainer)
    );
    assert_eq!(
        UnavailableReason::AgentInContainer.code(),
        "agent_in_container"
    );
    assert!(fixture.row("unit.restart").await.in_container);
}

#[tokio::test]
async fn non_container_agent_without_helper_still_gets_helper_missing() {
    let fixture = Fixture::new(domovoi_core::capability::unit_contracts()).await;
    fixture
        .link
        .set_host_words(fixture.host_id(), helper_words(HelperState::Absent, &[]));
    assert_eq!(
        fixture.availability("unit.restart").await,
        missing(UnavailableReason::HelperMissing)
    );
    assert!(!fixture.row("unit.restart").await.in_container);
}

#[tokio::test]
async fn container_agent_skill_is_refused_with_agent_in_container() {
    let fixture = Fixture::new(Vec::new()).await;
    fixture
        .link
        .set_skills(fixture.host_id(), vec![summary("prune_logs", Risk::Read)]);
    fixture.complete(&[("skill.prune_logs", 1)]);
    fixture.words(HostWords {
        in_container: true,
        ..HostWords::default()
    });
    assert_eq!(
        fixture.skill_state("prune_logs").await,
        missing(UnavailableReason::AgentInContainer)
    );
}

#[tokio::test]
async fn container_agent_read_contracts_are_unchanged() {
    let fixture = Fixture::new(Vec::new()).await;
    fixture.complete(&[("container.logs", 1)]);
    fixture.words(HostWords {
        in_container: true,
        container_runtime: true,
        ..HostWords::default()
    });
    assert_eq!(
        fixture.availability("container.logs").await,
        Availability::Available
    );
}

#[tokio::test]
async fn write_skill_is_available_without_a_ceiling() {
    // A skill runs on a person's approval; the unattended ceiling cannot name `skill.*`.
    let fixture = Fixture::new(Vec::new()).await;
    fixture.link.set_skills(
        fixture.host_id(),
        vec![summary("restart_unit", Risk::Write)],
    );
    fixture.complete(&[("skill.restart_unit", 1)]);
    fixture.words(HostWords {
        runs_as_root: true,
        ..HostWords::default()
    });

    assert_eq!(
        fixture.skill_state("restart_unit").await,
        Availability::Available
    );
}

#[tokio::test]
async fn write_skill_on_an_agent_that_is_not_root_is_unavailable_with_the_reason() {
    // The three-identities layout: the runner is ready but its account is not root, so a
    // write skill would fail at run time. It must not read as Available.
    let fixture = Fixture::new(Vec::new()).await;
    fixture.link.set_skills(
        fixture.host_id(),
        vec![
            summary("restart_unit", Risk::Write),
            summary("disk_report", Risk::Read),
        ],
    );
    fixture.complete(&[("skill.restart_unit", 1), ("skill.disk_report", 1)]);
    fixture.words(HostWords {
        runs_as_root: false,
        runner: domovoi_core::ports::RunnerState::Ready,
        ..HostWords::default()
    });

    assert_eq!(
        fixture.skill_state("restart_unit").await,
        missing(UnavailableReason::WriteSkillNeedsRoot)
    );
    assert_eq!(
        fixture.skill_state("disk_report").await,
        Availability::Available
    );
    assert!(
        UnavailableReason::WriteSkillNeedsRoot
            .describe()
            .contains("unit.restart")
    );
}
