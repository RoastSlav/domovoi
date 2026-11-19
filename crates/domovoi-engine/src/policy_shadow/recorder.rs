//! The sweep that gives every Execution step one decision row.
//!
//! The recorder reads the environment and the policy repository only. It holds no operation
//! or action handle, no link, no approval or permit, and commits nothing but
//! `PolicyWrite::RecordShadow`. A failed sweep cannot block an operation: the operation writer
//! never waits for it.

use std::sync::Arc;

use domovoi_core::policy_store::{
    MAX_ROW_JSON_BYTES, PolicyWrite, ShadowCandidate, ShadowRecord, ShadowStatus, SweepStats,
};
use domovoi_core::ports::{
    Clock, CommitError, PolicyRepo, StateWrite, StoreError, Transaction, UnitOfWork,
};
use jiff::{SignedDuration, Timestamp};
use tokio_util::sync::CancellationToken;

use super::codec::{CodecError, decode_rules, rules_hash};
use super::doc::DecisionEnvelope;
use super::env::ShadowEnvironment;
use super::inputs::{Built, Verdict, build};
use super::recorded::{CandidateDoc, InputsEnvelope};
use crate::policy::{Rule, evaluate, evaluate_shadow};

/// How often the sweep runs.
pub const SWEEP_EVERY: SignedDuration = SignedDuration::from_secs(5);
/// Steps per batch, and per commit.
pub const BATCH: u16 = 50;
/// Batches per sweep, so a backlog drains over several ticks instead of one long one.
pub const MAX_BATCHES_PER_SWEEP: u32 = 10;
/// A step evaluated later than this after it was created is marked `late`: its inputs are
/// those of evaluation time, not of step creation.
pub const LATE_AFTER: SignedDuration = SignedDuration::from_secs(300);

/// What the stored `last_error` says when the active rules cannot be read.
const RULES_UNREADABLE: &str = "stored rules unreadable";

/// Why a sweep failed.
#[derive(Debug, thiserror::Error)]
pub enum ShadowError {
    /// A read failed.
    #[error("policy store: {0}")]
    Store(#[from] StoreError),
    /// A commit failed.
    #[error("policy commit: {0}")]
    Commit(#[from] CommitError),
    /// The active version's rules could not be decoded.
    #[error("active rules unreadable: {0}")]
    Rules(#[from] CodecError),
}

/// What one pass did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SweepReport {
    /// Rows written with both decisions.
    pub evaluated: u32,
    /// Rows written because the inputs could not be established.
    pub unevaluable: u32,
    /// Rows written for contracts that are never evaluated.
    pub skipped: u32,
    /// Commits made.
    pub batches: u32,
}

/// Records what the policy would decide for each Execution step. Enforces nothing.
pub struct ShadowRecorder {
    repo: Arc<dyn PolicyRepo>,
    uow: Arc<dyn UnitOfWork>,
    env: Arc<dyn ShadowEnvironment>,
    clock: Arc<dyn Clock>,
}

/// The rule set a sweep evaluates against.
struct Active {
    rules: Vec<Rule>,
    version: u32,
    hash: String,
}

impl ShadowRecorder {
    /// A recorder over the given ports.
    #[must_use]
    pub fn new(
        repo: Arc<dyn PolicyRepo>,
        uow: Arc<dyn UnitOfWork>,
        env: Arc<dyn ShadowEnvironment>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            repo,
            uow,
            env,
            clock,
        }
    }

    /// One pass. Tests call this directly; they never sleep.
    pub async fn sweep_once(&self) -> Result<SweepReport, ShadowError> {
        let now = self.clock.now();
        let active = match self.load_active().await {
            Ok(active) => active,
            Err(error) => {
                if matches!(error, ShadowError::Rules(_)) {
                    self.note_failure(now, RULES_UNREADABLE).await;
                }
                return Err(error);
            }
        };
        let mut report = SweepReport::default();
        for _ in 0..MAX_BATCHES_PER_SWEEP {
            let candidates = self.repo.shadow_candidates(BATCH).await?;
            if candidates.is_empty() {
                break;
            }
            let mut records = Vec::with_capacity(candidates.len());
            let mut stats = SweepStats::default();
            for candidate in &candidates {
                let record = self.record_for(candidate, &active, now).await;
                match record.status {
                    ShadowStatus::Evaluated => stats.evaluated += 1,
                    ShadowStatus::Unevaluable => stats.unevaluable += 1,
                    ShadowStatus::Skipped => stats.skipped += 1,
                }
                records.push(record);
            }
            let write = PolicyWrite::RecordShadow {
                records,
                sweep: stats,
                at: now,
                error: None,
            };
            if let Err(error) = self.commit(write).await {
                self.note_failure(now, "could not store decisions").await;
                return Err(error.into());
            }
            report.evaluated += stats.evaluated;
            report.unevaluable += stats.unevaluable;
            report.skipped += stats.skipped;
            report.batches += 1;
        }
        Ok(report)
    }

    /// Loops [`ShadowRecorder::sweep_once`] every [`SWEEP_EVERY`] on the clock port until
    /// `cancel` fires. A failed sweep is logged and the loop carries on.
    pub async fn run(self, cancel: CancellationToken) {
        while !cancel.is_cancelled() {
            if let Err(error) = self.sweep_once().await {
                tracing::warn!(%error, "shadow policy sweep failed; it will be retried");
            }
            let wake = self.clock.now() + SWEEP_EVERY;
            tokio::select! {
                () = cancel.cancelled() => break,
                () = self.clock.sleep_until(wake) => {}
            }
        }
    }

    async fn load_active(&self) -> Result<Active, ShadowError> {
        match self.repo.active().await? {
            Some(version) => {
                let stored = self.repo.rules(version.id).await?;
                let rules = decode_rules(&stored)?;
                Ok(Active {
                    rules,
                    version: version.number.unwrap_or(0),
                    hash: rules_hash(&stored),
                })
            }
            None => Ok(Active {
                rules: Vec::new(),
                version: 0,
                hash: rules_hash(&[]),
            }),
        }
    }

    async fn commit(&self, write: PolicyWrite) -> Result<(), CommitError> {
        let tx = Transaction::new().write(StateWrite::Policy(write));
        self.uow.commit(tx).await.map(|_| ())
    }

    /// Makes a failure visible in the stored state. Best effort: if this commit fails too,
    /// the log line is all there is.
    async fn note_failure(&self, at: Timestamp, text: &str) {
        let write = PolicyWrite::RecordShadow {
            records: Vec::new(),
            sweep: SweepStats::default(),
            at,
            error: Some(text.to_owned()),
        };
        if let Err(error) = self.commit(write).await {
            tracing::warn!(%error, "could not record the shadow sweep failure");
        }
    }

    async fn record_for(
        &self,
        candidate: &ShadowCandidate,
        active: &Active,
        now: Timestamp,
    ) -> ShadowRecord {
        let built = build(candidate, &active.rules, self.env.as_ref(), now).await;
        let mut record = base_record(candidate, active, now, &built);
        match built.verdict {
            Verdict::Skip(code) => {
                record.status = ShadowStatus::Skipped;
                record.reason = Some(code.to_owned());
            }
            Verdict::Unevaluable(code) => {
                record.status = ShadowStatus::Unevaluable;
                record.reason = Some(code.to_owned());
            }
            Verdict::Evaluate => self.evaluate_into(&mut record, built, active, candidate, now),
        }
        record
    }

    fn evaluate_into(
        &self,
        record: &mut ShadowRecord,
        built: Built,
        active: &Active,
        candidate: &ShadowCandidate,
        now: Timestamp,
    ) {
        let (Some(request), Some(inputs)) = (built.request, built.inputs) else {
            record.status = ShadowStatus::Unevaluable;
            record.reason = Some("inputs_missing".to_owned());
            return;
        };
        let enforced = DecisionEnvelope::from_result(&evaluate(&active.rules, &request));
        let shadow = DecisionEnvelope::from_result(&evaluate_shadow(&active.rules, &request));
        let inputs_json = serde_json::to_string(&InputsEnvelope::new(inputs)).unwrap_or_default();
        let (enforced_json, shadow_json) = (enforced.to_json(), shadow.to_json());
        let too_large = [&inputs_json, &enforced_json, &shadow_json]
            .iter()
            .any(|text| text.len() > MAX_ROW_JSON_BYTES);
        if too_large {
            record.status = ShadowStatus::Unevaluable;
            record.reason = Some("too_large".to_owned());
            return;
        }
        record.status = ShadowStatus::Evaluated;
        record.enforced_outcome = Some(enforced.outcome_text().to_owned());
        record.shadow_outcome = Some(shadow.outcome_text().to_owned());
        record.inputs = Some(inputs_json);
        record.enforced = Some(enforced_json);
        record.shadow = Some(shadow_json);
        if now.duration_since(candidate.created_at) > LATE_AFTER {
            record.reason = Some("late".to_owned());
        }
    }
}

/// A row with everything that does not depend on the evaluation.
fn base_record(
    candidate: &ShadowCandidate,
    active: &Active,
    now: Timestamp,
    built: &Built,
) -> ShadowRecord {
    let gaps_in_use = matches!(built.verdict, Verdict::Evaluate);
    let gaps = if gaps_in_use {
        built.gaps.as_slice()
    } else {
        &[]
    };
    ShadowRecord {
        execution: candidate.execution,
        operation: candidate.operation,
        host: candidate.host,
        contract: candidate.contract.to_string(),
        step_created_at: candidate.created_at,
        evaluated_at: now,
        status: ShadowStatus::Unevaluable,
        reason: None,
        policy_version: active.version,
        rules_hash: active.hash.clone(),
        enforced_outcome: None,
        shadow_outcome: None,
        degraded: !gaps.is_empty(),
        candidate: serde_json::to_string(&CandidateDoc::from_candidate(candidate))
            .unwrap_or_default(),
        inputs: None,
        enforced: None,
        shadow: None,
        gaps: serde_json::to_string(gaps).unwrap_or_else(|_| "[]".to_owned()),
    }
}
