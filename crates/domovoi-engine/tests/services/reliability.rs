//! Model reliability: persisted admission, per-call attribution, prompt caps, redaction and
//! host-scoped similar incidents.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use domovoi_core::ports::{
    AdmissionOutcome, AdmissionPurpose, AdmissionRepo, IncidentRepo, LlmError, LlmProvider,
    LlmRequest, Part, Usage, UsageEntry, UsageRepo, UsageSource, UsageTotal,
};
use domovoi_core::ports::{IncidentTimelineEntry, StoreError};
use domovoi_core::{
    CheckKey, Incident, IncidentEvent, IncidentPolicy, Severity, Signal, SignalClass, SourceKind,
    TargetKey, TriageOutcome, UserId, apply,
};
use domovoi_engine::triage::{TriageError, TriageSettings};
use domovoi_testkit::build::{T0, at, open_incident, resolved_incident};
use domovoi_testkit::{InMemoryStore, ScriptedLlm, UsageRow};
use jiff::SignedDuration;
use jiff::civil::Date;
use serde_json::json;
use tokio::task::yield_now;

use crate::harness::{
    SECRET, Setup, World, add_signal, disk, host_id, read_calls, report, results, seen, world,
    world_over,
};
use domovoi_core::ports::ToolOutput;

/// A usage repository whose month read fails while `broken` is set.
struct FlakyUsage {
    inner: Arc<InMemoryStore>,
    broken: AtomicBool,
}

impl FlakyUsage {
    fn new(inner: Arc<InMemoryStore>) -> Self {
        Self {
            inner,
            broken: AtomicBool::new(true),
        }
    }

    fn heal(&self) {
        self.broken.store(false, Ordering::SeqCst);
    }
}

#[async_trait]
impl UsageRepo for FlakyUsage {
    async fn record_call(&self, entry: &UsageEntry<'_>) -> Result<(), StoreError> {
        UsageRepo::record_call(&*self.inner, entry).await
    }

    async fn month_total(&self, month: Date) -> Result<UsageTotal, StoreError> {
        if self.broken.load(Ordering::SeqCst) {
            return Err(StoreError::Backend("disk full".to_owned()));
        }
        UsageRepo::month_total(&*self.inner, month).await
    }
}

/// An admission repository whose `admit` and `starts_since` may fail independently.
struct FlakyAdmission {
    inner: Arc<InMemoryStore>,
    fail_admit: AtomicBool,
    fail_reads: AtomicBool,
    reads: AtomicUsize,
}

impl FlakyAdmission {
    fn new(inner: Arc<InMemoryStore>) -> Self {
        Self {
            inner,
            fail_admit: AtomicBool::new(false),
            fail_reads: AtomicBool::new(false),
            reads: AtomicUsize::new(0),
        }
    }

    fn store(&self) -> Arc<InMemoryStore> {
        self.inner.clone()
    }

    fn fail_admit(&self) {
        self.fail_admit.store(true, Ordering::SeqCst);
    }

    fn allow_admit(&self) {
        self.fail_admit.store(false, Ordering::SeqCst);
    }

    fn fail_reads(&self) {
        self.fail_reads.store(true, Ordering::SeqCst);
    }

    fn allow_reads(&self) {
        self.fail_reads.store(false, Ordering::SeqCst);
    }

    fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl AdmissionRepo for FlakyAdmission {
    async fn admit(
        &self,
        purpose: AdmissionPurpose,
        room: u32,
        window: SignedDuration,
        now: jiff::Timestamp,
    ) -> Result<AdmissionOutcome, StoreError> {
        if self.fail_admit.load(Ordering::SeqCst) {
            return Err(StoreError::Backend(
                "the admission transaction failed".to_owned(),
            ));
        }
        AdmissionRepo::admit(&*self.inner, purpose, room, window, now).await
    }

    async fn starts_since(
        &self,
        since: jiff::Timestamp,
    ) -> Result<Vec<jiff::Timestamp>, StoreError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        if self.fail_reads.load(Ordering::SeqCst) {
            return Err(StoreError::Backend(
                "the admission window is unreadable".to_owned(),
            ));
        }
        AdmissionRepo::starts_since(&*self.inner, since).await
    }
}

/// A world whose triage service uses `admission` instead of the store's own repository.
async fn flaky_world(admission: Arc<FlakyAdmission>) -> World {
    let store = admission.store();
    world_over(
        Setup {
            admission: Some(admission),
            ..Setup::default()
        },
        store,
    )
    .await
}

/// Yields until the provider has received `wanted` requests, or the loop budget runs out.
async fn until_requests(w: &World, wanted: usize) {
    for _ in 0..500 {
        if w.primary.requests().len() >= wanted {
            return;
        }
        yield_now().await;
    }
}

async fn settle() {
    for _ in 0..500 {
        yield_now().await;
    }
}

/// A second incident on the same host with its own target.
async fn another(w: &World, target: &str) -> Incident {
    let incident = open_incident(host_id(), "docker.container", Some(target)).unwrap();
    IncidentRepo::insert(&*w.store, &incident).await.unwrap();
    add_signal(&w.store, &incident, disk(40.0), w.now()).await;
    incident
}

/// The starts recorded in the admission window, from far enough back that nothing pruned
/// them.
async fn starts(w: &World) -> Vec<jiff::Timestamp> {
    AdmissionRepo::starts_since(&*w.store, at(-86_400))
        .await
        .unwrap()
}

/// Counts the incident's timeline entries of `kind`.
async fn count(w: &World, id: domovoi_core::IncidentId, kind: &str) -> usize {
    IncidentRepo::timeline(&*w.store, id)
        .await
        .unwrap()
        .iter()
        .filter(|entry| entry.kind == kind)
        .count()
}

/// The bytes a request occupies, computed the way the engine caps a prompt.
fn size(req: &LlmRequest) -> usize {
    let mut total = req.system.len();
    for message in &req.messages {
        for part in &message.parts {
            total += match part {
                Part::Text(text) => text.len(),
                Part::ToolCall { name, input, .. } => name.len() + input.to_string().len(),
                Part::ToolResult { content, .. } => content.len(),
            };
        }
    }
    total
}

fn row(w: &World, index: usize) -> UsageRow {
    w.store.usage_rows().remove(index)
}

#[tokio::test]
async fn admission_survives_restart() {
    let settings = TriageSettings {
        per_hour_runs: 1,
        ..TriageSettings::default()
    };
    let w = world(Setup {
        settings,
        ..Setup::default()
    })
    .await;
    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));
    assert_eq!(w.primary.requests().len(), 1);

    let other = another(&w, "redis").await;
    // The restarted service's start-up pass investigates the open incident nobody triaged.
    let _restarted = w.restart_triage();
    w.drive(2, 1).await;
    let kinds = crate::parity::kinds(&w, &other).await;
    assert!(
        kinds.iter().any(|kind| kind == "triage.skipped"),
        "the restarted service still counts the run from before it: {kinds:?}"
    );
    assert_eq!(w.primary.requests().len(), 1, "no model call was made");

    // The skipped investigation retries through its own effect once the slot frees.
    w.primary.push_text(report());
    w.drive(70, 61).await;
    assert_eq!(
        w.primary.requests().len(),
        2,
        "the slot freed after an hour"
    );
    let kinds = crate::parity::kinds(&w, &other).await;
    assert!(
        kinds.iter().any(|kind| kind == "triage.report"),
        "{kinds:?}"
    );
}

#[tokio::test]
async fn cache_hit_uses_no_slot() {
    let settings = TriageSettings {
        per_hour_runs: 1,
        ..TriageSettings::default()
    };
    let w = world(Setup {
        settings,
        ..Setup::default()
    })
    .await;
    w.primary.push_text(report());
    let TriageOutcome::Report(first) = w.run().await else {
        unreachable!("the first run calls the model");
    };

    let earlier = resolved_incident(host_id(), "docker.container", Some("nginx"), w.now()).unwrap();
    IncidentRepo::insert(&*w.store, &earlier).await.unwrap();
    w.signal(disk(92.1)).await;
    assert_eq!(
        w.run().await,
        TriageOutcome::Cached {
            report: first,
            from: earlier.id()
        }
    );
    assert_eq!(w.primary.requests().len(), 1, "the cache answered");
    assert_eq!(
        starts(&w).await.len(),
        1,
        "the cached run added no start to the window"
    );

    // Different evidence is a different situation, so the cache cannot answer it. The one
    // start from the first run has expired, and no slot was spent on the cache hit.
    w.clock.advance(SignedDuration::from_mins(61));
    w.signal(disk(55.0)).await;
    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));
    assert_eq!(w.primary.requests().len(), 2);
}

#[tokio::test]
async fn budget_read_error_fails_closed_for_questions() {
    let settings = TriageSettings {
        monthly_budget_micros: Some(1_000_000),
        ..TriageSettings::default()
    };
    let store = Arc::new(InMemoryStore::new());
    let usage = Arc::new(FlakyUsage::new(store.clone()));
    let w = world_over(
        Setup {
            settings,
            usage: Some(usage),
            ..Setup::default()
        },
        store,
    )
    .await;

    let refused = w
        .triage
        .follow_up(w.incident.id(), "Why?", UserId::new())
        .await;
    assert_eq!(refused, Err(TriageError::AdmissionUnavailable));
    assert_eq!(w.primary.requests().len(), 0, "no model call was made");
    assert!(starts(&w).await.is_empty(), "no slot was recorded");
}

#[tokio::test]
async fn unverifiable_admission_skips_triage_and_leaves_the_card() {
    let store = Arc::new(InMemoryStore::new());
    let usage = Arc::new(FlakyUsage::new(store.clone()));
    let mut w = world_over(
        Setup {
            settings: TriageSettings {
                monthly_budget_micros: Some(1_000_000),
                ..TriageSettings::default()
            },
            usage: Some(usage.clone()),
            ..Setup::default()
        },
        store,
    )
    .await;
    w.drain();
    let outcome = w.run().await;
    assert!(
        matches!(&outcome, TriageOutcome::Skipped { reason } if reason.contains("could not be checked")),
        "an unreadable limit skips the run: {outcome:?}"
    );
    assert_eq!(w.primary.requests().len(), 0, "no model call was made");
    assert!(
        w.drain().iter().any(|event| matches!(
            event.kind,
            domovoi_engine::EventKind::TriageCompleted { .. }
        )),
        "the card still receives a triage outcome"
    );

    // The service scheduled a retry in five minutes; it runs once the store is healthy.
    usage.heal();
    w.primary.push_text(report());
    w.clock.advance(SignedDuration::from_mins(5));
    settle().await;
    assert_eq!(w.primary.requests().len(), 1, "the deferred retry ran");
    assert_eq!(count(&w, w.incident.id(), "triage.report").await, 1);
}

#[tokio::test]
async fn unpriced_admission_still_refuses_before_the_request() {
    let provider: Arc<dyn LlmProvider> = Arc::new(ScriptedLlm::new().without_price());
    let w = world(Setup {
        settings: TriageSettings {
            monthly_budget_micros: Some(1_000_000),
            ..TriageSettings::default()
        },
        provider: Some(provider),
        ..Setup::default()
    })
    .await;
    let outcome = w.run().await;
    assert!(
        matches!(&outcome, TriageOutcome::Skipped { reason } if reason.contains("has no price")),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn admit_error_skips_triage_and_defers_the_retry() {
    let admission = Arc::new(FlakyAdmission::new(Arc::new(InMemoryStore::new())));
    admission.fail_admit();
    let mut w = flaky_world(admission.clone()).await;
    w.drain();

    let outcome = w.run().await;
    assert!(
        matches!(&outcome, TriageOutcome::Skipped { reason } if reason.contains("could not be checked")),
        "an unreadable admission window skips the run: {outcome:?}"
    );
    assert_eq!(w.primary.requests().len(), 0, "no model call was made");
    assert!(
        starts(&w).await.is_empty(),
        "the failed admission inserted no start"
    );
    assert!(
        w.drain().iter().any(|event| matches!(
            event.kind,
            domovoi_engine::EventKind::TriageCompleted { .. }
        )),
        "the card still receives a triage outcome"
    );

    // The service scheduled a retry in five minutes; it runs once admission works.
    admission.allow_admit();
    w.primary.push_text(report());
    w.clock.advance(SignedDuration::from_mins(5));
    until_requests(&w, 1).await;
    assert_eq!(w.primary.requests().len(), 1, "the five-minute retry ran");
    assert_eq!(starts(&w).await.len(), 1, "the retry recorded one start");
}

#[tokio::test]
async fn admit_error_answers_questions_as_unavailable() {
    let admission = Arc::new(FlakyAdmission::new(Arc::new(InMemoryStore::new())));
    admission.fail_admit();
    let w = flaky_world(admission).await;

    let follow_up = w
        .triage
        .follow_up(w.incident.id(), "Why?", UserId::new())
        .await;
    assert_eq!(follow_up, Err(TriageError::AdmissionUnavailable));
    let chat = w
        .triage
        .chat(Some(host_id()), "the host is healthy", &Vec::new(), "Why?")
        .await;
    assert_eq!(chat, Err(TriageError::AdmissionUnavailable));
    assert_eq!(w.primary.requests().len(), 0, "no model call was made");
    assert!(
        starts(&w).await.is_empty(),
        "neither question recorded a start"
    );
}

#[tokio::test]
async fn starts_since_error_defers_the_due_retry_by_five_minutes() {
    let admission = Arc::new(FlakyAdmission::new(Arc::new(InMemoryStore::new())));
    admission.fail_admit();
    let w = flaky_world(admission.clone()).await;

    let skipped = w.run().await;
    assert!(
        matches!(&skipped, TriageOutcome::Skipped { reason } if reason.contains("could not be checked")),
        "{skipped:?}"
    );
    assert_eq!(w.primary.requests().len(), 0);

    // Let the first deferral settle, then make the persisted window unreadable. From here
    // the retry worker's `free_slots` read is the only admission read left.
    admission.allow_admit();
    settle().await;
    admission.fail_reads();
    let baseline = admission.reads();

    // The retry is due now. Its unreadable window must not be read as free capacity.
    w.clock.advance(SignedDuration::from_mins(5));
    settle().await;
    assert!(
        admission.reads() > baseline,
        "the retry worker read the failing window"
    );
    assert_eq!(
        w.primary.requests().len(),
        0,
        "an unreadable window is not free capacity"
    );

    // The failed pass pushed the retry to five minutes from now, not to now.
    w.clock.advance(SignedDuration::from_mins(4));
    settle().await;
    assert_eq!(w.primary.requests().len(), 0);

    // Making the window readable frees the retry at its new five-minute time.
    admission.allow_reads();
    w.primary.push_text(report());
    w.clock.advance(SignedDuration::from_mins(1));
    until_requests(&w, 1).await;
    assert_eq!(
        w.primary.requests().len(),
        1,
        "the retry reached the provider once the window was readable"
    );
    assert_eq!(count(&w, w.incident.id(), "triage.report").await, 1);
}
#[tokio::test]
async fn failed_call_records_usage() {
    let primary = Arc::new(ScriptedLlm::new().with_model("test-model"));
    primary.push_err(LlmError::Timeout {
        after: SignedDuration::from_secs(300),
    });
    let primary_dyn: Arc<dyn LlmProvider> = primary.clone();
    let w = world(Setup {
        secondary: true,
        provider: Some(primary_dyn),
        ..Setup::default()
    })
    .await;
    w.secondary.push_text(report());

    assert!(matches!(w.run().await, TriageOutcome::Report(_)));
    let rows = w.store.usage_rows();
    assert_eq!(rows.len(), 2, "one row per provider call");
    let failed = &rows[0];
    assert_eq!(failed.provider, "scripted");
    assert_eq!(failed.model.as_deref(), Some("test-model"));
    assert_eq!(failed.outcome, domovoi_core::ports::CallOutcome::Timeout);
    assert_eq!((failed.tokens_in, failed.tokens_out), (0, 0));
    assert_eq!(failed.cost_micros, None);
    assert_eq!(failed.usage_source, None);
    assert_eq!(failed.latency_ms, 0, "the fake clock does not move");

    let answered = &rows[1];
    assert_eq!(answered.outcome, domovoi_core::ports::CallOutcome::Ok);
    assert_eq!(answered.usage_source, Some(UsageSource::Reported));
    assert_eq!(answered.cost_micros, Some(0));
}

#[tokio::test]
async fn unknown_cost_is_recorded_as_unknown_not_free() {
    let provider = Arc::new(ScriptedLlm::new().without_price());
    provider.push_ok(domovoi_core::ports::LlmResponse {
        parts: vec![Part::Text(report())],
        stop: domovoi_core::ports::StopReason::EndTurn,
        usage: Usage {
            input_tokens: 10,
            output_tokens: 5,
            cost_micros: None,
            source: UsageSource::Unknown,
        },
    });
    let provider_dyn: Arc<dyn LlmProvider> = provider;
    let w = world(Setup {
        provider: Some(provider_dyn),
        ..Setup::default()
    })
    .await;
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));
    let unknown = row(&w, 0);
    assert_eq!(unknown.cost_micros, None, "unknown is stored as NULL");
    assert_eq!(unknown.usage_source, Some(UsageSource::Unknown));
    assert_eq!((unknown.tokens_in, unknown.tokens_out), (10, 5));

    let month = w.now().to_zoned(jiff::tz::TimeZone::UTC).date();
    let total = domovoi_core::ports::UsageRepo::month_total(&*w.store, month)
        .await
        .unwrap();
    assert_eq!(total.cost_micros, 0, "an unknown price is not free");
    assert_eq!(total.unpriced_responses, 1);
    assert_eq!((total.tokens_in, total.tokens_out), (10, 5));

    // A genuine zero price is a different value from an unknown one.
    let free = world(Setup::default()).await;
    free.primary.push_text(report());
    assert!(matches!(free.run().await, TriageOutcome::Report(_)));
    let zero = row(&free, 0);
    assert_eq!(zero.cost_micros, Some(0));
    assert_eq!(zero.usage_source, Some(UsageSource::Reported));
}

/// Captures every tracing event's fields, so a test can prove a secret never reaches a log.
#[derive(Clone, Default)]
struct CapturedLines(Arc<Mutex<Vec<String>>>);

struct FieldVisitor<'a>(&'a mut String);

impl tracing::field::Visit for FieldVisitor<'_> {
    fn record_debug(&mut self, _field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        use std::fmt::Write as _;
        let _ = write!(self.0, "{value:?} ");
    }

    fn record_str(&mut self, _field: &tracing::field::Field, value: &str) {
        use std::fmt::Write as _;
        let _ = write!(self.0, "{value} ");
    }
}

impl tracing::Subscriber for CapturedLines {
    fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let mut line = String::new();
        event.record(&mut FieldVisitor(&mut line));
        if let Ok(mut lines) = self.0.lock() {
            lines.push(line);
        }
    }

    fn enter(&self, _span: &tracing::span::Id) {}

    fn exit(&self, _span: &tracing::span::Id) {}
}

#[tokio::test]
async fn provider_error_redacted() {
    let lines = CapturedLines::default();
    let _guard = tracing::subscriber::set_default(lines.clone());
    let w = world(Setup::default()).await;
    w.primary
        .push_err(LlmError::Unavailable(format!("upstream said {SECRET}")));

    let outcome = w.run().await;
    let TriageOutcome::Failed { reason } = outcome else {
        unreachable!("the provider failure fails the run");
    };
    assert!(
        !reason.contains(SECRET),
        "the run reason leaks a secret: {reason}"
    );

    w.primary
        .push_err(LlmError::Unavailable(format!("upstream said {SECRET}")));
    let failed = w
        .triage
        .follow_up(w.incident.id(), "Why?", UserId::new())
        .await
        .expect_err("the provider failure fails the question");
    assert!(
        !failed.to_string().contains(SECRET),
        "the question error leaks a secret: {failed}"
    );

    for row in w.store.usage_rows() {
        assert_eq!(row.outcome.as_str(), "unavailable");
        assert_eq!(row.cost_micros, None);
        assert_eq!(row.usage_source, None);
    }
    let captured = lines.0.lock().unwrap().join("\n");
    assert!(
        !captured.contains(SECRET),
        "a log line leaks a secret: {captured}"
    );
}

#[tokio::test]
async fn replayed_follow_up_history_is_redacted() {
    let w = world(Setup::default()).await;
    IncidentRepo::append_event(
        &*w.store,
        &IncidentTimelineEntry {
            incident: w.incident.id(),
            at: T0,
            kind: domovoi_engine::triage::FOLLOW_UP_KIND.to_owned(),
            summary: "Old question?".to_owned(),
            detail: json!({
                "asked_by": "user:admin",
                "question": "What is the token?",
                "answer": format!("It was {SECRET}, but do not say so."),
                "asked_at": T0.as_millisecond(),
            }),
        },
    )
    .await
    .unwrap();
    w.primary.push_text("It is not needed.");

    w.triage
        .follow_up(w.incident.id(), "Anything else?", UserId::new())
        .await
        .unwrap();
    for request in w.primary.requests() {
        assert!(
            !seen(&request).contains(SECRET),
            "a replayed request carries a secret"
        );
    }
}

#[tokio::test]
async fn prompt_over_the_cap_is_trimmed_from_old_tool_results() {
    let settings = TriageSettings {
        max_prompt_bytes: 32 * 1024,
        ..TriageSettings::default()
    };
    let w = world(Setup {
        settings,
        ..Setup::default()
    })
    .await;
    w.link.on_tool(|_, _| {
        Ok(ToolOutput {
            text: "x".repeat(8 * 1024),
            truncated: false,
        })
    });
    for call in 1..=6 {
        let id = format!("c{call}");
        w.primary
            .push_ok(read_calls(&[(id.as_str(), "host_overview", json!({}))]));
    }
    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));

    let requests = w.primary.requests();
    let first_message = requests[0].messages[0].clone();
    for request in &requests {
        assert!(
            size(request) <= 32 * 1024,
            "a request is over the cap: {} bytes",
            size(request)
        );
        assert_eq!(
            request.messages[0], first_message,
            "the opening message is never trimmed"
        );
    }
    let last = requests.last().unwrap();
    let newest = results(last);
    assert_eq!(newest.len(), 1, "the newest tool result is still there");
    assert!(
        newest[0].0.len() >= 4 * 1024,
        "the newest tool result was not cut away"
    );
    let old_placeholder = requests.iter().any(|request| {
        request.messages[..request.messages.len().saturating_sub(1)]
            .iter()
            .flat_map(|message| &message.parts)
            .any(|part| {
                matches!(part, Part::ToolResult { content, .. }
                    if content.contains("earlier tool output omitted"))
            })
    });
    assert!(
        old_placeholder,
        "an old tool result carries the placeholder"
    );
}

fn resolved_with_title(
    host: domovoi_core::HostId,
    target: &str,
    summary: &str,
    at: jiff::Timestamp,
) -> Incident {
    let signal = Signal::firing(
        SourceKind::Agent,
        host,
        CheckKey::parse("docker.container").unwrap(),
        Some(TargetKey::parse(target).unwrap()),
        Severity::Warning,
        summary.to_owned(),
        at,
    );
    let mut incident = Incident::new_pending(&signal, at);
    apply(
        &mut incident,
        IncidentEvent::Firing {
            severity: Severity::Warning,
            class: SignalClass::Immediate,
            observed_at: at,
            value: None,
        },
        &IncidentPolicy::default(),
        at,
    )
    .unwrap();
    apply(
        &mut incident,
        IncidentEvent::Resolved { observed_at: at },
        &IncidentPolicy::default(),
        at,
    )
    .unwrap();
    incident
}

#[tokio::test]
async fn similar_incidents_come_from_the_same_host_only() {
    let w = world(Setup::default()).await;
    let other_host = domovoi_core::HostId::new();
    let here = resolved_with_title(host_id(), "nginx", "nginx on host alpha", T0);
    let elsewhere = resolved_with_title(other_host, "nginx", "nginx on host beta", at(1));
    IncidentRepo::insert(&*w.store, &here).await.unwrap();
    IncidentRepo::insert(&*w.store, &elsewhere).await.unwrap();

    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));
    let prompt = seen(&w.primary.requests()[0]);
    assert!(
        prompt.contains("nginx on host alpha"),
        "the same host's similar incident is shown"
    );
    assert!(
        !prompt.contains("nginx on host beta"),
        "another host's incident never enters the prompt"
    );
}

#[tokio::test]
#[ignore = "measurement fixture for the prompt-size default; run by hand with --ignored"]
async fn measure_prompt_sizes() {
    use std::io::Write as _;

    let settings = TriageSettings {
        max_prompt_bytes: 1024 * 1024,
        ..TriageSettings::default()
    };
    let w = world(Setup {
        settings,
        ..Setup::default()
    })
    .await;
    for sample in 0..10 {
        add_signal(&w.store, &w.incident, disk(90.0 + f64::from(sample)), T0).await;
    }
    for index in 0..3 {
        w.note(&format!("Runbook {index}"), "Restart the container.")
            .await;
    }
    w.link.on_tool(|_, _| {
        Ok(ToolOutput {
            text: "y".repeat(8 * 1024),
            truncated: false,
        })
    });
    for call in 1..=15 {
        let id = format!("c{call}");
        w.primary
            .push_ok(read_calls(&[(id.as_str(), "host_overview", json!({}))]));
    }
    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));

    let mut sizes: Vec<usize> = w.primary.requests().iter().map(size).collect();
    let sorted = {
        let mut sorted = sizes.clone();
        sorted.sort_unstable();
        sorted
    };
    let median = sorted[sorted.len() / 2];
    let p95 = sorted[(sorted.len() * 95 / 100).min(sorted.len() - 1)];
    let max = *sorted.last().unwrap();
    let _ = writeln!(
        std::io::stderr(),
        "triage prompt_bytes: calls={} median={median} p95={p95} max={max} sizes={sizes:?}",
        sizes.len()
    );
    sizes.clear();

    // A ten-turn follow-up conversation, as the incident thread replays it.
    for index in 0..10 {
        IncidentRepo::append_event(
            &*w.store,
            &IncidentTimelineEntry {
                incident: w.incident.id(),
                at: at(i64::from(index)),
                kind: domovoi_engine::triage::FOLLOW_UP_KIND.to_owned(),
                summary: "Earlier question".to_owned(),
                detail: json!({
                    "asked_by": "user:admin",
                    "question": "What is happening?",
                    "answer": "z".repeat(1024),
                    "asked_at": at(i64::from(index)).as_millisecond(),
                }),
            },
        )
        .await
        .unwrap();
    }
    w.primary.push_text("It is the config.");
    let before = w.primary.requests().len();
    w.triage
        .follow_up(w.incident.id(), "Anything else?", UserId::new())
        .await
        .unwrap();
    let follow_up: Vec<usize> = w.primary.requests()[before..].iter().map(size).collect();
    let _ = writeln!(
        std::io::stderr(),
        "follow-up prompt_bytes: calls={} sizes={follow_up:?}",
        follow_up.len()
    );

    // A general chat with a ten-turn history.
    let history: Vec<domovoi_engine::triage::ChatTurn> = (0..10)
        .map(|index| domovoi_engine::triage::ChatTurn {
            role: if index % 2 == 0 {
                domovoi_engine::triage::ChatRole::User
            } else {
                domovoi_engine::triage::ChatRole::Assistant
            },
            text: "w".repeat(1024),
        })
        .collect();
    w.primary.push_text("It looks healthy.");
    let before = w.primary.requests().len();
    w.triage
        .chat(
            Some(host_id()),
            &"o".repeat(8 * 1024),
            &history,
            "How is it?",
        )
        .await
        .unwrap();
    let chat: Vec<usize> = w.primary.requests()[before..].iter().map(size).collect();
    let _ = writeln!(
        std::io::stderr(),
        "chat prompt_bytes: calls={} sizes={chat:?}",
        chat.len()
    );
}
