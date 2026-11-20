//! The evidence source: one exact read on a host, as a context item.

use async_trait::async_trait;
use domovoi_core::ReadToolCall;
use domovoi_core::context::{
    ContextBatch, ContextGap, ContextItem, ContextItemId, ContextLayer, ContextQuery, ContextScope,
    ContextSelector, ContextSourceId, ItemStatus, Origin, Provenance,
};
use domovoi_core::ports::ContextSource;
use sha2::{Digest, Sha256};

use crate::triage::TriageDeps;
use crate::triage::tools::{self, EvidenceClass, Scope, cut};

/// Longest title, reference and reason an item or gap carries, in characters.
const TITLE_MAX: usize = 120;
const REFERENCE_MAX: usize = 120;
const REASON_MAX: usize = 200;
/// Longest argument text in a title.
const ARGS_MAX: usize = 80;

/// Answers one exact read on the query's host through the same boundary every read uses.
///
/// A read that ran becomes one `known` item; a read that could not run or did not finish
/// becomes one `unknown` item with the reason; a read that was not allowed as asked becomes a
/// gap. Any selector other than an exact read is answered with a gap.
#[derive(Clone)]
pub struct EvidenceSource {
    deps: TriageDeps,
}

/// One answered read with its class, for callers that must tell unavailable from failed.
pub(crate) struct ClassifiedRead {
    /// What the read came to.
    pub(crate) class: EvidenceClass,
    /// The item or the gap.
    pub(crate) batch: ContextBatch,
}

impl EvidenceSource {
    /// A source over the triage dependencies: the evidence boundary, the store and the
    /// redactor.
    #[must_use]
    pub fn new(deps: TriageDeps) -> Self {
        Self { deps }
    }

    fn source_id() -> ContextSourceId {
        ContextSourceId::evidence()
    }

    fn gap(&self, reason: &str) -> ContextGap {
        ContextGap {
            source: Self::source_id(),
            reason: chars(&self.deps.redactor.redact(reason), REASON_MAX),
        }
    }

    /// Runs `call` for `query` and says what it came to.
    pub(crate) async fn read(&self, query: &ContextQuery, call: &ReadToolCall) -> ClassifiedRead {
        let deps = &self.deps;
        if deps.clock.now() >= query.deadline {
            let reason = "timed out".to_owned();
            return ClassifiedRead {
                class: EvidenceClass::Failed(reason.clone()),
                batch: ContextBatch {
                    items: Vec::new(),
                    gaps: vec![self.gap(&reason)],
                },
            };
        }
        let scope = scope_of(&query.scope);
        let (class, text) = tools::read_redacted(deps, scope, call.clone(), query.deadline).await;
        let tool = call.tool().name();
        let args = args_text(call);
        let id = ContextItemId::new(&Self::source_id(), &format!("{tool}:{}", digest(call)));
        let title = chars(&deps.redactor.redact(&format!("{tool} {args}")), TITLE_MAX);
        let reference = chars(
            &deps.redactor.redact(&format!("{tool} {args}")),
            REFERENCE_MAX,
        );
        let max = query.max_item_bytes as usize;
        let (body, truncated, status) = match &class {
            EvidenceClass::Observed => (cut(&text, max), text.len() > max, ItemStatus::Known),
            EvidenceClass::Unavailable(reason) | EvidenceClass::Failed(reason) => (
                String::new(),
                false,
                ItemStatus::Unknown {
                    reason: chars(reason, REASON_MAX),
                },
            ),
            EvidenceClass::Refused(reason) => {
                let gap = self.gap(reason);
                return ClassifiedRead {
                    class,
                    batch: ContextBatch {
                        items: Vec::new(),
                        gaps: vec![gap],
                    },
                };
            }
        };
        let item = ContextItem {
            id,
            source: Self::source_id(),
            layer: ContextLayer::Evidence,
            title,
            body,
            status,
            provenance: Provenance {
                origin: Origin::Observed,
                reference,
                observed_at: Some(deps.clock.now()),
                actor: None,
            },
            truncated,
        };
        ClassifiedRead {
            class,
            batch: ContextBatch {
                items: vec![item],
                gaps: Vec::new(),
            },
        }
    }
}

#[async_trait]
impl ContextSource for EvidenceSource {
    fn id(&self) -> ContextSourceId {
        Self::source_id()
    }

    fn layer(&self) -> ContextLayer {
        ContextLayer::Evidence
    }

    fn supports(&self, selector: &ContextSelector) -> bool {
        matches!(selector, ContextSelector::Read { .. })
    }

    async fn fetch(&self, query: &ContextQuery) -> ContextBatch {
        match &query.selector {
            ContextSelector::Read { call } => self.read(query, call).await.batch,
            _ => ContextBatch {
                items: Vec::new(),
                gaps: vec![self.gap("this source answers exact reads only")],
            },
        }
    }
}

/// The read scope of a context scope: its host, and the incident to leave out of history.
fn scope_of(scope: &ContextScope) -> Scope {
    match scope {
        ContextScope::Host(host) | ContextScope::Operation { host, .. } => Scope {
            host: *host,
            incident: None,
        },
        ContextScope::Incident { host, incident } => Scope {
            host: *host,
            incident: Some(*incident),
        },
    }
}

/// The call's arguments as compact JSON without its tool name, cut short.
fn args_text(call: &ReadToolCall) -> String {
    let mut value = serde_json::to_value(call).unwrap_or_default();
    if let Some(object) = value.as_object_mut() {
        object.remove("tool");
    }
    chars(&value.to_string(), ARGS_MAX)
}

/// The first eight hex digits of the SHA-256 of the call's canonical JSON.
fn digest(call: &ReadToolCall) -> String {
    let json = serde_json::to_string(call).unwrap_or_default();
    let hash = Sha256::digest(json.as_bytes());
    hash.iter().take(4).map(|b| format!("{b:02x}")).collect()
}

fn chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}
