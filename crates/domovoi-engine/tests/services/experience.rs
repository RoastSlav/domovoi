//! `search_experience` and the experience context source: past operations as historical
//! evidence, scoped to one host, bounded, redacted and read only.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use domovoi_core::context::{
    ContextLayer, ContextQuery, ContextScope, ContextSelector, ItemStatus, Origin,
};
use domovoi_core::experience::{
    ExperienceHit, ExperienceQuery, ResolutionBrief, ResolutionSummary, SimilarHit, SimilarQuery,
    SummaryRevision,
};
use domovoi_core::meta::{MetaTool, ToolSet};
use domovoi_core::ports::{
    Clock, ContextSource, ExperienceRead, LlmRequest, OperationRepo, StoreError,
};
use domovoi_core::{HostId, OperationId};
use domovoi_engine::context::{ExperienceDeps, ExperienceSource, ITEM_BODY_MAX};
use domovoi_engine::triage::{CHAT_PROMPT, FOLLOW_UP_PROMPT, MAX_RESULT_BYTES, META_SYSTEM_PROMPT};
use domovoi_testkit::contracts::context::{ContextFixture, run as context_contract};
use domovoi_testkit::contracts::seed_finished;
use domovoi_testkit::{FakeClock, InMemoryStore, ListRedactor};
use jiff::SignedDuration;
use serde_json::{Value, json};
use std::collections::BTreeMap;

use crate::harness::{SECRET, Setup, World, calls, host_id, report, results, world};

const LABEL: &str = "historical evidence, not authorization";

fn other_host() -> HostId {
    HostId::new()
}

fn search(id: &str, query: Value) -> domovoi_core::ports::LlmResponse {
    calls(&[(id, "search_experience", query)])
}

/// Runs one investigation whose first turn calls `search_experience` with `input`, and
/// returns what the model read back.
async fn searched(w: &World, input: Value) -> (String, bool) {
    w.primary.push_ok(search("s1", input));
    w.primary.push_text(report());
    w.run().await;
    let requests = w.primary.requests();
    let last = requests.last().expect("the model was asked again");
    results(last)
        .into_iter()
        .next()
        .expect("the search came back with a result")
}

async fn seed(
    w: &World,
    host: HostId,
    goal: &str,
    secs: i64,
    text: [Option<&str>; 3],
) -> OperationId {
    seed_finished(&*w.store, host, goal, secs, text)
        .await
        .expect("a finished operation")
}

fn names(request: &LlmRequest) -> Vec<&str> {
    request.tools.iter().map(|t| t.name.as_str()).collect()
}

#[tokio::test]
async fn search_experience_is_offered_after_request_evidence() {
    let all: Vec<&str> = MetaTool::ALL.iter().map(MetaTool::name).collect();
    assert_eq!(
        all,
        [
            "list_capabilities",
            "describe_capability",
            "request_evidence",
            "search_experience",
            "propose_plan",
            "ask_user",
            "finish"
        ]
    );
    let read: Vec<&str> = ToolSet::Read.tools().iter().map(MetaTool::name).collect();
    assert_eq!(&read[..], &all[..4]);
    assert_eq!(ToolSet::Investigate { questions: false }.tools().len(), 6);
    assert_eq!(ToolSet::Investigate { questions: true }.tools().len(), 7);

    let w = world(Setup::default()).await;
    w.primary.push_text(report());
    w.run().await;
    let request = &w.primary.requests()[0];
    assert!(names(request).contains(&"search_experience"));
    let spec = request
        .tools
        .iter()
        .find(|t| t.name == "search_experience")
        .unwrap();
    let properties = spec.input_schema["properties"].as_object().unwrap();
    assert_eq!(properties.keys().collect::<Vec<_>>(), ["query"]);
    assert_eq!(spec.input_schema["additionalProperties"], json!(false));
}

#[tokio::test]
async fn experience_search_is_host_scoped() {
    let w = world(Setup::default()).await;
    let here = seed(
        &w,
        host_id(),
        "plex disk full",
        100,
        [Some("disk full"), None, None],
    )
    .await;
    let there = seed(
        &w,
        other_host(),
        "plex disk full",
        200,
        [Some("disk full"), None, None],
    )
    .await;

    let (text, is_error) = searched(&w, json!({"query": "disk full"})).await;

    assert!(!is_error);
    assert!(text.contains(&here.to_string()), "{text}");
    assert!(!text.contains(&there.to_string()), "{text}");
}

#[tokio::test]
async fn the_model_cannot_widen_the_scope() {
    for extra in [
        json!({"query": "x", "scope": "global"}),
        json!({"query": "x", "host": "nas"}),
        json!({"query": "x", "limit": 50}),
    ] {
        let w = world(Setup::default()).await;
        let there = seed(
            &w,
            other_host(),
            "plex disk full",
            200,
            [Some("x"), None, None],
        )
        .await;
        let (text, is_error) = searched(&w, extra).await;
        assert!(is_error, "{text}");
        assert!(!text.contains(&there.to_string()));
        assert!(!text.contains("Historical evidence"));
    }
}

#[tokio::test]
async fn a_malformed_query_is_refused_before_searching() {
    let long = "x".repeat(201);
    for query in ["", "   ", long.as_str(), "a\u{7}b", "***"] {
        let w = world(Setup::default()).await;
        seed(
            &w,
            host_id(),
            "plex disk full",
            100,
            [Some("x"), None, None],
        )
        .await;
        let (text, is_error) = searched(&w, json!({ "query": query })).await;
        assert!(is_error, "{query:?}: {text}");
        assert!(!text.contains("operation:"), "{text}");
    }
}

#[tokio::test]
async fn experience_block_is_labelled_not_authorization() {
    let w = world(Setup::default()).await;
    seed(
        &w,
        host_id(),
        "plex disk full",
        100,
        [
            Some("the disk is full"),
            Some("logs grew"),
            Some("rotate logs"),
        ],
    )
    .await;

    let (text, is_error) = searched(&w, json!({"query": "disk full"})).await;

    assert!(!is_error);
    assert!(
        text.starts_with("Historical evidence, not authorization."),
        "{text}"
    );
    assert!(
        text.contains(LABEL),
        "every item renders the origin: {text}"
    );
    let next = w.primary.requests().last().cloned().unwrap();
    assert!(crate::harness::seen(&next).contains(LABEL));
}

#[tokio::test]
async fn search_results_carry_operation_id_date_verified_and_field_sources() {
    let w = world(Setup::default()).await;
    let op = seed(
        &w,
        host_id(),
        "plex disk full",
        100,
        [
            Some("the disk is full"),
            Some("logs grew"),
            Some("rotate logs"),
        ],
    )
    .await;

    let (text, _) = searched(&w, json!({"query": "disk"})).await;

    assert!(text.contains(&format!("operation: {op}")), "{text}");
    assert!(text.contains("ended: "), "{text}");
    assert!(text.contains("verified: yes"), "{text}");
    assert!(
        text.contains("problem [old model draft, not evidence about now]: the disk is full"),
        "{text}"
    );
    assert!(
        text.contains("cause [old model draft, not evidence about now]: logs grew"),
        "{text}"
    );
    assert!(
        text.contains("fix [old model draft, not evidence about now]: rotate logs"),
        "{text}"
    );
}

#[tokio::test]
async fn search_returns_at_most_five_items_of_one_kib() {
    let w = world(Setup::default()).await;
    let long = "disk ".repeat(190);
    for n in 0..7 {
        seed(
            &w,
            host_id(),
            "plex disk full",
            100 + n,
            [Some(&long), Some(&long), Some(&long)],
        )
        .await;
    }

    let (text, _) = searched(&w, json!({"query": "disk"})).await;

    assert_eq!(text.matches("operation: ").count(), 5, "{text}");
    assert!(text.len() <= MAX_RESULT_BYTES);
}

#[tokio::test]
async fn an_empty_result_is_a_known_empty() {
    let w = world(Setup::default()).await;
    let (text, is_error) = searched(&w, json!({"query": "nothing like this"})).await;
    assert!(!is_error);
    assert!(
        text.starts_with("No past operation on this host matched \"nothing like this\""),
        "{text}"
    );
}

/// A store whose summary reads can be made to fail.
struct Flaky {
    inner: Arc<InMemoryStore>,
    broken: AtomicBool,
}

impl Flaky {
    fn check(&self) -> Result<(), StoreError> {
        if self.broken.load(Ordering::SeqCst) {
            Err(StoreError::Backend("the store failed".to_owned()))
        } else {
            Ok(())
        }
    }
}

#[async_trait]
impl ExperienceRead for Flaky {
    async fn summary(&self, op: OperationId) -> Result<Option<ResolutionSummary>, StoreError> {
        self.check()?;
        self.inner.summary(op).await
    }
    async fn revisions(&self, op: OperationId) -> Result<Vec<SummaryRevision>, StoreError> {
        self.check()?;
        self.inner.revisions(op).await
    }
    async fn current(&self, op: OperationId) -> Result<Vec<SummaryRevision>, StoreError> {
        self.check()?;
        self.inner.current(op).await
    }
    async fn briefs(
        &self,
        ops: &[OperationId],
    ) -> Result<BTreeMap<OperationId, ResolutionBrief>, StoreError> {
        self.check()?;
        self.inner.briefs(ops).await
    }
    async fn search(&self, query: &ExperienceQuery) -> Result<Vec<ExperienceHit>, StoreError> {
        self.check()?;
        self.inner.search(query).await
    }
    async fn similar(&self, query: &SimilarQuery) -> Result<Vec<SimilarHit>, StoreError> {
        self.check()?;
        self.inner.similar(query).await
    }
    async fn undrafted(&self, limit: u16) -> Result<Vec<OperationId>, StoreError> {
        self.check()?;
        self.inner.undrafted(limit).await
    }
}

fn source_over(
    store: &Arc<InMemoryStore>,
    experience: Arc<dyn ExperienceRead>,
    clock: Arc<FakeClock>,
) -> ExperienceSource {
    ExperienceSource::new(ExperienceDeps {
        experience,
        incidents: store.clone(),
        operations: store.clone(),
        hosts: store.clone(),
        redactor: Arc::new(ListRedactor::new([SECRET])),
        clock,
    })
}

fn query(scope: ContextScope, selector: ContextSelector, clock: &FakeClock) -> ContextQuery {
    ContextQuery {
        scope,
        selector,
        limit: 5,
        max_item_bytes: 4096,
        deadline: clock.now() + SignedDuration::from_secs(30),
    }
}

#[tokio::test]
async fn a_failed_read_is_unknown_not_empty() {
    let store = Arc::new(InMemoryStore::new());
    let clock = Arc::new(FakeClock::new(domovoi_testkit::build::T0));
    let flaky = Arc::new(Flaky {
        inner: store.clone(),
        broken: AtomicBool::new(true),
    });
    let source = source_over(&store, flaky, clock.clone());

    let batch = source
        .fetch(&query(
            ContextScope::Host(host_id()),
            ContextSelector::Search {
                text: "disk".to_owned(),
            },
            &clock,
        ))
        .await;

    assert!(batch.items.is_empty());
    assert_eq!(batch.gaps.len(), 1);
    assert_eq!(batch.gaps[0].reason, "could not read past operations");
}

struct Fixture {
    store: Arc<InMemoryStore>,
    flaky: Arc<Flaky>,
    clock: Arc<FakeClock>,
    source: ExperienceSource,
}

impl Fixture {
    fn new() -> Self {
        let store = Arc::new(InMemoryStore::new());
        let clock = Arc::new(FakeClock::new(domovoi_testkit::build::T0));
        let flaky = Arc::new(Flaky {
            inner: store.clone(),
            broken: AtomicBool::new(false),
        });
        let source = source_over(&store, flaky.clone(), clock.clone());
        Self {
            store,
            flaky,
            clock,
            source,
        }
    }
}

impl ContextFixture for Fixture {
    fn source(&self) -> &dyn ContextSource {
        &self.source
    }
    fn selector(&self) -> ContextSelector {
        ContextSelector::Search {
            text: "disk".to_owned(),
        }
    }
    fn clock(&self) -> &FakeClock {
        &self.clock
    }
    fn secret(&self) -> &str {
        SECRET
    }
    fn answer_with(&self, text: &str) {
        self.flaky.broken.store(false, Ordering::SeqCst);
        let store = self.store.clone();
        let text = text.to_owned();
        let host = host_id();
        // Setup is synchronous in the trait: run it on the current runtime.
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async move {
                let stored: String = format!("disk {text}").chars().take(900).collect();
                let _ = seed_finished(
                    &*store,
                    host,
                    "disk",
                    10,
                    [Some(stored.as_str()), Some(stored.as_str()), None],
                )
                .await;
            });
        });
    }
    fn fail_next(&self) {
        self.flaky.broken.store(true, Ordering::SeqCst);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_experience_source_satisfies_the_context_contract() {
    let fixture = Fixture::new();
    context_contract(&fixture, ContextScope::Host(host_id())).await;
}

#[tokio::test]
async fn experience_items_are_always_historical_and_small() {
    let w = world(Setup::default()).await;
    let long = "disk ".repeat(190);
    for n in 0..7 {
        seed(
            &w,
            host_id(),
            "plex disk full",
            100 + n,
            [Some(&long), Some(&long), Some(&long)],
        )
        .await;
    }
    let source = ExperienceSource::over(
        w.store.clone(),
        Arc::new(ListRedactor::new([SECRET])),
        w.clock.clone(),
    );

    let overview = source
        .fetch(&query(
            ContextScope::Host(host_id()),
            ContextSelector::Overview,
            &w.clock,
        ))
        .await;
    assert!(
        overview.items.len() <= 3,
        "an overview holds at most three items"
    );

    let found = source
        .fetch(&query(
            ContextScope::Host(host_id()),
            ContextSelector::Search {
                text: "disk".to_owned(),
            },
            &w.clock,
        ))
        .await;
    assert_eq!(found.items.len(), 5);
    for item in overview.items.iter().chain(&found.items) {
        assert_eq!(item.provenance.origin, Origin::Historical);
        assert_eq!(item.status, ItemStatus::Known);
        assert_eq!(item.layer, ContextLayer::Experience);
        assert!(item.provenance.reference.starts_with("operation:"));
        assert!(item.body.len() <= ITEM_BODY_MAX);
        let value = serde_json::to_value(item).unwrap();
        let mut keys: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "body",
                "id",
                "layer",
                "provenance",
                "source",
                "status",
                "title",
                "truncated"
            ]
        );
    }
}

#[tokio::test]
async fn the_incident_overview_returns_similar_operations_and_a_mismatch_is_a_gap() {
    let w = world(Setup::default()).await;
    let title = w.incident.title().to_owned();
    let op = seed(
        &w,
        host_id(),
        &title,
        100,
        [Some("it was down"), None, None],
    )
    .await;
    let source = ExperienceSource::over(
        w.store.clone(),
        Arc::new(ListRedactor::new([SECRET])),
        w.clock.clone(),
    );
    let scope = ContextScope::Incident {
        host: host_id(),
        incident: w.incident.id(),
    };

    let batch = source
        .fetch(&query(scope, ContextSelector::Overview, &w.clock))
        .await;
    assert!(batch.gaps.is_empty(), "{:?}", batch.gaps);
    assert!(
        batch
            .items
            .iter()
            .any(|i| i.provenance.reference == format!("operation:{op}"))
    );

    for scope in [
        ContextScope::Incident {
            host: other_host(),
            incident: w.incident.id(),
        },
        ContextScope::Incident {
            host: host_id(),
            incident: domovoi_core::IncidentId::new(),
        },
        ContextScope::Operation {
            host: host_id(),
            operation: OperationId::new(),
        },
    ] {
        let batch = source
            .fetch(&query(scope, ContextSelector::Overview, &w.clock))
            .await;
        assert!(batch.items.is_empty());
        assert_eq!(batch.gaps.len(), 1);
    }
    let late = ContextQuery {
        deadline: w.clock.now(),
        ..query(
            ContextScope::Host(host_id()),
            ContextSelector::Overview,
            &w.clock,
        )
    };
    assert_eq!(source.fetch(&late).await.gaps[0].reason, "timed out");
}

#[tokio::test]
async fn global_search_names_hosts_and_a_host_scoped_fetch_never_does() {
    let w = world(Setup::default()).await;
    seed(
        &w,
        host_id(),
        "plex disk full",
        100,
        [Some("disk full"), None, None],
    )
    .await;
    seed(
        &w,
        other_host(),
        "plex disk full",
        200,
        [Some("disk full"), None, None],
    )
    .await;
    let source = ExperienceSource::over(
        w.store.clone(),
        Arc::new(ListRedactor::new([SECRET])),
        w.clock.clone(),
    );

    let global = source
        .global_search(
            "disk",
            5,
            1024,
            w.clock.now() + SignedDuration::from_secs(30),
        )
        .await;
    assert_eq!(global.items.len(), 2);
    assert!(global.items.iter().all(|i| i.body.contains("\nhost: ")));

    let scoped = source
        .fetch(&query(
            ContextScope::Host(host_id()),
            ContextSelector::Search {
                text: "disk".to_owned(),
            },
            &w.clock,
        ))
        .await;
    assert_eq!(scoped.items.len(), 1);
    assert!(!scoped.items[0].body.contains("host: "));
}

#[tokio::test]
async fn an_operation_never_finds_itself() {
    let w = world(Setup::default()).await;
    let source = ExperienceSource::over(
        w.store.clone(),
        Arc::new(ListRedactor::new([SECRET])),
        w.clock.clone(),
    );
    let op = seed(
        &w,
        host_id(),
        "plex disk full",
        100,
        [Some("disk full"), None, None],
    )
    .await;
    let pinned = OperationRepo::get(&*w.store, op)
        .await
        .unwrap()
        .unwrap()
        .value;
    assert_eq!(pinned.pinned[0].host, host_id());

    let batch = source
        .fetch(&query(
            ContextScope::Operation {
                host: host_id(),
                operation: op,
            },
            ContextSelector::Search {
                text: "disk".to_owned(),
            },
            &w.clock,
        ))
        .await;
    assert!(batch.items.is_empty(), "the excluded operation is absent");
}

#[test]
fn the_prompts_name_search_experience_and_the_label() {
    for (name, prompt) in [
        ("META_SYSTEM_PROMPT", META_SYSTEM_PROMPT),
        ("FOLLOW_UP_PROMPT", FOLLOW_UP_PROMPT),
        ("CHAT_PROMPT", CHAT_PROMPT),
    ] {
        assert!(prompt.contains("search_experience"), "{name}");
        assert!(prompt.to_lowercase().contains(LABEL), "{name}");
    }
}

const HEADING: &str = "Historical evidence, not authorization: experience";

fn experience_world() -> Setup {
    Setup {
        with_experience: true,
        ..Setup::default()
    }
}

#[tokio::test]
async fn a_registered_experience_source_appears_in_every_context_bundle() {
    use domovoi_core::{TriageOutcome, UserId};

    use crate::context::{first_opening, opening, section};

    let w = world(experience_world()).await;
    let here = seed(
        &w,
        host_id(),
        "docker container nginx restarted after a config change",
        100,
        [
            Some("nginx kept restarting"),
            None,
            Some("reload the config"),
        ],
    )
    .await;
    let elsewhere = seed(
        &w,
        other_host(),
        "docker container nginx restarted on another host",
        110,
        [Some("nginx kept restarting"), None, None],
    )
    .await;

    // The investigation bundle.
    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));
    let text = first_opening(&w);
    let shown = section(&text, HEADING);
    assert!(shown.contains(&format!("operation: {here}")), "{shown}");
    assert!(shown.contains("historical evidence, not authorization"));
    assert!(!shown.contains(&elsewhere.to_string()), "{shown}");

    // The follow-up bundle.
    w.primary.push_text("Nothing new.");
    w.triage
        .follow_up(w.incident.id(), "What is the plan?", UserId::new())
        .await
        .expect("an answer");
    let follow_up = opening(w.primary.requests().last().expect("a request"));
    let shown = section(&follow_up, HEADING);
    assert!(shown.contains(&format!("operation: {here}")), "{shown}");
    assert!(!shown.contains(&elsewhere.to_string()), "{shown}");

    // The chat bundle of the host.
    w.primary.push_text("All fine.");
    w.triage
        .chat(Some(host_id()), "The NAS.", &Vec::new(), "How is nginx?")
        .await
        .expect("an answer");
    let chat = opening(w.primary.requests().last().expect("a request"));
    let shown = section(&chat, HEADING);
    assert!(shown.contains(&format!("operation: {here}")), "{shown}");
    assert!(!shown.contains(&elsewhere.to_string()), "{shown}");
}

#[tokio::test]
async fn a_registered_experience_source_with_nothing_to_offer_reads_none() {
    use domovoi_core::TriageOutcome;

    use crate::context::{first_opening, section};

    let w = world(experience_world()).await;
    seed(
        &w,
        other_host(),
        "docker container nginx elsewhere",
        100,
        [None; 3],
    )
    .await;
    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));
    let shown = section(&first_opening(&w), HEADING);
    assert!(shown.contains("None."), "{shown}");
}

#[tokio::test]
async fn the_experience_section_stays_small() {
    use domovoi_core::TriageOutcome;
    use domovoi_engine::context::LayerCaps;

    use crate::context::{first_opening, section};

    assert_eq!(
        LayerCaps::DEFAULT.experience,
        4 * 1024,
        "the cap is unchanged"
    );
    let w = world(experience_world()).await;
    let long = "nginx disk ".repeat(60);
    for n in 0..7 {
        seed(
            &w,
            host_id(),
            "docker container nginx restart loop",
            100 + n,
            [Some(&long), Some(&long), Some(&long)],
        )
        .await;
    }
    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));
    let shown = section(&first_opening(&w), HEADING);
    assert!(
        shown.len() <= LayerCaps::DEFAULT.experience + 512,
        "the section is {} bytes: {shown}",
        shown.len()
    );
    let items = shown.matches("operation: ").count();
    assert!((1..=5).contains(&items), "{items} items: {shown}");
}
