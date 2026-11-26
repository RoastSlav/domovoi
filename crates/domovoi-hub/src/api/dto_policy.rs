//! Wire shapes of the policy editor's versions, the draft, previews and publication.

use domovoi_core::policy_store::{AckRecord, PolicyVersionRecord};
use domovoi_engine::policy::Rule;
use domovoi_engine::policy_shadow::LockChange;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub use super::dto_policy_rule::RuleDto;
use crate::services::{Preview, RuleIssue};

/// The header of one policy version or draft.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct VersionSummaryDto {
    /// The version number.
    pub number: u32,
    /// When it was published, RFC 3339 UTC.
    pub published_at: String,
    /// Who published it.
    pub published_by: String,
    /// Rules in the version.
    pub rule_count: u32,
    /// Locked prohibitions among them.
    pub locked_count: u32,
    /// Shadow rules among them.
    pub shadow_count: u32,
    /// Hash of the rules.
    pub rules_hash: String,
}

impl From<&PolicyVersionRecord> for VersionSummaryDto {
    fn from(record: &PolicyVersionRecord) -> Self {
        Self {
            number: record.number.unwrap_or(0),
            published_at: record
                .published_at
                .map(|at| at.to_string())
                .unwrap_or_default(),
            published_by: record
                .published_by
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default(),
            rule_count: record.rule_count,
            locked_count: record.locked_count,
            shadow_count: record.shadow_count,
            rules_hash: record.rules_hash.clone(),
        }
    }
}

/// The header of the draft.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct DraftSummaryDto {
    /// The draft's revision; echo it to replace or discard the draft.
    pub revision: u64,
    /// The version number the draft was based on, when one was active.
    pub based_on: Option<u32>,
    /// When it was last saved.
    pub updated_at: String,
    /// Who saved it.
    pub updated_by: String,
    /// Rules in the draft.
    pub rule_count: u32,
    /// Locked prohibitions among them.
    pub locked_count: u32,
    /// Shadow rules among them.
    pub shadow_count: u32,
}

impl From<&PolicyVersionRecord> for DraftSummaryDto {
    fn from(record: &PolicyVersionRecord) -> Self {
        Self {
            revision: record.revision,
            based_on: record.based_on,
            updated_at: record.updated_at.to_string(),
            updated_by: record.updated_by.to_string(),
            rule_count: record.rule_count,
            locked_count: record.locked_count,
            shadow_count: record.shadow_count,
        }
    }
}

/// A page of published versions, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct VersionPageDto {
    /// The versions.
    pub items: Vec<VersionSummaryDto>,
    /// Pass as `cursor` for the next page; absent on the last.
    pub next_cursor: Option<String>,
}

/// One acknowledged publication finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct AckDto {
    /// The finding's stable key.
    pub finding_key: String,
    /// The finding's kind.
    pub kind: String,
    /// The first rule involved.
    pub first_rule: String,
    /// The second rule involved.
    pub second_rule: String,
    /// What the pair resolves to.
    pub resolution: Option<String>,
    /// What the review said.
    pub note: String,
    /// Who acknowledged it.
    pub acknowledged_by: String,
    /// When, RFC 3339 UTC.
    pub acknowledged_at: String,
}

impl From<&AckRecord> for AckDto {
    fn from(ack: &AckRecord) -> Self {
        Self {
            finding_key: ack.finding_key.clone(),
            kind: ack.kind.clone(),
            first_rule: ack.first_rule.clone(),
            second_rule: ack.second_rule.clone(),
            resolution: ack.resolution.clone(),
            note: ack.note.clone(),
            acknowledged_by: ack.acknowledged_by.to_string(),
            acknowledged_at: ack.acknowledged_at.to_string(),
        }
    }
}

/// One published version with its rules and acknowledgements.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct VersionDetailDto {
    /// The version's header.
    pub summary: VersionSummaryDto,
    /// Its rules.
    pub rules: Vec<RuleDto>,
    /// The findings acknowledged when it was published.
    pub acknowledgements: Vec<AckDto>,
}

/// The draft with its rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct DraftDetailDto {
    /// The draft's revision.
    pub revision: u64,
    /// The version number the draft was based on.
    pub based_on: Option<u32>,
    /// When it was last saved.
    pub updated_at: String,
    /// Who saved it.
    pub updated_by: String,
    /// Hash of the rules.
    pub rules_hash: String,
    /// The rules.
    pub rules: Vec<RuleDto>,
}

impl DraftDetailDto {
    /// The draft `record` with `rules`.
    #[must_use]
    pub fn new(record: &PolicyVersionRecord, rules: &[Rule]) -> Self {
        Self {
            revision: record.revision,
            based_on: record.based_on,
            updated_at: record.updated_at.to_string(),
            updated_by: record.updated_by.to_string(),
            rules_hash: record.rules_hash.clone(),
            rules: rules.iter().map(RuleDto::from).collect(),
        }
    }
}

/// One locked prohibition that differs from the version it replaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct LockChangeDto {
    /// The rule.
    pub rule: String,
    /// `created`, `edited`, `unlocked` or `removed`.
    pub change: String,
}

impl From<&LockChange> for LockChangeDto {
    fn from(change: &LockChange) -> Self {
        Self {
            rule: change.rule.to_string(),
            change: change.change.as_str().to_owned(),
        }
    }
}

/// Replace the draft, or create it.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct SaveDraftRequest {
    /// `null` creates the draft; a revision replaces that revision.
    #[serde(default)]
    pub base_revision: Option<u64>,
    /// The complete rule set.
    pub rules: Vec<RuleDto>,
    /// The admin's password; needed only when a locked prohibition changes.
    #[serde(default)]
    pub password: Option<String>,
    /// The `preview_hash` of exactly these rules; needed only when a locked prohibition changes.
    #[serde(default)]
    pub preview_hash: Option<String>,
}

/// The saved draft.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct DraftSavedDto {
    /// The draft as stored.
    pub draft: DraftDetailDto,
    /// Locked prohibitions the save touched.
    pub locked_changes: Vec<LockChangeDto>,
}

/// Preview a candidate rule set.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct PreviewRequest {
    /// What the candidate is measured against, which decides its `preview_hash`. `null` measures
    /// against the active version: the preview to publish. A draft revision measures against that
    /// draft: the preview to save over it. The two hashes differ, so publish needs the `null` one.
    #[serde(default)]
    pub base_revision: Option<u64>,
    /// The candidate rules.
    pub rules: Vec<RuleDto>,
}

/// One problem that stops the rules from being saved or published.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ValidationDto {
    /// The rule it is about, if any.
    pub rule: Option<String>,
    /// What is wrong.
    pub message: String,
}

impl From<&RuleIssue> for ValidationDto {
    fn from(issue: &RuleIssue) -> Self {
        Self {
            rule: issue.rule.clone(),
            message: issue.message.clone(),
        }
    }
}

/// One finding of the publication review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct FindingDto {
    /// The stable key to acknowledge.
    pub key: String,
    /// `equal_scope_conflict`, `incomparable_conflict`, `ineffective_under_lock` or
    /// `target_outside_every_ceiling`.
    pub kind: String,
    /// The first rule involved.
    pub first: String,
    /// The second rule involved.
    pub second: String,
    /// What the pair resolves to.
    pub resolution: Option<String>,
    /// Whether either rule is a shadow rule.
    pub shadow: bool,
    /// What the review says.
    pub note: String,
    /// Whether publishing must wait for the owner to acknowledge it.
    pub requires_acknowledgement: bool,
}

/// The publication review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PolicyReportDto {
    /// Every finding, worst first.
    pub findings: Vec<FindingDto>,
    /// Whether any finding blocks publication until acknowledged.
    pub blocked: bool,
}

/// What a candidate rule set would do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PreviewDto {
    /// The active version number, 0 when none.
    pub base_number: u32,
    /// Hash of the candidate rules.
    pub rules_hash: String,
    /// Echo this to save a locked change or to publish.
    pub preview_hash: String,
    /// Problems that stop the rules; when any, `report` is null.
    pub validation: Vec<ValidationDto>,
    /// The publication review.
    pub report: Option<PolicyReportDto>,
    /// The finding keys the owner must acknowledge.
    pub blocking: Vec<String>,
    /// Locked prohibitions the change touches.
    pub locked_changes: Vec<LockChangeDto>,
    /// What the change would do to recent recorded decisions.
    pub simulation: Option<super::dto_policy_trace::SimulationSummaryDto>,
    /// Standing notes.
    pub notes: Vec<String>,
}

impl From<&Preview> for PreviewDto {
    fn from(preview: &Preview) -> Self {
        Self {
            base_number: preview.base_number,
            rules_hash: preview.rules_hash.clone(),
            preview_hash: preview.preview_hash.clone(),
            validation: preview.validation.iter().map(ValidationDto::from).collect(),
            report: preview.report.as_ref().map(|report| PolicyReportDto {
                blocked: report.is_blocked(),
                findings: report
                    .findings
                    .iter()
                    .map(|finding| {
                        let key = domovoi_engine::policy_shadow::finding_key(finding);
                        FindingDto {
                            kind: key.split(':').next().unwrap_or_default().to_owned(),
                            key,
                            first: finding.first.to_string(),
                            second: finding.second.to_string(),
                            resolution: finding.resolution.map(|o| o.name().to_owned()),
                            shadow: finding.shadow,
                            note: finding.note.clone(),
                            requires_acknowledgement: finding.requires_acknowledgement,
                        }
                    })
                    .collect(),
            }),
            blocking: preview.blocking.clone(),
            locked_changes: preview
                .locked_changes
                .iter()
                .map(LockChangeDto::from)
                .collect(),
            simulation: preview.simulation.as_ref().map(Into::into),
            notes: preview.notes.iter().map(|n| (*n).to_owned()).collect(),
        }
    }
}

/// Publish the draft.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct PublishRequest {
    /// The draft revision to publish.
    pub revision: u64,
    /// The `preview_hash` of the draft's rules against the active version.
    pub preview_hash: String,
    /// Exactly the keys of the blocking findings.
    #[serde(default)]
    pub acknowledged: Vec<String>,
    /// The admin's password. Always needed.
    #[serde(default)]
    pub password: Option<String>,
}

/// The version that was published.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PublishedDto {
    /// The new version.
    pub version: VersionSummaryDto,
    /// Locked prohibitions that differ from the version it replaces.
    pub locked_changes: Vec<LockChangeDto>,
    /// The findings acknowledged.
    pub acknowledged: Vec<String>,
}
