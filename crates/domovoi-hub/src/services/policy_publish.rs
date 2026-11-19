//! Publishing a draft as the next immutable policy version, and previewing a candidate rule
//! set before either step.

use std::collections::BTreeSet;

use domovoi_core::policy_store::{AckRecord, PolicyVersionRecord, PolicyWrite};
use domovoi_core::ports::{
    AggregateRef, CommitError, JournalDraft, JournalKind, StateWrite, Transaction,
};
use domovoi_engine::policy::Rule;
use domovoi_engine::policy_shadow::{LockChange, finding_key, outcome_text};
use serde_json::json;

use super::Caller;
use super::policy::{Analysis, PolicyService, audit, lock_audit};
use super::policy_types::{
    AUDIT_PUBLISHED, PolicyCommandError, Preview, SIM_PREVIEW_LIMIT, StepUp,
};

/// A version that was just published.
#[derive(Debug, Clone)]
pub struct PublishedVersion {
    /// The version, now numbered and frozen.
    pub record: PolicyVersionRecord,
    /// Locked prohibitions that differ from the version it replaces.
    pub locked_changes: Vec<LockChange>,
    /// The findings the owner acknowledged.
    pub acknowledged: Vec<String>,
}

impl PolicyService {
    /// Publishes draft `revision` as the next version number.
    ///
    /// The password comes first, before anything is read. The echoed `preview_hash` must be the
    /// one the draft's rules preview as against the active version, and `acknowledged` must be
    /// exactly the findings that block, so an acknowledgement always binds to these rules.
    pub async fn publish(
        &self,
        caller: &Caller,
        revision: u64,
        preview_hash: &str,
        acknowledged: &[String],
        step_up: StepUp<'_>,
    ) -> Result<PublishedVersion, PolicyCommandError> {
        self.step_up(caller, step_up).await?;
        let draft = self
            .repo
            .draft()
            .await?
            .ok_or(PolicyCommandError::NoDraft)?;
        if draft.revision != revision {
            return Err(PolicyCommandError::StaleDraft {
                current: draft.revision,
            });
        }
        let rules = self.rules_of(draft.id).await?;
        let base = self.load_base(None).await?;
        if draft.based_on.unwrap_or(0) != base.active_number {
            return Err(PolicyCommandError::StaleActive {
                current: base.active_number,
            });
        }
        let analysis = self.analyse(&rules, &base).await;
        if !analysis.validation.is_empty() {
            return Err(PolicyCommandError::Invalid(analysis.validation));
        }
        if preview_hash != analysis.hash {
            return Err(PolicyCommandError::StalePreview);
        }
        let keys = check_acknowledgements(&analysis, acknowledged)?;
        let (at, actor) = (self.clock.now(), caller.actor());
        let number = base.active_number + 1;
        let acknowledgements: Vec<AckRecord> = analysis
            .report
            .iter()
            .flat_map(|report| report.blocking())
            .map(|finding| AckRecord {
                finding_key: finding_key(finding),
                kind: kind_of(&finding_key(finding)),
                first_rule: finding.first.to_string(),
                second_rule: finding.second.to_string(),
                resolution: finding.resolution.map(|o| outcome_text(o).to_owned()),
                note: finding.note.clone(),
                acknowledged_by: actor.clone(),
                acknowledged_at: at,
            })
            .collect();
        let changes = &analysis.changes;
        let changed: Vec<_> = changes
            .iter()
            .map(|c| json!({ "rule": c.rule.as_str(), "change": c.change.as_str() }))
            .collect();
        let payload = json!({
            "version": number,
            "rules": draft.rule_count,
            "locked": draft.locked_count,
            "shadow": draft.shadow_count,
            "rules_hash": analysis.rules_hash,
            "acknowledged": keys,
            "locked_changes": changed,
        });
        let mut tx = Transaction::new()
            .write(StateWrite::Policy(PolicyWrite::Publish {
                id: draft.id,
                expected_revision: revision,
                number,
                acknowledgements,
                actor: actor.clone(),
                at,
            }))
            .journal(JournalDraft {
                aggregate: AggregateRef::policy(draft.id),
                kind: JournalKind::PolicyPublished,
                payload_v: 1,
                payload,
                actor: actor.clone(),
                occurred_at: at,
            })
            .audit(audit(
                &actor,
                at,
                AUDIT_PUBLISHED,
                draft.id,
                json!({
                    "number": number,
                    "rules": draft.rule_count,
                    "locked": draft.locked_count,
                    "shadow": draft.shadow_count,
                    "acknowledged": keys.len(),
                    "preview_hash": preview_hash.chars().take(8).collect::<String>(),
                }),
            ));
        for change in changes {
            tx = tx.audit(lock_audit(
                &actor,
                at,
                draft.id,
                change,
                "published",
                Some(number),
            ));
        }
        match self.uow.commit(tx).await {
            Ok(_) => {}
            Err(CommitError::Conflict { .. }) => {
                let current = self.repo.active().await?.and_then(|v| v.number);
                return Err(PolicyCommandError::StaleActive {
                    current: current.unwrap_or(0),
                });
            }
            Err(other) => return Err(PolicyCommandError::Commit(other)),
        }
        let record = self
            .repo
            .by_id(draft.id)
            .await?
            .ok_or(PolicyCommandError::NotFound)?;
        Ok(PublishedVersion {
            record,
            locked_changes: analysis.changes,
            acknowledged: keys,
        })
    }

    /// Read-only analysis of a candidate rule set against its base. No password, no write.
    pub async fn preview(
        &self,
        rules: Vec<Rule>,
        base_revision: Option<u64>,
    ) -> Result<Preview, PolicyCommandError> {
        let base = self.load_base(base_revision).await?;
        let analysis = self.analyse(&rules, &base).await;
        let simulation = if analysis.validation.is_empty() {
            Some(self.replay_recent(&rules, SIM_PREVIEW_LIMIT).await?)
        } else {
            None
        };
        Ok(Preview {
            base_number: base.active_number,
            rules_hash: analysis.rules_hash,
            validation: analysis.validation,
            report: analysis.report,
            locked_changes: analysis.changes,
            simulation,
            preview_hash: analysis.hash,
            blocking: analysis.blocking,
            notes: self.notes().await,
        })
    }
}

/// The finding kind inside a `kind:first:second` key.
fn kind_of(key: &str) -> String {
    key.split(':').next().unwrap_or_default().to_owned()
}

/// The acknowledged keys, sorted and deduplicated, when they are exactly the blocking ones.
fn check_acknowledgements(
    analysis: &Analysis,
    acknowledged: &[String],
) -> Result<Vec<String>, PolicyCommandError> {
    let given: BTreeSet<&String> = acknowledged.iter().collect();
    let blocking: BTreeSet<&String> = analysis.blocking.iter().collect();
    let missing: Vec<String> = blocking.difference(&given).map(|k| (*k).clone()).collect();
    if !missing.is_empty() {
        return Err(PolicyCommandError::Unacknowledged { keys: missing });
    }
    let unknown: Vec<String> = given.difference(&blocking).map(|k| (*k).clone()).collect();
    if !unknown.is_empty() {
        return Err(PolicyCommandError::UnknownAcknowledgement { keys: unknown });
    }
    Ok(given.into_iter().cloned().collect())
}
