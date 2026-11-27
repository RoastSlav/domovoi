//! Management paths: listing them, confirming them against verified coverage, and marking
//! them stale when the network changed.

use domovoi_core::knowledge::{
    CoverageEvidence, CoverageVerdict, Fact, FactSource, FactUpdate, KnowledgeStatus,
    KnowledgeWrite, PathConfirmation, PathState, Predicate, SubjectRef, UnverifiableReason,
    path_state,
};
use domovoi_core::ports::{AuditDraft, StateWrite, Transaction, Versioned};
use domovoi_core::{Actor, FactRecordId, HostId, UserId};
use serde_json::json;

use super::decide::apply_update;
use super::error::KnowledgeError;
use super::service::{KnowledgeService, fact_journal, system};

/// A management-path fact with its confirmation and where it stands.
#[derive(Debug, Clone)]
pub struct ManagementPathView {
    /// The path fact.
    pub fact: Versioned<Fact>,
    /// The owner's confirmation, if there is one for this exact fact.
    pub confirmation: Option<PathConfirmation>,
    /// Where the path stands.
    pub state: PathState,
}

/// A confirmed management path and whether its confirmation still holds against the helper's
/// current attestation.
#[derive(Debug, Clone)]
pub struct ConfirmedPath {
    /// The path as listed.
    pub path: ManagementPathView,
    /// `Ok` when the confirmation is current; otherwise why it is not.
    pub currency: Result<(), UnverifiableReason>,
}

/// Why a network change marks paths stale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkChangeCause {
    /// The host's address changed.
    PeerAddressChanged,
    /// The helper's set of covered paths changed.
    HelperSafetySetChanged,
    /// Anything else.
    Other,
}

impl NetworkChangeCause {
    /// The name recorded in the audit entry.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PeerAddressChanged => "peer_address_changed",
            Self::HelperSafetySetChanged => "helper_safety_set_changed",
            Self::Other => "other",
        }
    }
}

/// The audit entry for a management path that went stale.
pub(super) fn stale_audit(
    fact: &Fact,
    host: HostId,
    cause: NetworkChangeCause,
    at: jiff::Timestamp,
) -> AuditDraft {
    AuditDraft::new(
        system(),
        at,
        "knowledge.management_path.stale".to_owned(),
        format!("fact:{}", fact.id),
        json!({"host": host.to_string(), "cause": cause.as_str()}),
    )
}

impl KnowledgeService {
    /// The host's live management paths, ordered by range.
    pub async fn management_paths(
        &self,
        host: HostId,
    ) -> Result<Vec<ManagementPathView>, KnowledgeError> {
        let now = self.now();
        let live = self
            .deps
            .read
            .live_facts(&SubjectRef::Host(host), Predicate::ManagementPath)
            .await?;
        let mut views = Vec::with_capacity(live.len());
        for fact in live {
            let confirmation = self.deps.read.confirmation(fact.value.id).await?;
            let state = path_state(&fact.value, confirmation.as_ref(), now);
            views.push(ManagementPathView {
                fact,
                confirmation,
                state,
            });
        }
        let cidr = |v: &ManagementPathView| {
            v.fact
                .value
                .value
                .as_ref()
                .and_then(|value| value.get("cidr"))
                .and_then(|c| c.as_str())
                .map(str::to_owned)
        };
        views.sort_by_key(cidr);
        Ok(views)
    }

    /// The host's confirmed management paths, each with whether its confirmation is still
    /// current against the helper's verified attestation. A path that is not confirmed is not
    /// listed.
    pub async fn confirmed_paths_with_currency(
        &self,
        host: HostId,
    ) -> Result<Vec<ConfirmedPath>, KnowledgeError> {
        let mut out = Vec::new();
        for path in self.management_paths(host).await? {
            let Some(confirmation) = path.confirmation.as_ref() else {
                continue;
            };
            if path.state != PathState::Confirmed {
                continue;
            }
            let currency = self
                .deps
                .coverage
                .confirmation_current(host, &confirmation.evidence, confirmation.confirmed_at)
                .await;
            out.push(ConfirmedPath { path, currency });
        }
        Ok(out)
    }

    /// Confirms the owner's management path, but only when the helper's verified attestation
    /// shows it covered. A refusal changes nothing but is audited with its reason; with no
    /// attestor wired in, that is all that is written.
    pub async fn confirm_management_path(
        &self,
        id: FactRecordId,
        expected: u64,
        user: UserId,
    ) -> Result<ManagementPathView, KnowledgeError> {
        let _guard = self.lock.lock().await;
        let now = self.now();
        let actor = Actor::User(user);
        let found = self.load(id).await?;
        if found.value.predicate != Predicate::ManagementPath {
            return Err(KnowledgeError::InvalidInput(
                "only a management path can be confirmed".to_owned(),
            ));
        }
        let host = found.value.subject.host().ok_or_else(|| {
            KnowledgeError::InvalidInput("a management path belongs to a host".to_owned())
        })?;
        let evidence = match self.confirmable(&found, expected, host, now).await {
            Ok(evidence) => evidence,
            Err(why) => {
                // The row names the fact, never its value, like every knowledge audit row.
                let refused = AuditDraft::new(
                    actor,
                    now,
                    "knowledge.management_path.confirm_refused".to_owned(),
                    format!("fact:{id}"),
                    json!({"host": host.to_string(), "reason": why.to_string()}),
                );
                self.apply(Transaction::new().audit(refused)).await?;
                return Err(why);
            }
        };
        let fact = &found.value;
        let update = FactUpdate {
            status: Some(KnowledgeStatus::Current),
            last_verified_at: Some(now),
            ..FactUpdate::default()
        };
        let after = apply_update(fact, &update);
        let tx = Transaction::new()
            .write(StateWrite::Knowledge(KnowledgeWrite::FactUpdate {
                id,
                expected,
                update,
            }))
            .write(StateWrite::Knowledge(KnowledgeWrite::PathConfirmation(
                PathConfirmation {
                    fact: id,
                    host,
                    confirmed_by: actor.clone(),
                    confirmed_at: now,
                    evidence,
                },
            )))
            .journal(fact_journal(&after, "confirmed", &actor, now))
            .audit(AuditDraft::new(
                actor,
                now,
                "knowledge.management_path.confirmed".to_owned(),
                format!("fact:{id}"),
                json!({"host": host.to_string()}),
            ));
        self.apply(tx).await?;
        let fact = self.load(id).await?;
        let confirmation = self.deps.read.confirmation(id).await?;
        let state = path_state(&fact.value, confirmation.as_ref(), now);
        Ok(ManagementPathView {
            fact,
            confirmation,
            state,
        })
    }

    /// The coverage evidence that allows confirming `found`, or why it may not be confirmed.
    async fn confirmable(
        &self,
        found: &Versioned<Fact>,
        expected: u64,
        host: HostId,
        now: jiff::Timestamp,
    ) -> Result<CoverageEvidence, KnowledgeError> {
        if found.version != expected {
            return Err(KnowledgeError::Conflict);
        }
        let fact = &found.value;
        if fact.source != FactSource::User {
            return Err(KnowledgeError::NotOwnerFact);
        }
        let status = fact.effective_status(now);
        if !matches!(status, KnowledgeStatus::Current | KnowledgeStatus::Stale) {
            return Err(KnowledgeError::NotLive { status });
        }
        let cidr = fact
            .value
            .as_ref()
            .and_then(|v| v.get("cidr"))
            .and_then(|c| c.as_str())
            .ok_or(KnowledgeError::NotLive { status })?;
        match self.deps.coverage.verdict(host, cidr).await {
            CoverageVerdict::Covered { evidence } => Ok(evidence),
            CoverageVerdict::Uncovered => Err(KnowledgeError::PathUncovered),
            CoverageVerdict::Unverifiable { reason } => {
                Err(KnowledgeError::CoverageUnverifiable(reason))
            }
        }
    }

    /// Marks the host's current management paths stale because the network changed. Their
    /// confirmations stay, but a stale path is never `Confirmed`. Returns the ids changed.
    pub async fn note_network_change(
        &self,
        host: HostId,
        cause: NetworkChangeCause,
    ) -> Result<Vec<FactRecordId>, KnowledgeError> {
        let _guard = self.lock.lock().await;
        let now = self.now();
        let actor = system();
        let live = self
            .deps
            .read
            .live_facts(&SubjectRef::Host(host), Predicate::ManagementPath)
            .await?;
        let mut tx = Transaction::new();
        let mut changed = Vec::new();
        for found in live
            .iter()
            .filter(|v| v.value.status == KnowledgeStatus::Current)
        {
            let update = FactUpdate {
                status: Some(KnowledgeStatus::Stale),
                ..FactUpdate::default()
            };
            let after = apply_update(&found.value, &update);
            tx = tx
                .write(StateWrite::Knowledge(KnowledgeWrite::FactUpdate {
                    id: found.value.id,
                    expected: found.version,
                    update,
                }))
                .journal(fact_journal(&after, "stale", &actor, now))
                .audit(stale_audit(&found.value, host, cause, now));
            changed.push(found.value.id);
        }
        if !changed.is_empty() {
            self.apply(tx).await?;
        }
        Ok(changed)
    }
}
