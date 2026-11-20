//! What the API returns for past operations: the summary of one operation with its history,
//! similar operations, and the owner's search. Core types are mapped here and never exposed
//! directly. Every body that carries past operations also carries the label that says they are
//! evidence only.

use std::collections::BTreeMap;

use domovoi_core::HostId;
use domovoi_core::experience::{
    ExecutedContract, ExperienceHit, HISTORICAL_LABEL, NarrativeField, ResolutionBrief,
    ResolutionSummary, SimilarHit, SimilarReason, SummaryField, SummaryRevision,
};
use domovoi_core::ports::Redactor;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

/// The most revisions a summary response lists.
pub const HISTORY_LIMIT: usize = 200;

/// A host as a person knows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct HostRefDto {
    /// Host id.
    pub id: String,
    /// The host's name; `null` when the host was deleted.
    pub name: Option<String>,
}

/// The current revision of one summary field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct SummaryFieldDto {
    /// The field name, for example `problem`.
    pub field: String,
    /// The canonical JSON of the revision: a string, a boolean or an array.
    pub value: serde_json::Value,
    /// Who wrote it: `code`, `model` or `user`.
    pub source: String,
    /// The actor, for example `system:operations`.
    pub actor: String,
    /// The revision number, from 1.
    pub revision: u32,
    /// When it was written.
    pub created_at: String,
    /// Whether the owner can edit it: a narrative field of a finished operation.
    pub editable: bool,
}

/// One revision in a field's history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct SummaryRevisionDto {
    /// The field name.
    pub field: String,
    /// The revision number, from 1.
    pub revision: u32,
    /// Who wrote it: `code`, `model` or `user`.
    pub source: String,
    /// The actor.
    pub actor: String,
    /// When it was written.
    pub created_at: String,
    /// The value of this revision.
    pub value: serde_json::Value,
}

/// The summary of one operation with where each field came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ResolutionSummaryDto {
    /// Operation id.
    pub operation_id: String,
    /// The journal position read before the summary; apply only events with a greater `seq`.
    pub as_of_seq: u64,
    /// `not_ready` while the operation is open, `drafting` once it has finished and has no
    /// summary yet, `ready` after.
    pub state: String,
    /// When the operation ended.
    pub ended_at: Option<String>,
    /// The current revision of each field that has one.
    pub fields: Vec<SummaryFieldDto>,
    /// Every revision, oldest first; the newest 200 when there are more.
    pub history: Vec<SummaryRevisionDto>,
    /// Whether older revisions were left out of `history`.
    pub history_truncated: bool,
    /// Always `historical evidence, not authorization`.
    pub label: String,
}

/// What to write into a narrative field.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct EditSummaryFieldRequest {
    /// The new text, 1 to 1000 characters. The same text as a model draft confirms it.
    pub value: String,
    /// The field's newest revision when it was opened; 0 when it had none.
    pub expected_revision: u32,
}

/// The result of an edit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct EditSummaryResponse {
    /// The revision that now holds the text.
    pub field: SummaryFieldDto,
    /// Whether the text was already the owner's, so nothing was added.
    pub unchanged: bool,
}

/// A capability run that was handed to an agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ExecutedContractDto {
    /// The contract, for example `skill.restart_container`.
    pub contract: String,
    /// The host it ran on.
    pub host_id: String,
    /// The target, if any.
    pub target: Option<String>,
    /// `succeeded`, `failed`, `outcome_unknown` or `running`.
    pub outcome: String,
}

/// One finished operation, the shape every list of past operations shares.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PastOperationDto {
    /// Operation id.
    pub operation_id: String,
    /// What it was for.
    pub goal: String,
    /// When it ended.
    pub ended_at: String,
    /// `succeeded`, `failed`, `escalated` or `cancelled`.
    pub outcome: String,
    /// Whether the fix was verified.
    pub verified: bool,
    /// The hosts involved.
    pub hosts: Vec<HostRefDto>,
    /// What was run.
    pub contracts: Vec<ExecutedContractDto>,
    /// `problem`, `cause` and `fix`: only those that exist, each with its source.
    pub narrative: Vec<SummaryFieldDto>,
}

/// Why a past operation was judged similar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct SimilarReasonDto {
    /// `same_check`, `same_target`, `same_contract` or `similar_words`.
    pub code: String,
    /// The contract id for `same_contract`.
    pub detail: Option<String>,
}

/// A similar past operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct SimilarOperationDto {
    /// The operation.
    pub operation: PastOperationDto,
    /// The score; higher is more similar.
    pub score: u32,
    /// Why it scored.
    pub reasons: Vec<SimilarReasonDto>,
}

/// Similar past operations, best first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct SimilarOperationsDto {
    /// The matches.
    pub items: Vec<SimilarOperationDto>,
    /// The journal position read before the list.
    pub as_of_seq: u64,
    /// Always `historical evidence, not authorization`.
    pub label: String,
}

/// One search hit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ExperienceItemDto {
    /// The operation.
    pub operation: PastOperationDto,
    /// `all_words` or `any_word`.
    pub match_pass: String,
    /// A short excerpt around the match.
    pub snippet: Option<String>,
}

/// The result of the owner's search over past operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ExperienceSearchDto {
    /// `host` or `global`.
    pub scope: String,
    /// The host searched, for scope `host`.
    pub host: Option<HostRefDto>,
    /// The search text as given.
    pub query: String,
    /// The matches; the newest operations when the query had no words.
    pub items: Vec<ExperienceItemDto>,
    /// The journal position read before the search.
    pub as_of_seq: u64,
    /// Always `historical evidence, not authorization`.
    pub label: String,
}

/// The outcome of a finished operation, for a list row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ResolutionBriefDto {
    /// `succeeded`, `failed`, `escalated` or `cancelled`.
    pub outcome: String,
    /// Whether the fix was verified.
    pub verified: bool,
    /// When it ended.
    pub ended_at: String,
}

impl From<&ResolutionBrief> for ResolutionBriefDto {
    fn from(brief: &ResolutionBrief) -> Self {
        Self {
            outcome: brief.outcome.as_str().to_owned(),
            verified: brief.verified,
            ended_at: brief.ended_at.to_string(),
        }
    }
}

/// Filters of the similar-operations lists.
#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SimilarQueryParams {
    /// How many to return, 1 to 20. Default 5.
    pub limit: Option<u16>,
}

/// Filters of the owner's search.
#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SearchQueryParams {
    /// What to look for, at most 200 characters; empty lists the newest operations.
    pub q: Option<String>,
    /// `host` (default) or `global`.
    pub scope: Option<String>,
    /// The host's name, required for scope `host`.
    pub host: Option<String>,
    /// How many to return, 1 to 50. Default 10.
    pub limit: Option<u16>,
}

fn revision_value(redactor: &dyn Redactor, value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::String(text) => serde_json::Value::String(redactor.redact(text)),
        other => match serde_json::to_string(other) {
            Ok(text) => {
                let clean = redactor.redact(&text);
                serde_json::from_str(&clean).unwrap_or_else(|_| other.clone())
            }
            Err(_) => other.clone(),
        },
    }
}

impl SummaryFieldDto {
    /// The current revision, redacted again on the way out.
    #[must_use]
    pub fn of(revision: &SummaryRevision, terminal: bool, redactor: &dyn Redactor) -> Self {
        Self {
            field: revision.field.as_str().to_owned(),
            value: revision_value(redactor, &revision.value),
            source: revision.source.as_str().to_owned(),
            actor: revision.actor.clone(),
            revision: revision.revision,
            created_at: revision.created_at.to_string(),
            editable: terminal && revision.field.is_narrative(),
        }
    }

    fn narrative(field: SummaryField, value: &NarrativeField, redactor: &dyn Redactor) -> Self {
        Self {
            field: field.as_str().to_owned(),
            value: serde_json::Value::String(redactor.redact(&value.text)),
            source: value.source.as_str().to_owned(),
            actor: value.actor.clone(),
            revision: value.revision,
            created_at: value.at.to_string(),
            editable: true,
        }
    }
}

impl SummaryRevisionDto {
    /// One revision of the history, redacted again on the way out.
    #[must_use]
    pub fn of(revision: &SummaryRevision, redactor: &dyn Redactor) -> Self {
        Self {
            field: revision.field.as_str().to_owned(),
            revision: revision.revision,
            source: revision.source.as_str().to_owned(),
            actor: revision.actor.clone(),
            created_at: revision.created_at.to_string(),
            value: revision_value(redactor, &revision.value),
        }
    }
}

impl ExecutedContractDto {
    fn of(contract: &ExecutedContract, redactor: &dyn Redactor) -> Self {
        Self {
            contract: redactor.redact(&contract.contract),
            host_id: contract.host.to_string(),
            target: contract.target.as_deref().map(|t| redactor.redact(t)),
            outcome: contract.outcome.clone(),
        }
    }
}

impl PastOperationDto {
    /// A past operation with host names from `names`, redacted again on the way out.
    #[must_use]
    pub fn of(
        summary: &ResolutionSummary,
        names: &BTreeMap<HostId, String>,
        redactor: &dyn Redactor,
    ) -> Self {
        let mut narrative = Vec::new();
        for (field, value) in [
            (SummaryField::Problem, &summary.problem),
            (SummaryField::Cause, &summary.cause),
            (SummaryField::Fix, &summary.fix),
        ] {
            if let Some(value) = value {
                narrative.push(SummaryFieldDto::narrative(field, value, redactor));
            }
        }
        Self {
            operation_id: summary.operation.to_string(),
            goal: redactor.redact(&summary.goal),
            ended_at: summary.ended_at.to_string(),
            outcome: summary.outcome.as_str().to_owned(),
            verified: summary.verified,
            hosts: summary
                .hosts
                .iter()
                .map(|id| HostRefDto {
                    id: id.to_string(),
                    name: names.get(id).cloned(),
                })
                .collect(),
            contracts: summary
                .contracts
                .iter()
                .map(|c| ExecutedContractDto::of(c, redactor))
                .collect(),
            narrative,
        }
    }
}

impl SimilarOperationDto {
    /// A similar operation with its reasons.
    #[must_use]
    pub fn of(hit: &SimilarHit, names: &BTreeMap<HostId, String>, redactor: &dyn Redactor) -> Self {
        Self {
            operation: PastOperationDto::of(&hit.summary, names, redactor),
            score: hit.score,
            reasons: hit
                .reasons
                .iter()
                .map(|reason| match reason {
                    SimilarReason::SameCheck => reason_of("same_check", None),
                    SimilarReason::SameTarget => reason_of("same_target", None),
                    SimilarReason::SameContract(id) => reason_of("same_contract", Some(id)),
                    SimilarReason::SimilarWords => reason_of("similar_words", None),
                })
                .collect(),
        }
    }
}

fn reason_of(code: &str, detail: Option<&String>) -> SimilarReasonDto {
    SimilarReasonDto {
        code: code.to_owned(),
        detail: detail.cloned(),
    }
}

impl ExperienceItemDto {
    /// A search hit.
    #[must_use]
    pub fn of(
        hit: &ExperienceHit,
        names: &BTreeMap<HostId, String>,
        redactor: &dyn Redactor,
    ) -> Self {
        Self {
            operation: PastOperationDto::of(&hit.summary, names, redactor),
            match_pass: hit.pass.as_str().to_owned(),
            snippet: hit.snippet.as_deref().map(|s| redactor.redact(s)),
        }
    }
}

/// The label every body of past operations carries.
#[must_use]
pub fn label() -> String {
    HISTORICAL_LABEL.to_owned()
}
