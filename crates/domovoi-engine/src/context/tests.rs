//! Aggregator, caps and rendering over scripted sources.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use domovoi_core::context::{
    ContextBatch, ContextGap, ContextItem, ContextItemId, ContextLayer, ContextQuery, ContextScope,
    ContextSelector, ContextSourceId, ItemStatus, Origin, Provenance,
};
use domovoi_core::id::{HostId, IncidentId};
use domovoi_core::ports::{Clock, ContextSource, Redactor};
use domovoi_testkit::context::SelectorKind;
use domovoi_testkit::{FakeClock, FakeContextSource, ListRedactor};
use jiff::{SignedDuration, Timestamp};
use uuid::Uuid;

use super::layout::Omission;
use super::normalize;
use super::plan::{ContextBudget, ContextPlan, ContextPurpose, LayerCaps};
use super::render::render_section;
use super::{Aggregator, LayeredContext, title_of};

/// The fake clock's start, as seconds since the epoch.
const NOW: i64 = 1_700_000_000;

/// The order in which the total cap gives layers up.
const GIVE_UP_ORDER: [ContextLayer; 6] = [
    ContextLayer::Experience,
    ContextLayer::Knowledge,
    ContextLayer::Evidence,
    ContextLayer::Host,
    ContextLayer::Capabilities,
    ContextLayer::Incident,
];

fn ts(seconds: i64) -> Timestamp {
    Timestamp::from_second(seconds).unwrap()
}

fn sid(name: &str) -> ContextSourceId {
    ContextSourceId::parse(name).unwrap()
}

/// A known, observed item of `source` with a fixed observation time.
fn item(source: &str, layer: ContextLayer, key: &str, body: &str) -> ContextItem {
    let source = sid(source);
    ContextItem {
        id: ContextItemId::new(&source, key),
        source,
        layer,
        title: format!("item {key}"),
        body: body.to_owned(),
        status: ItemStatus::Known,
        provenance: Provenance {
            origin: Origin::Observed,
            reference: format!("ref:{key}"),
            observed_at: Some(ts(NOW)),
            actor: None,
        },
        truncated: false,
    }
}

/// `count` items named `i0`, `i1`, ... with bodies of `body_len` bytes.
fn many_items(
    source: &str,
    layer: ContextLayer,
    count: usize,
    body_len: usize,
) -> Vec<ContextItem> {
    (0..count)
        .map(|index| item(source, layer, &format!("i{index}"), &"x".repeat(body_len)))
        .collect()
}

/// A source that returns pre-built batches and records the queries it was asked.
struct StubSource {
    id: ContextSourceId,
    layer: ContextLayer,
    overview: Option<ContextBatch>,
    search: Option<ContextBatch>,
    delay: Option<Duration>,
    queries: Mutex<Vec<ContextQuery>>,
}

impl StubSource {
    fn new(id: &str, layer: ContextLayer) -> Self {
        Self {
            id: sid(id),
            layer,
            overview: None,
            search: None,
            delay: None,
            queries: Mutex::new(Vec::new()),
        }
    }

    /// Answers an overview with `items`.
    fn items(mut self, items: Vec<ContextItem>) -> Self {
        self.overview = Some(ContextBatch {
            items,
            gaps: Vec::new(),
        });
        self
    }

    /// Answers a search with `items`.
    fn search(mut self, items: Vec<ContextItem>) -> Self {
        self.search = Some(ContextBatch {
            items,
            gaps: Vec::new(),
        });
        self
    }

    /// Answers an overview with no item and one gap.
    fn gap(mut self, reason: &str) -> Self {
        self.overview = Some(ContextBatch {
            items: Vec::new(),
            gaps: vec![ContextGap {
                source: self.id.clone(),
                reason: reason.to_owned(),
            }],
        });
        self
    }

    /// Answers an overview with nothing at all.
    fn empty(mut self) -> Self {
        self.overview = Some(ContextBatch::default());
        self
    }

    /// Takes `delay` (in tokio time) before answering.
    fn slow(mut self, delay: Duration) -> Self {
        self.delay = Some(delay);
        self
    }

    fn asked(&self) -> Vec<ContextQuery> {
        self.queries.lock().unwrap().clone()
    }

    fn arc(self) -> Arc<Self> {
        Arc::new(self)
    }
}

#[async_trait]
impl ContextSource for StubSource {
    fn id(&self) -> ContextSourceId {
        self.id.clone()
    }

    fn layer(&self) -> ContextLayer {
        self.layer
    }

    fn supports(&self, selector: &ContextSelector) -> bool {
        match selector {
            ContextSelector::Overview => self.overview.is_some(),
            ContextSelector::Search { .. } => self.search.is_some(),
            ContextSelector::Read { .. } => false,
        }
    }

    async fn fetch(&self, query: &ContextQuery) -> ContextBatch {
        self.queries.lock().unwrap().push(query.clone());
        if let Some(delay) = self.delay {
            tokio::time::sleep(delay).await;
        }
        match query.selector {
            ContextSelector::Overview => self.overview.clone(),
            ContextSelector::Search { .. } => self.search.clone(),
            ContextSelector::Read { .. } => None,
        }
        .unwrap_or_default()
    }
}

fn scope() -> ContextScope {
    ContextScope::Incident {
        host: HostId::from_uuid(Uuid::from_u128(1)),
        incident: IncidentId::from_uuid(Uuid::from_u128(2)),
    }
}

/// A budget no test content reaches unless the test lowers a limit.
fn wide_budget() -> ContextBudget {
    let mut budget = ContextBudget::for_purpose(ContextPurpose::Investigate, 4 << 20);
    budget.total_bytes = 1 << 20;
    budget
}

fn plan() -> ContextPlan {
    ContextPlan {
        purpose: ContextPurpose::Investigate,
        scope: scope(),
        search_text: None,
        budget: wide_budget(),
    }
}

fn aggregator(sources: &[Arc<StubSource>], secrets: &[&str]) -> Aggregator {
    let sources = sources
        .iter()
        .map(|source| Arc::clone(source) as Arc<dyn ContextSource>)
        .collect();
    Aggregator::new(
        sources,
        Arc::new(ListRedactor::new(secrets.iter().copied())),
        Arc::new(FakeClock::new(ts(NOW))),
    )
}

async fn gather(sources: &[Arc<StubSource>], plan: &ContextPlan) -> LayeredContext {
    aggregator(sources, &[]).gather(plan).await
}

fn all_items(ctx: &LayeredContext) -> Vec<&ContextItem> {
    ctx.sections.iter().flat_map(|s| s.items.iter()).collect()
}

fn count(ctx: &LayeredContext, layer: ContextLayer) -> usize {
    ctx.sections
        .iter()
        .filter(|s| s.layer == layer)
        .map(|s| s.items.len())
        .sum()
}

fn omitted(ctx: &LayeredContext, layer: ContextLayer) -> u32 {
    ctx.omissions
        .iter()
        .filter(|o| o.layer == layer)
        .map(|o| o.items)
        .sum()
}

fn has(ctx: &LayeredContext, id: &ContextItemId) -> bool {
    all_items(ctx).iter().any(|i| i.id == *id)
}

/// One source per layer, each with two items of `body_len` bytes.
fn every_layer(body_len: usize) -> Vec<Arc<StubSource>> {
    [
        ("s_incident", ContextLayer::Incident),
        ("s_host", ContextLayer::Host),
        ("s_evidence", ContextLayer::Evidence),
        ("s_experience", ContextLayer::Experience),
        ("s_knowledge", ContextLayer::Knowledge),
        ("s_capabilities", ContextLayer::Capabilities),
    ]
    .into_iter()
    .map(|(id, layer)| {
        StubSource::new(id, layer)
            .items(many_items(id, layer, 2, body_len))
            .arc()
    })
    .collect()
}

#[tokio::test]
async fn layers_render_in_layer_order_whatever_the_registration_order() {
    let sources = vec![
        StubSource::new("caps", ContextLayer::Capabilities)
            .items(many_items("caps", ContextLayer::Capabilities, 1, 5))
            .arc(),
        StubSource::new("know_a", ContextLayer::Knowledge)
            .items(many_items("know_a", ContextLayer::Knowledge, 1, 5))
            .arc(),
        StubSource::new("inc", ContextLayer::Incident)
            .items(many_items("inc", ContextLayer::Incident, 1, 5))
            .arc(),
        StubSource::new("host", ContextLayer::Host)
            .items(many_items("host", ContextLayer::Host, 1, 5))
            .arc(),
        StubSource::new("know_b", ContextLayer::Knowledge)
            .items(many_items("know_b", ContextLayer::Knowledge, 1, 5))
            .arc(),
    ];
    let ctx = gather(&sources, &plan()).await;
    let order: Vec<&str> = ctx.sections.iter().map(|s| s.source.as_str()).collect();
    assert_eq!(order, ["inc", "host", "know_a", "know_b", "caps"]);
    let text = ctx.render();
    let at = |title: &str| text.find(&format!("## {title}\n")).unwrap();
    assert!(at("Incident: inc") < at("Host: host"));
    assert!(at("Host: host") < at("Knowledge: know_a"));
    assert!(at("Knowledge: know_a") < at("Knowledge: know_b"));
    assert!(at("Knowledge: know_b") < at("Capabilities: caps"));
}

#[tokio::test]
async fn layered_context_render_snapshot() {
    let mut budget = wide_budget();
    budget.overview_limit = 2;
    let plan = ContextPlan { budget, ..plan() };

    let mut main = item(
        "incident",
        ContextLayer::Incident,
        "main",
        "container exited with code 137",
    );
    main.title = "Plex is down".to_owned();
    main.provenance.reference = "docker_ps".to_owned();
    let mut deploys = item(
        "incident",
        ContextLayer::Incident,
        "deploys",
        "no reading taken",
    );
    deploys.title = "Recent deploys".to_owned();
    deploys.status = ItemStatus::Unknown {
        reason: "agent offline".to_owned(),
    };

    let seen = ts(NOW - 3 * 86_400);
    let mut disk = item("profile", ContextLayer::Host, "disk", "disk 61% full");
    disk.title = "Disk usage".to_owned();
    disk.status = ItemStatus::Stale { observed_at: seen };
    disk.provenance.observed_at = Some(seen);
    let mut memory = item("profile", ContextLayer::Host, "memory", "memory 40% used");
    memory.title = "Memory".to_owned();
    memory.status = ItemStatus::Disputed {
        with: ContextItemId::new(&sid("notes"), "memory"),
    };
    memory.provenance.origin = Origin::Declared;
    memory.provenance.actor = Some("rostislav".to_owned());

    let mut cut = item(
        "signals",
        ContextLayer::Evidence,
        "log",
        "line 1\nline 2\nline 3",
    );
    cut.title = "Container log".to_owned();
    cut.truncated = true;
    let spare = item("signals", ContextLayer::Evidence, "spare", "second");
    let left_out = item("signals", ContextLayer::Evidence, "left_out", "third");

    let mut past = item(
        "past_runs",
        ContextLayer::Experience,
        "run",
        "restarted plex, it came back",
    );
    past.title = "Restarted plex last week".to_owned();

    let sources = vec![
        StubSource::new("incident", ContextLayer::Incident)
            .items(vec![main, deploys])
            .arc(),
        StubSource::new("profile", ContextLayer::Host)
            .items(vec![disk, memory])
            .arc(),
        StubSource::new("signals", ContextLayer::Evidence)
            .items(vec![cut, spare, left_out])
            .arc(),
        StubSource::new("past_runs", ContextLayer::Experience)
            .items(vec![past])
            .arc(),
        StubSource::new("notes", ContextLayer::Knowledge)
            .gap("note store unavailable")
            .arc(),
        StubSource::new("skills", ContextLayer::Capabilities)
            .empty()
            .arc(),
    ];
    let ctx = gather(&sources, &plan).await;
    insta::assert_snapshot!("layered_context_render_snapshot", ctx.render());
}

#[tokio::test]
async fn a_gap_renders_unknown_never_none() {
    let sources = vec![
        StubSource::new("src", ContextLayer::Knowledge)
            .gap("disk unreadable")
            .arc(),
    ];
    let ctx = gather(&sources, &plan()).await;
    let text = ctx.render();
    assert!(ctx.sections[0].items.is_empty());
    assert!(text.contains("[knowledge | src | UNKNOWN: disk unreadable] Knowledge: src"));
    assert!(text.contains("nothing could be read from this source."));
    assert!(!text.contains("None."));
}

#[tokio::test]
async fn a_source_with_nothing_renders_none() {
    let sources = vec![
        StubSource::new("src", ContextLayer::Knowledge)
            .empty()
            .arc(),
    ];
    let ctx = gather(&sources, &plan()).await;
    assert_eq!(ctx.render(), "## Knowledge: src\n\nNone.");
    assert!(ctx.omissions.is_empty());
}

#[tokio::test]
async fn a_source_that_supports_neither_selector_gets_no_section() {
    let sources = vec![StubSource::new("src", ContextLayer::Knowledge).arc()];
    let plan = ContextPlan {
        search_text: Some("disk".to_owned()),
        ..plan()
    };
    let ctx = gather(&sources, &plan).await;
    assert!(ctx.sections.is_empty());
    assert!(sources[0].asked().is_empty());
    assert_eq!(ctx.render(), "");
}

#[tokio::test]
async fn overview_is_asked_first_and_search_only_with_search_text() {
    let clock = Arc::new(FakeClock::new(ts(NOW)));
    let redactor: Arc<dyn Redactor> = Arc::new(ListRedactor::new(Vec::<String>::new()));
    let make = |name: &str| {
        let source = Arc::new(FakeContextSource::new(
            sid(name),
            ContextLayer::Knowledge,
            Arc::clone(&redactor),
            Arc::clone(&clock),
        ));
        source.answer(SelectorKind::Overview, vec!["overview body".to_owned()]);
        source.answer(SelectorKind::Search, vec!["search body".to_owned()]);
        source
    };
    let budget = ContextBudget::for_purpose(ContextPurpose::Investigate, 131_072);
    let deadline = clock
        .now()
        .checked_add(SignedDuration::from_secs(5))
        .unwrap();

    let with_search = make("with_search");
    let aggregator = Aggregator::new(
        vec![Arc::clone(&with_search) as Arc<dyn ContextSource>],
        Arc::clone(&redactor),
        Arc::clone(&clock) as Arc<dyn Clock>,
    );
    let plan = ContextPlan {
        purpose: ContextPurpose::Investigate,
        scope: scope(),
        search_text: Some("disk full".to_owned()),
        budget,
    };
    aggregator.gather(&plan).await;
    let queries = with_search.queries();
    assert_eq!(queries.len(), 2);
    assert_eq!(queries[0].selector, ContextSelector::Overview);
    assert_eq!(
        queries[1].selector,
        ContextSelector::Search {
            text: "disk full".to_owned()
        }
    );
    assert_eq!(queries[0].limit, 10);
    assert_eq!(queries[1].limit, 5);
    for query in &queries {
        assert_eq!(query.max_item_bytes, 4096);
        assert_eq!(query.deadline, deadline);
        assert_eq!(query.scope, scope());
    }

    let without = make("without");
    let aggregator = Aggregator::new(
        vec![Arc::clone(&without) as Arc<dyn ContextSource>],
        Arc::clone(&redactor),
        Arc::clone(&clock) as Arc<dyn Clock>,
    );
    let plan = ContextPlan {
        search_text: None,
        ..plan
    };
    aggregator.gather(&plan).await;
    let queries = without.queries();
    assert_eq!(queries.len(), 1);
    assert_eq!(queries[0].selector, ContextSelector::Overview);
}

#[tokio::test]
async fn overlapping_overview_and_search_answers_are_deduped() {
    let same = item("src", ContextLayer::Knowledge, "a", "same");
    let sources = vec![
        StubSource::new("src", ContextLayer::Knowledge)
            .items(vec![same.clone()])
            .search(vec![same])
            .arc(),
    ];
    let plan = ContextPlan {
        search_text: Some("a".to_owned()),
        ..plan()
    };
    let ctx = gather(&sources, &plan).await;
    assert_eq!(sources[0].asked().len(), 2);
    assert_eq!(ctx.sections[0].items.len(), 1);
    assert_eq!(ctx.render().matches("item a").count(), 1);
}

#[test]
fn dedupe_keeps_the_newest_and_the_first_on_a_tie() {
    let at = |seconds: Option<i64>, key: &str, body: &str| {
        let mut i = item("src", ContextLayer::Knowledge, key, body);
        i.provenance.observed_at = seconds.map(ts);
        i
    };
    let bodies = |items: Vec<ContextItem>| -> Vec<String> {
        normalize::dedupe(items)
            .into_iter()
            .map(|i| i.body)
            .collect()
    };

    // The newer wins and stays at its own position.
    assert_eq!(
        bodies(vec![
            at(Some(10), "a", "old"),
            at(Some(15), "b", "other"),
            at(Some(20), "a", "new"),
        ]),
        ["other", "new"]
    );
    // The newer one first: it stays first.
    assert_eq!(
        bodies(vec![at(Some(20), "a", "new"), at(Some(10), "a", "old")]),
        ["new"]
    );
    // A tie keeps the earlier answer at its position.
    assert_eq!(
        bodies(vec![
            at(Some(10), "a", "first"),
            at(Some(11), "b", "other"),
            at(Some(10), "a", "second"),
        ]),
        ["first", "other"]
    );
    // An item with no time is the oldest.
    assert_eq!(
        bodies(vec![at(None, "a", "undated"), at(Some(1), "a", "dated")]),
        ["dated"]
    );
    assert_eq!(
        bodies(vec![at(None, "a", "first"), at(None, "a", "second")]),
        ["first"]
    );
    // Three copies resolve to the newest.
    assert_eq!(
        bodies(vec![
            at(Some(10), "a", "one"),
            at(Some(20), "a", "two"),
            at(Some(30), "a", "three"),
        ]),
        ["three"]
    );
    assert_eq!(
        bodies(vec![
            at(Some(30), "a", "three"),
            at(Some(20), "a", "two"),
            at(Some(10), "a", "one"),
        ]),
        ["three"]
    );
}

#[tokio::test]
async fn a_known_and_an_unknown_item_with_the_same_id_are_both_rendered() {
    let known = item("src", ContextLayer::Knowledge, "a", "last good reading");
    let mut unknown = item("src", ContextLayer::Knowledge, "a", "");
    unknown.status = ItemStatus::Unknown {
        reason: "newest read failed".to_owned(),
    };
    let sources = vec![
        StubSource::new("src", ContextLayer::Knowledge)
            .items(vec![known.clone()])
            .search(vec![unknown.clone()])
            .arc(),
    ];
    let plan = ContextPlan {
        search_text: Some("a".to_owned()),
        ..plan()
    };
    let ctx = gather(&sources, &plan).await;
    assert_eq!(ctx.sections[0].items.len(), 2);
    let text = ctx.render();
    assert!(text.contains("| known]"));
    assert!(text.contains("UNKNOWN: newest read failed"));
    assert_eq!(normalize::dedupe(vec![known, unknown]).len(), 2);
}

#[tokio::test(start_paused = true)]
async fn a_source_past_its_deadline_becomes_a_timed_out_gap() {
    let slow = StubSource::new("slow", ContextLayer::Host)
        .items(many_items("slow", ContextLayer::Host, 1, 5))
        .slow(Duration::from_secs(10))
        .arc();
    let next = StubSource::new("next", ContextLayer::Knowledge)
        .items(many_items("next", ContextLayer::Knowledge, 1, 5))
        .arc();
    let ctx = gather(&[Arc::clone(&slow), Arc::clone(&next)], &plan()).await;

    let slow_section = &ctx.sections[0];
    assert_eq!(slow_section.source, sid("slow"));
    assert!(slow_section.items.is_empty());
    assert_eq!(slow_section.gaps.len(), 1);
    assert_eq!(slow_section.gaps[0].reason, "timed out");
    assert_eq!(slow_section.gaps[0].source, sid("slow"));
    assert_eq!(next.asked().len(), 1);
    assert_eq!(ctx.sections[1].items.len(), 1);
    assert!(ctx.render().contains("UNKNOWN: timed out"));
}

#[tokio::test]
async fn an_item_is_restamped_to_its_source_and_layer() {
    let stray = item("other", ContextLayer::Incident, "x", "body");
    let sources = vec![
        StubSource::new("alpha", ContextLayer::Knowledge)
            .items(vec![stray])
            .arc(),
    ];
    let ctx = gather(&sources, &plan()).await;
    let only = &ctx.sections[0].items[0];
    assert_eq!(only.source, sid("alpha"));
    assert_eq!(only.layer, ContextLayer::Knowledge);
    let text = ctx.render();
    assert!(text.contains("[knowledge | alpha | known]"));
    assert!(!text.contains("[incident"));
    assert!(!text.contains("| other |"));
}

#[tokio::test]
async fn experience_items_are_forced_historical() {
    let sources = vec![
        StubSource::new("past_ops", ContextLayer::Experience)
            .items(vec![item("past_ops", ContextLayer::Experience, "r", "ran")])
            .arc(),
    ];
    let ctx = gather(&sources, &plan()).await;
    assert_eq!(
        ctx.sections[0].items[0].provenance.origin,
        Origin::Historical
    );
    assert_eq!(
        ctx.sections[0].title,
        "Historical evidence, not authorization: past_ops"
    );
    let text = ctx.render();
    assert!(text.contains("from: historical evidence, not authorization ref:r"));
    assert!(text.starts_with("## Historical evidence, not authorization: past_ops"));
}

#[tokio::test]
async fn secrets_are_redacted_before_the_cut() {
    const SECRET: &str = "SECRETVALUE1234";
    let mut straddling = item(
        "src",
        ContextLayer::Knowledge,
        "a",
        &format!("{}{SECRET}tail", "a".repeat(4085)),
    );
    straddling.title = format!("title {SECRET}");
    straddling.provenance.reference = format!("ref {SECRET}");
    straddling.provenance.actor = Some(format!("actor {SECRET}"));
    let mut unknown = item("src", ContextLayer::Knowledge, "b", "none");
    unknown.status = ItemStatus::Unknown {
        reason: format!("reason {SECRET}"),
    };
    let source = StubSource::new("src", ContextLayer::Knowledge)
        .items(vec![straddling, unknown])
        .arc();
    let mut with_gap = StubSource::new("gappy", ContextLayer::Host);
    with_gap.overview = Some(ContextBatch {
        items: Vec::new(),
        gaps: vec![ContextGap {
            source: sid("gappy"),
            reason: format!("gap {SECRET}"),
        }],
    });
    let sources = vec![source, with_gap.arc()];
    let ctx = aggregator(&sources, &[SECRET]).gather(&plan()).await;
    let text = ctx.render();
    assert!(!text.contains(SECRET));
    assert!(!text.contains("SECRE"));
    assert!(text.contains("[REDACTED]"));
    assert!(text.contains("title [REDACTED]"));
    assert!(text.contains("ref [REDACTED]"));
    let actor = all_items(&ctx)
        .iter()
        .find_map(|i| i.provenance.actor.clone());
    assert_eq!(actor.as_deref(), Some("actor [REDACTED]"));
    assert!(text.contains("reason [REDACTED]"));
    assert!(text.contains("gap [REDACTED]"));
    let body = &all_items(&ctx)
        .iter()
        .find(|i| i.id.as_str() == "src:a")
        .unwrap()
        .body;
    assert!(body.len() <= 4096);
}

#[tokio::test]
async fn a_body_is_cut_on_a_character_boundary() {
    // 2000 characters of 3 bytes each: 4096 falls inside a character.
    let body = "\u{20ac}".repeat(2000);
    let sources = vec![
        StubSource::new("src", ContextLayer::Knowledge)
            .items(vec![item("src", ContextLayer::Knowledge, "a", &body)])
            .arc(),
    ];
    let ctx = gather(&sources, &plan()).await;
    let only = &ctx.sections[0].items[0];
    assert!(only.truncated);
    assert_eq!(only.body.len(), 4095);
    assert!(only.body.chars().all(|c| c == '\u{20ac}'));
    assert!(ctx.render().ends_with("\n[cut]"));
}

#[tokio::test]
async fn more_items_than_the_limit_are_omitted_and_counted() {
    let sources = vec![
        StubSource::new("src", ContextLayer::Knowledge)
            .items(many_items("src", ContextLayer::Knowledge, 12, 5))
            .arc(),
    ];
    let ctx = gather(&sources, &plan()).await;
    assert_eq!(ctx.sections[0].items.len(), 10);
    assert_eq!(
        ctx.omissions,
        [Omission {
            layer: ContextLayer::Knowledge,
            source: sid("src"),
            items: 2,
        }]
    );
    let text = ctx.render();
    assert!(text.contains("## Context left out to fit the size limit"));
    assert!(text.contains("- knowledge: 2 items from src"));
}

#[tokio::test]
async fn a_layer_cap_drops_from_the_end_and_names_it() {
    let cap = 1000;
    let mut plan = plan();
    plan.budget.layers = LayerCaps {
        knowledge: cap,
        ..LayerCaps::DEFAULT
    };
    let sources = vec![
        StubSource::new("src", ContextLayer::Knowledge)
            .items(many_items("src", ContextLayer::Knowledge, 5, 200))
            .arc(),
    ];
    let ctx = gather(&sources, &plan).await;
    let kept = ctx.sections[0].items.len();
    assert!((1..5).contains(&kept), "kept {kept}");
    let ids: Vec<&str> = ctx.sections[0]
        .items
        .iter()
        .map(|i| i.id.as_str())
        .collect();
    let expected: Vec<String> = (0..kept).map(|n| format!("src:i{n}")).collect();
    assert_eq!(ids, expected);
    assert!(render_section(&ctx.sections[0]).len() <= cap);
    let dropped = u32::try_from(5 - kept).unwrap();
    assert_eq!(
        ctx.omissions,
        [Omission {
            layer: ContextLayer::Knowledge,
            source: sid("src"),
            items: dropped,
        }]
    );
    assert!(
        ctx.render()
            .contains(&format!("- knowledge: {dropped} items from src"))
    );
}

#[tokio::test]
async fn an_item_alone_over_its_layer_cap_is_cut_not_dropped() {
    let sources = vec![
        StubSource::new("src", ContextLayer::Knowledge)
            .items(many_items("src", ContextLayer::Knowledge, 1, 3000))
            .arc(),
    ];
    let mut plan = plan();
    plan.budget.layers = LayerCaps {
        knowledge: 1000,
        ..LayerCaps::DEFAULT
    };
    let ctx = gather(&sources, &plan).await;
    let only = &ctx.sections[0].items[0];
    assert!(only.truncated);
    assert!(
        (256..3000).contains(&only.body.len()),
        "{}",
        only.body.len()
    );
    assert!(render_section(&ctx.sections[0]).len() <= 1000);
    assert!(ctx.omissions.is_empty());

    // A cap that no cut can meet stops at the floor.
    plan.budget.layers = LayerCaps {
        knowledge: 100,
        ..LayerCaps::DEFAULT
    };
    let ctx = gather(&sources, &plan).await;
    assert_eq!(ctx.sections[0].items.len(), 1);
    assert_eq!(ctx.sections[0].items[0].body.len(), 256);
    assert!(ctx.sections[0].items[0].truncated);
    assert!(ctx.omissions.is_empty());
}

#[tokio::test]
async fn the_total_cap_gives_up_experience_then_knowledge_first() {
    let sources = every_layer(400);
    let mut plan = plan();
    let full = gather(&sources, &plan).await.render().len();

    // One byte under the full size: the first thing to go is experience.
    plan.budget.total_bytes = full - 1;
    let ctx = gather(&sources, &plan).await;
    assert!(omitted(&ctx, ContextLayer::Experience) > 0);
    for layer in &GIVE_UP_ORDER[1..] {
        assert_eq!(omitted(&ctx, *layer), 0, "{layer:?}");
    }

    // At every size: a layer loses items only once the layers before it have none left.
    let mut touched = Vec::new();
    let mut total = full;
    loop {
        plan.budget.total_bytes = total;
        let ctx = gather(&sources, &plan).await;
        for (index, layer) in GIVE_UP_ORDER.iter().enumerate() {
            if omitted(&ctx, *layer) > 0 {
                if !touched.contains(layer) {
                    touched.push(*layer);
                }
                for earlier in &GIVE_UP_ORDER[..index] {
                    assert_eq!(count(&ctx, *earlier), 0, "{earlier:?} before {layer:?}");
                }
            }
        }
        if total < 97 {
            break;
        }
        total -= 97;
    }
    assert_eq!(touched, GIVE_UP_ORDER);
}

#[tokio::test]
async fn the_incident_item_is_never_dropped() {
    let sources = every_layer(1000);
    let mut plan = plan();
    plan.budget.total_bytes = 10;
    let ctx = gather(&sources, &plan).await;
    assert_eq!(ctx.sections.len(), 1);
    assert_eq!(ctx.sections[0].layer, ContextLayer::Incident);
    assert_eq!(ctx.sections[0].items.len(), 1);
    let first = &ctx.sections[0].items[0];
    assert_eq!(first.id.as_str(), "s_incident:i0");
    assert_eq!(first.body.len(), 256);
    assert!(first.truncated);
    assert_eq!(omitted(&ctx, ContextLayer::Incident), 1);
    assert_eq!(omitted(&ctx, ContextLayer::Host), 2);
}

#[tokio::test]
async fn unknown_items_and_gaps_survive_until_the_last_step() {
    let mut unknown = item("notes", ContextLayer::Knowledge, "u1", &"u".repeat(200));
    unknown.status = ItemStatus::Unknown {
        reason: "read failed".to_owned(),
    };
    let known_1 = item("notes", ContextLayer::Knowledge, "k1", &"k".repeat(200));
    let known_2 = item("notes", ContextLayer::Knowledge, "k2", &"k".repeat(200));
    let sources = vec![
        StubSource::new("notes", ContextLayer::Knowledge)
            .items(vec![known_1, unknown, known_2])
            .arc(),
        StubSource::new("skills", ContextLayer::Capabilities)
            .gap("catalogue unreadable")
            .arc(),
    ];
    let (k1, u1, k2) = (
        ContextItemId::new(&sid("notes"), "k1"),
        ContextItemId::new(&sid("notes"), "u1"),
        ContextItemId::new(&sid("notes"), "k2"),
    );
    let mut plan = plan();
    let full = gather(&sources, &plan).await.render().len();
    let mut unknown_alone = false;
    let mut total = full;
    loop {
        plan.budget.total_bytes = total;
        let ctx = gather(&sources, &plan).await;
        if !has(&ctx, &u1) {
            assert!(!has(&ctx, &k1) && !has(&ctx, &k2), "total {total}");
        }
        unknown_alone |= has(&ctx, &u1) && !has(&ctx, &k1) && !has(&ctx, &k2);
        let skills = ctx.sections.iter().find(|s| s.source == sid("skills"));
        assert_eq!(skills.map(|s| s.gaps.len()), Some(1), "total {total}");
        if total < 20 {
            break;
        }
        total -= 20;
    }
    assert!(unknown_alone);

    plan.budget.total_bytes = 0;
    let ctx = gather(&sources, &plan).await;
    assert_eq!(count(&ctx, ContextLayer::Knowledge), 0);
    assert_eq!(
        ctx.omissions,
        [Omission {
            layer: ContextLayer::Knowledge,
            source: sid("notes"),
            items: 3,
        }]
    );
    assert!(
        ctx.render()
            .contains("nothing could be read from this source.")
    );
}

#[tokio::test]
async fn a_disputed_pair_is_never_split_by_a_cap() {
    let notes = sid("notes");
    let (a_id, b_id) = (
        ContextItemId::new(&notes, "a"),
        ContextItemId::new(&notes, "b"),
    );
    let plain = item("notes", ContextLayer::Knowledge, "c", &"c".repeat(200));
    let mut a = item("notes", ContextLayer::Knowledge, "a", &"a".repeat(200));
    a.status = ItemStatus::Disputed { with: b_id.clone() };
    let mut b = item("notes", ContextLayer::Knowledge, "b", &"b".repeat(200));
    b.status = ItemStatus::Disputed { with: a_id.clone() };
    let sources = vec![
        StubSource::new("notes", ContextLayer::Knowledge)
            .items(vec![plain, a, b])
            .arc(),
    ];

    let mut plan = plan();
    let full = gather(&sources, &plan).await.render().len();
    let mut both_gone = false;
    let mut total = full;
    loop {
        plan.budget.total_bytes = total;
        let ctx = gather(&sources, &plan).await;
        assert_eq!(has(&ctx, &a_id), has(&ctx, &b_id), "total {total}");
        both_gone |= !has(&ctx, &a_id);
        if total < 20 {
            break;
        }
        total -= 20;
    }
    assert!(both_gone);

    let mut plan = self::plan();
    for cap in (0..1500).step_by(50) {
        plan.budget.layers = LayerCaps {
            knowledge: cap,
            ..LayerCaps::DEFAULT
        };
        let ctx = gather(&sources, &plan).await;
        assert_eq!(has(&ctx, &a_id), has(&ctx, &b_id), "cap {cap}");
    }
    plan.budget.layers = LayerCaps {
        knowledge: 0,
        ..LayerCaps::DEFAULT
    };
    let ctx = gather(&sources, &plan).await;
    // The pair goes together; the last item of a layer is cut, never dropped, by a layer cap.
    assert_eq!(omitted(&ctx, ContextLayer::Knowledge), 2);
    assert_eq!(count(&ctx, ContextLayer::Knowledge), 1);
}

#[tokio::test]
async fn an_item_disputed_with_the_incident_is_not_dropped_without_it() {
    let incident = sid("incident");
    let other = sid("signals");
    let incident_id = ContextItemId::new(&incident, "i");
    let other_id = ContextItemId::new(&other, "s");
    let mut first = item("incident", ContextLayer::Incident, "i", &"i".repeat(200));
    first.status = ItemStatus::Disputed {
        with: other_id.clone(),
    };
    let mut second = item("signals", ContextLayer::Incident, "s", &"s".repeat(200));
    second.status = ItemStatus::Disputed {
        with: incident_id.clone(),
    };
    let sources = vec![
        StubSource::new("incident", ContextLayer::Incident)
            .items(vec![first])
            .arc(),
        StubSource::new("signals", ContextLayer::Incident)
            .items(vec![second])
            .arc(),
    ];
    let mut plan = plan();
    for total in (0..800).step_by(25) {
        plan.budget.total_bytes = total;
        let ctx = gather(&sources, &plan).await;
        assert!(has(&ctx, &incident_id), "the incident stays at {total}");
        assert!(has(&ctx, &other_id), "its counterpart stays at {total}");
    }
}

#[test]
fn omission_section_text_is_fixed() {
    let one = LayeredContext {
        sections: Vec::new(),
        omissions: vec![Omission {
            layer: ContextLayer::Evidence,
            source: sid("signals"),
            items: 1,
        }],
    };
    insta::assert_snapshot!("omission_section_one_item", one.render());
    let several = LayeredContext {
        sections: Vec::new(),
        omissions: vec![
            Omission {
                layer: ContextLayer::Evidence,
                source: sid("signals"),
                items: 3,
            },
            Omission {
                layer: ContextLayer::Experience,
                source: sid("past_runs"),
                items: 1,
            },
            Omission {
                layer: ContextLayer::Knowledge,
                source: sid("notes"),
                items: 12,
            },
        ],
    };
    insta::assert_snapshot!("omission_section_several_items", several.render());
}

#[test]
fn the_budget_follows_the_prompt_cap() {
    const KIB: usize = 1024;
    let totals = |cap: usize| {
        [
            ContextPurpose::Investigate,
            ContextPurpose::FollowUp,
            ContextPurpose::Chat,
        ]
        .map(|purpose| ContextBudget::for_purpose(purpose, cap).total_bytes)
    };
    assert_eq!(totals(131_072), [30 * KIB, 32 * KIB, 24 * KIB]);
    assert_eq!(totals(32_768), [6 * KIB, 8 * KIB, 8 * KIB]);
    assert_eq!(totals(4 << 20), [30 * KIB, 40 * KIB, 24 * KIB]);
    // Below the floor the floor applies (the investigation keeps its reserve out of it).
    assert_eq!(totals(0), [6 * KIB, 8 * KIB, 8 * KIB]);

    let budget = ContextBudget::for_purpose(ContextPurpose::Chat, 131_072);
    assert_eq!(budget.layers, LayerCaps::DEFAULT);
    assert_eq!(budget.item_bytes, 4096);
    assert_eq!(budget.overview_limit, 10);
    assert_eq!(budget.search_limit, 5);
    assert_eq!(budget.source_deadline, SignedDuration::from_secs(5));
}

#[test]
fn titles_cover_the_known_sources_and_label_the_fallbacks() {
    use ContextLayer::{Capabilities, Evidence, Experience, Host, Incident, Knowledge};
    use ContextPurpose::{Chat, FollowUp, Investigate};
    let known = [
        ("incident", Investigate, Incident, "Incident"),
        (
            "signals",
            Investigate,
            Evidence,
            "Recent signals, newest first",
        ),
        (
            "profile",
            Investigate,
            Host,
            "Host profile (the normal state of this host)",
        ),
        ("notes", Investigate, Knowledge, "Host notes"),
        (
            "changes",
            Investigate,
            Host,
            "What changed in the last 24 hours",
        ),
        (
            "similar_incidents",
            Investigate,
            Experience,
            "Similar resolved incidents",
        ),
        (
            "skills",
            Investigate,
            Capabilities,
            "Skills you may propose",
        ),
        ("skills", FollowUp, Capabilities, "Skills you may propose"),
        (
            "skills",
            Chat,
            Capabilities,
            "Read-only skills you may use on this host",
        ),
        (
            "incident_thread",
            FollowUp,
            Incident,
            "This incident so far: report, actions and timeline",
        ),
    ];
    for (source, purpose, layer, expected) in known {
        assert_eq!(title_of(&sid(source), purpose, layer), expected, "{source}");
    }
    let fallbacks = [
        (Incident, "Incident: other"),
        (Host, "Host: other"),
        (Evidence, "Evidence: other"),
        (Experience, "Historical evidence, not authorization: other"),
        (Knowledge, "Knowledge: other"),
        (Capabilities, "Capabilities: other"),
    ];
    for (layer, expected) in fallbacks {
        assert_eq!(title_of(&sid("other"), Investigate, layer), expected);
    }
}

#[tokio::test]
async fn rendering_is_deterministic() {
    let plan = ContextPlan {
        search_text: Some("disk".to_owned()),
        ..plan()
    };
    let build = || {
        every_layer(120)
            .into_iter()
            .map(|source| {
                let mut stub = Arc::into_inner(source).unwrap();
                let id = stub.id.as_str().to_owned();
                stub.search = Some(ContextBatch {
                    items: many_items(&id, stub.layer, 1, 30),
                    gaps: Vec::new(),
                });
                stub.arc()
            })
            .collect::<Vec<_>>()
    };
    let first = gather(&build(), &plan).await;
    let second = gather(&build(), &plan).await;
    assert_eq!(first, second);
    assert_eq!(first.render(), second.render());
    assert!(!first.render().is_empty());
}

#[tokio::test]
async fn an_empty_plan_renders_the_empty_string() {
    let ctx = gather(&[], &plan()).await;
    assert!(ctx.sections.is_empty());
    assert!(ctx.omissions.is_empty());
    assert_eq!(ctx.render(), "");
    assert_eq!(LayeredContext::default().render(), "");
}

#[test]
fn bytes_cuts_on_a_character_boundary() {
    assert_eq!(normalize::bytes("hello", 10), "hello");
    assert_eq!(normalize::bytes("hello", 3), "hel");
    assert_eq!(normalize::bytes("\u{20ac}\u{20ac}", 4), "\u{20ac}");
    assert_eq!(normalize::bytes("\u{20ac}", 2), "");
    assert_eq!(normalize::bytes("abc", 0), "");
    assert_eq!(
        normalize::chars("\u{20ac}\u{20ac}\u{20ac}", 2),
        "\u{20ac}\u{20ac}"
    );
}
