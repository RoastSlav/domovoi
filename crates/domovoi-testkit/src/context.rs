//! A scripted context source, so a caller of the context port can be tested without a store or
//! an agent.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use domovoi_core::context::{
    ContextBatch, ContextGap, ContextItem, ContextItemId, ContextLayer, ContextQuery,
    ContextSelector, ContextSourceId, ItemStatus, Origin, Provenance,
};
use domovoi_core::ports::{Clock, ContextSource, Redactor};
use jiff::SignedDuration;

use crate::clock::FakeClock;
use crate::lock;

/// Which kind of selector a script answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SelectorKind {
    /// [`ContextSelector::Overview`].
    Overview,
    /// [`ContextSelector::Search`].
    Search,
    /// [`ContextSelector::Read`].
    Read,
}

impl SelectorKind {
    /// The kind of `selector`.
    #[must_use]
    pub fn of(selector: &ContextSelector) -> Self {
        match selector {
            ContextSelector::Overview => Self::Overview,
            ContextSelector::Search { .. } => Self::Search,
            ContextSelector::Read { .. } => Self::Read,
        }
    }
}

/// What the fake answers next for one selector kind.
#[derive(Debug, Clone)]
enum Script {
    /// Items whose bodies are this text, one per entry.
    Items(Vec<String>),
    /// One gap with this reason.
    Gap(String),
}

/// A context source that answers scripted text per selector kind, records every query, and
/// keeps the port's obligations the way a real source must: it redacts, bounds and identifies
/// every item, and answers a gap past the deadline.
pub struct FakeContextSource {
    id: ContextSourceId,
    layer: ContextLayer,
    redactor: Arc<dyn Redactor>,
    clock: Arc<FakeClock>,
    scripts: Mutex<BTreeMap<SelectorKind, Script>>,
    queries: Mutex<Vec<ContextQuery>>,
    sleep: Mutex<Option<SignedDuration>>,
}

impl FakeContextSource {
    /// A source named `id` in `layer` that answers nothing until scripted.
    #[must_use]
    pub fn new(
        id: ContextSourceId,
        layer: ContextLayer,
        redactor: Arc<dyn Redactor>,
        clock: Arc<FakeClock>,
    ) -> Self {
        Self {
            id,
            layer,
            redactor,
            clock,
            scripts: Mutex::new(BTreeMap::new()),
            queries: Mutex::new(Vec::new()),
            sleep: Mutex::new(None),
        }
    }

    /// Makes `kind` answer one item per entry of `bodies`.
    pub fn answer(&self, kind: SelectorKind, bodies: Vec<String>) {
        lock(&self.scripts).insert(kind, Script::Items(bodies));
    }

    /// Makes `kind` answer with one gap carrying `reason`, as a source that cannot answer.
    pub fn fail(&self, kind: SelectorKind, reason: &str) {
        lock(&self.scripts).insert(kind, Script::Gap(reason.to_owned()));
    }

    /// Makes every later fetch take `duration` on the fake clock before it answers.
    pub fn sleep(&self, duration: SignedDuration) {
        *lock(&self.sleep) = Some(duration);
    }

    /// Every query asked so far, oldest first.
    #[must_use]
    pub fn queries(&self) -> Vec<ContextQuery> {
        lock(&self.queries).clone()
    }

    fn gap(&self, reason: &str) -> ContextBatch {
        ContextBatch {
            items: Vec::new(),
            gaps: vec![ContextGap {
                source: self.id.clone(),
                reason: self.redactor.redact(reason).chars().take(200).collect(),
            }],
        }
    }

    fn item(&self, query: &ContextQuery, index: usize, text: &str) -> ContextItem {
        let body = self.redactor.redact(text);
        let max = query.max_item_bytes as usize;
        let truncated = body.len() > max;
        let mut end = body.len().min(max);
        while !body.is_char_boundary(end) {
            end -= 1;
        }
        ContextItem {
            id: ContextItemId::new(&self.id, &format!("item{index}")),
            source: self.id.clone(),
            layer: self.layer,
            title: format!("scripted item {index}"),
            body: body[..end].to_owned(),
            status: ItemStatus::Known,
            provenance: Provenance {
                origin: Origin::Observed,
                reference: format!("fake:{index}"),
                observed_at: Some(self.clock.now()),
                actor: None,
            },
            truncated,
        }
    }
}

#[async_trait]
impl ContextSource for FakeContextSource {
    fn id(&self) -> ContextSourceId {
        self.id.clone()
    }

    fn layer(&self) -> ContextLayer {
        self.layer
    }

    fn supports(&self, selector: &ContextSelector) -> bool {
        lock(&self.scripts).contains_key(&SelectorKind::of(selector))
    }

    async fn fetch(&self, query: &ContextQuery) -> ContextBatch {
        lock(&self.queries).push(query.clone());
        let sleep = *lock(&self.sleep);
        if let Some(duration) = sleep {
            self.clock.advance(duration);
        }
        if self.clock.now() >= query.deadline {
            return self.gap("timed out");
        }
        let script = lock(&self.scripts)
            .get(&SelectorKind::of(&query.selector))
            .cloned();
        match script {
            None => self.gap("this source does not answer that selector"),
            Some(Script::Gap(reason)) => self.gap(&reason),
            Some(Script::Items(bodies)) => ContextBatch {
                items: bodies
                    .iter()
                    .take(usize::from(query.limit))
                    .enumerate()
                    .map(|(index, text)| self.item(query, index, text))
                    .collect(),
                gaps: Vec::new(),
            },
        }
    }
}
