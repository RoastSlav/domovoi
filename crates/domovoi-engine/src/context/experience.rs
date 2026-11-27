//! The experience layer of model context: what finished operations on a host taught.
//!
//! An item is historical evidence, never permission: it names the operation, the day, whether
//! the fix was verified and where each narrative field came from, and it always renders the
//! origin `historical evidence, not authorization`. The source only reads.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::context::{
    ContextBatch, ContextGap, ContextItem, ContextItemId, ContextLayer, ContextQuery, ContextScope,
    ContextSelector, ContextSourceId, ItemStatus, Origin, Provenance,
};
use domovoi_core::experience::{
    ExperienceQuery, ExperienceScope, NarrativeField, ResolutionSummary, SimilarQuery,
    SummarySource,
};
use domovoi_core::ports::{
    Clock, ContextSource, ExperienceRead, HostRepo, IncidentRepo, OperationRepo, Redactor,
};
use domovoi_core::{HostId, OperationId};
use jiff::Timestamp;

/// The longest body of one item, in bytes, whatever the query allows.
pub const ITEM_BODY_MAX: usize = 1024;
/// Most items an overview returns, so it fits the small experience layer.
const OVERVIEW_ITEMS: u16 = 3;
/// Most items a search returns.
const SEARCH_ITEMS: u16 = 5;
/// Longest title, in characters.
const TITLE_CHARS: usize = 120;
/// Longest narrative text in an item, in characters.
const NARRATIVE_CHARS: usize = 240;
/// Longest list of contracts that ran, in characters.
const RAN_CHARS: usize = 200;
/// Longest gap reason, in characters.
const REASON_CHARS: usize = 200;

/// What the experience source reads. Read ports only.
#[derive(Clone)]
pub struct ExperienceDeps {
    /// Summaries of finished operations.
    pub experience: Arc<dyn ExperienceRead>,
    /// Incidents, to build an overview from one.
    pub incidents: Arc<dyn IncidentRepo>,
    /// Operations, to build an overview from one.
    pub operations: Arc<dyn OperationRepo>,
    /// Host names, for a search over every host.
    pub hosts: Arc<dyn HostRepo>,
    /// Runs on every string before it leaves.
    pub redactor: Arc<dyn Redactor>,
    /// Decides the query deadline.
    pub clock: Arc<dyn Clock>,
}

/// Context from past operations.
#[derive(Clone)]
pub struct ExperienceSource {
    deps: Arc<ExperienceDeps>,
}

impl ExperienceSource {
    /// A source over the summaries of finished operations.
    #[must_use]
    pub fn new(deps: ExperienceDeps) -> Self {
        Self {
            deps: Arc::new(deps),
        }
    }

    /// A source over one store that serves every read port it needs.
    #[must_use]
    pub fn over<S>(store: Arc<S>, redactor: Arc<dyn Redactor>, clock: Arc<dyn Clock>) -> Self
    where
        S: ExperienceRead + IncidentRepo + OperationRepo + HostRepo + 'static,
    {
        Self::new(ExperienceDeps {
            experience: store.clone(),
            incidents: store.clone(),
            operations: store.clone(),
            hosts: store,
            redactor,
            clock,
        })
    }

    /// The read port over operations and their executions, for lookups that are not context.
    #[must_use]
    pub fn operations(&self) -> &Arc<dyn OperationRepo> {
        &self.deps.operations
    }

    fn id() -> ContextSourceId {
        ContextSourceId::parse("experience").unwrap_or_else(|_| ContextSourceId::evidence())
    }

    fn gap(&self, reason: &str) -> ContextBatch {
        ContextBatch {
            items: Vec::new(),
            gaps: vec![ContextGap {
                source: Self::id(),
                reason: self.cut(reason, REASON_CHARS),
            }],
        }
    }

    fn cut(&self, text: &str, max_chars: usize) -> String {
        self.deps
            .redactor
            .redact(text)
            .chars()
            .take(max_chars)
            .collect()
    }

    /// A search over every host, for the owner's own global conversation only. It is not part
    /// of the [`ContextSource`] port, so no model argument and no bundle can reach it.
    pub async fn global_search(
        &self,
        text: &str,
        limit: u16,
        max_item_bytes: u32,
        deadline: Timestamp,
    ) -> ContextBatch {
        if self.deps.clock.now() >= deadline {
            return self.gap("timed out");
        }
        let query = ExperienceQuery {
            scope: ExperienceScope::Global,
            text: text.to_owned(),
            limit: limit.min(SEARCH_ITEMS),
            exclude: None,
        };
        let hits = match self.deps.experience.search(&query).await {
            Ok(hits) => hits,
            Err(_) => return self.gap("could not read past operations"),
        };
        let names: BTreeMap<HostId, String> = match self.deps.hosts.list_hosts().await {
            Ok(hosts) => hosts
                .into_iter()
                .map(|host| (host.id, host.name.to_string()))
                .collect(),
            Err(_) => BTreeMap::new(),
        };
        let items = hits
            .iter()
            .map(|hit| self.item_of(&hit.summary, max_item_bytes, Some(&names)))
            .collect();
        ContextBatch {
            items,
            gaps: Vec::new(),
        }
    }

    async fn search_host(
        &self,
        host: HostId,
        text: &str,
        limit: u16,
        exclude: Option<OperationId>,
        max_bytes: u32,
    ) -> ContextBatch {
        let query = ExperienceQuery {
            scope: ExperienceScope::Host(host),
            text: text.to_owned(),
            limit,
            exclude,
        };
        match self.deps.experience.search(&query).await {
            Ok(hits) => self.batch(hits.iter().map(|hit| &hit.summary), max_bytes),
            Err(_) => self.gap("could not read past operations"),
        }
    }

    async fn similar(&self, query: SimilarQuery, max_bytes: u32) -> ContextBatch {
        match self.deps.experience.similar(&query).await {
            Ok(hits) => self.batch(hits.iter().map(|hit| &hit.summary), max_bytes),
            Err(_) => self.gap("could not read past operations"),
        }
    }

    fn batch<'a>(
        &self,
        summaries: impl Iterator<Item = &'a ResolutionSummary>,
        max_bytes: u32,
    ) -> ContextBatch {
        ContextBatch {
            items: summaries
                .map(|summary| self.item_of(summary, max_bytes, None))
                .collect(),
            gaps: Vec::new(),
        }
    }

    async fn incident_overview(
        &self,
        host: HostId,
        incident: domovoi_core::IncidentId,
        limit: u16,
        max_bytes: u32,
    ) -> ContextBatch {
        let found = match self.deps.incidents.get(incident).await {
            Ok(Some(found)) => found,
            Ok(None) => return self.gap("incident not found"),
            Err(_) => return self.gap("could not read the incident"),
        };
        if found.host() != host {
            return self.gap("scope does not match the incident");
        }
        let query = SimilarQuery {
            host,
            check: Some(found.check().clone()),
            target: found.target().cloned(),
            text: found.title().to_owned(),
            contracts: Vec::new(),
            exclude: None,
            limit,
        };
        self.similar(query, max_bytes).await
    }

    async fn operation_overview(
        &self,
        host: HostId,
        operation: OperationId,
        limit: u16,
        max_bytes: u32,
    ) -> ContextBatch {
        let found = match self.deps.operations.get(operation).await {
            Ok(Some(found)) => found.value,
            Ok(None) => return self.gap("operation not found"),
            Err(_) => return self.gap("could not read the operation"),
        };
        if !found.pinned.iter().any(|subject| subject.host == host) {
            return self.gap("scope does not match the operation");
        }
        let first = found.pinned.first();
        let query = SimilarQuery {
            host,
            check: first.map(|subject| subject.check.clone()),
            target: first.and_then(|subject| subject.target.clone()),
            text: found.goal.clone(),
            contracts: Vec::new(),
            exclude: Some(operation),
            limit,
        };
        self.similar(query, max_bytes).await
    }

    /// One item for a finished operation, redacted and cut to `max_bytes` (never above
    /// [`ITEM_BODY_MAX`]). `host_names` is set only by a search over every host.
    fn item_of(
        &self,
        summary: &ResolutionSummary,
        max_bytes: u32,
        host_names: Option<&BTreeMap<HostId, String>>,
    ) -> ContextItem {
        let source = Self::id();
        let key = format!("op-{}", summary.operation);
        let limit = (max_bytes as usize).min(ITEM_BODY_MAX);
        let body = self.deps.redactor.redact(&body_of(summary, host_names));
        let mut end = body.len().min(limit);
        while !body.is_char_boundary(end) {
            end -= 1;
        }
        ContextItem {
            id: ContextItemId::new(&source, &key),
            source,
            layer: ContextLayer::Experience,
            title: self.cut(&title_of(summary), TITLE_CHARS),
            body: body[..end].to_owned(),
            status: ItemStatus::Known,
            provenance: Provenance {
                origin: Origin::Historical,
                reference: format!("operation:{}", summary.operation),
                observed_at: Some(summary.ended_at),
                actor: None,
            },
            truncated: end < body.len(),
        }
    }
}

fn title_of(summary: &ResolutionSummary) -> String {
    let verified = if summary.verified { ", verified" } else { "" };
    match summary.subjects.first() {
        Some(subject) => {
            let target = subject
                .target
                .as_ref()
                .map(|t| format!(" {}", t.as_str()))
                .unwrap_or_default();
            format!(
                "{}{target} - {}{verified}",
                subject.check.as_str(),
                summary.outcome.as_str()
            )
        }
        None => {
            let goal: String = summary.goal.chars().take(60).collect();
            format!("{goal} - {}{verified}", summary.outcome.as_str())
        }
    }
}

fn narrative_line(label: &str, field: Option<&NarrativeField>) -> Option<String> {
    let field = field?;
    let text: String = field.text.chars().take(NARRATIVE_CHARS).collect();
    // A model's narrative is a draft from that day, never a finding about the host now.
    let source = match field.source {
        SummarySource::Model => "old model draft, not evidence about now",
        _ => field.source.as_str(),
    };
    Some(format!("{label} [{source}]: {text}"))
}

fn body_of(summary: &ResolutionSummary, host_names: Option<&BTreeMap<HostId, String>>) -> String {
    let mut lines = vec![
        format!("operation: {}", summary.operation),
        format!("ended: {}", summary.ended_at),
    ];
    if let Some(names) = host_names {
        let hosts: Vec<String> = summary
            .hosts
            .iter()
            .map(|id| names.get(id).cloned().unwrap_or_else(|| id.to_string()))
            .collect();
        lines.push(format!("host: {}", hosts.join(", ")));
    }
    lines.push(format!("outcome: {}", summary.outcome.as_str()));
    lines.push(format!(
        "verified: {}",
        if summary.verified { "yes" } else { "no" }
    ));
    if !summary.contracts.is_empty() {
        let ran: Vec<String> = summary
            .contracts
            .iter()
            .map(|run| {
                let target = run
                    .target
                    .as_ref()
                    .map(|t| format!(" {t}"))
                    .unwrap_or_default();
                format!("{}{target} ({})", run.contract, run.outcome)
            })
            .collect();
        let ran: String = ran.join("; ").chars().take(RAN_CHARS).collect();
        lines.push(format!("ran: {ran}"));
    }
    lines.extend(narrative_line("problem", summary.problem.as_ref()));
    lines.extend(narrative_line("cause", summary.cause.as_ref()));
    lines.extend(narrative_line("fix", summary.fix.as_ref()));
    lines.join("\n")
}

#[async_trait]
impl ContextSource for ExperienceSource {
    fn id(&self) -> ContextSourceId {
        Self::id()
    }

    fn layer(&self) -> ContextLayer {
        ContextLayer::Experience
    }

    fn supports(&self, selector: &ContextSelector) -> bool {
        matches!(
            selector,
            ContextSelector::Overview | ContextSelector::Search { .. }
        )
    }

    async fn fetch(&self, query: &ContextQuery) -> ContextBatch {
        if self.deps.clock.now() >= query.deadline {
            return self.gap("timed out");
        }
        let bytes = query.max_item_bytes;
        match (&query.scope, &query.selector) {
            (_, ContextSelector::Read { .. }) => self.gap("this source cannot be read exactly"),
            (
                ContextScope::Host(host) | ContextScope::Incident { host, .. },
                ContextSelector::Search { text },
            ) => {
                let limit = query.limit.min(SEARCH_ITEMS);
                self.search_host(*host, text, limit, None, bytes).await
            }
            (ContextScope::Operation { host, operation }, ContextSelector::Search { text }) => {
                let limit = query.limit.min(SEARCH_ITEMS);
                self.search_host(*host, text, limit, Some(*operation), bytes)
                    .await
            }
            (ContextScope::Host(host), ContextSelector::Overview) => {
                let limit = query.limit.min(OVERVIEW_ITEMS);
                self.search_host(*host, "", limit, None, bytes).await
            }
            (ContextScope::Incident { host, incident }, ContextSelector::Overview) => {
                let limit = query.limit.min(OVERVIEW_ITEMS);
                self.incident_overview(*host, *incident, limit, bytes).await
            }
            (ContextScope::Operation { host, operation }, ContextSelector::Overview) => {
                let limit = query.limit.min(OVERVIEW_ITEMS);
                self.operation_overview(*host, *operation, limit, bytes)
                    .await
            }
        }
    }
}
