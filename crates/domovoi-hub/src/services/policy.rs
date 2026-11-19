//! The policy use cases: keep one editable draft, validate it, publish it as an immutable
//! numbered version, preview what a change would do and simulate rules over recorded decisions.
//!
//! Nothing here enforces anything. A version is data; the shadow recorder (engine) writes what
//! it would decide, and no approval, dispatch or permit reads either. Changing the live policy
//! widens what the hub may do on its own later, so publishing, and any draft change that
//! touches a locked prohibition, asks for the admin's password again first. The check is the
//! first thing those commands do: a refused password writes nothing, not even an audit row.

use std::sync::Arc;

use domovoi_core::PolicyVersionId;
use domovoi_core::policy_store::{DraftGuard, MAX_NOTE_CHARS, MAX_RULES, PolicyWrite};
use domovoi_core::ports::{
    AuditDraft, Clock, CommitError, PolicyRepo, StateWrite, StoreError, Transaction, UnitOfWork,
};
use domovoi_engine::policy::{
    CapabilityScope, PolicyError, PublicationReport, Rule, validate_publication, validate_rules,
};
use domovoi_engine::policy_shadow::{
    LockChange, ShadowEnvironment, decode_rules, finding_key, locked_changes, preview_hash,
    rules_hash, stored_rules,
};
use serde_json::{Value, json};

use super::Caller;
use super::policy_types::{
    AUDIT_DRAFT_DISCARDED, AUDIT_DRAFT_SAVED, AUDIT_LOCK_CHANGED, MAX_DRAFT_BYTES,
};
pub(super) use super::policy_types::{PACKAGE_PREFIX, STANDING_NOTES};
pub use super::policy_types::{PolicyCommandError, PolicyDeps, RuleIssue, SavedDraft, StepUp};
use crate::auth::{AuthError, AuthState};

/// The policy use cases.
#[derive(Clone)]
pub struct PolicyService {
    pub(super) repo: Arc<dyn PolicyRepo>,
    pub(super) uow: Arc<dyn UnitOfWork>,
    pub(super) env: Arc<dyn ShadowEnvironment>,
    auth: Arc<AuthState>,
    pub(super) clock: Arc<dyn Clock>,
    shadow_enabled: bool,
}

/// The rules a change is measured against.
pub(super) struct Base {
    /// The rules the locked-rule comparison starts from.
    pub(super) rules: Vec<Rule>,
    /// The active version number, 0 when none.
    pub(super) active_number: u32,
}

/// Everything a preview shows except the simulation.
pub(super) struct Analysis {
    pub(super) rules_hash: String,
    pub(super) validation: Vec<RuleIssue>,
    pub(super) report: Option<PublicationReport>,
    pub(super) blocking: Vec<String>,
    pub(super) changes: Vec<LockChange>,
    pub(super) hash: String,
}

impl PolicyService {
    /// The service over `deps`.
    #[must_use]
    pub fn new(deps: PolicyDeps) -> Self {
        Self {
            repo: deps.repo,
            uow: deps.uow,
            env: deps.env,
            auth: deps.auth,
            clock: deps.clock,
            shadow_enabled: deps.shadow_enabled,
        }
    }

    /// Whether shadow recording is on.
    #[must_use]
    pub fn shadow_enabled(&self) -> bool {
        self.shadow_enabled
    }

    /// The standing notes; the ceiling note shows only while no host reports a ceiling.
    pub async fn notes(&self) -> Vec<&'static str> {
        let [ceiling, rest @ ..] = STANDING_NOTES;
        let mut notes = Vec::new();
        if self.env.ceilings().await.is_empty() {
            notes.push(ceiling);
        }
        notes.extend(rest);
        notes
    }

    /// Replaces the whole draft. `base_revision: None` creates it (it fails while one exists);
    /// `Some(n)` replaces draft revision `n`. Changing a locked prohibition needs the password
    /// and the preview hash of exactly these rules.
    pub async fn save_draft(
        &self,
        caller: &Caller,
        rules: Vec<Rule>,
        base_revision: Option<u64>,
        preview_hash: Option<&str>,
        step_up: StepUp<'_>,
    ) -> Result<SavedDraft, PolicyCommandError> {
        check_rules(&rules)?;
        let stored = stored_rules(&rules);
        if stored.iter().map(|rule| rule.body.len()).sum::<usize>() > MAX_DRAFT_BYTES {
            return Err(PolicyCommandError::TooLarge);
        }
        let current = self.repo.draft().await?;
        match (base_revision, &current) {
            (None, Some(_)) => return Err(PolicyCommandError::DraftExists),
            (Some(_), None) => return Err(PolicyCommandError::NoDraft),
            (Some(wanted), Some(draft)) if draft.revision != wanted => {
                return Err(PolicyCommandError::StaleDraft {
                    current: draft.revision,
                });
            }
            _ => {}
        }
        let base = self.load_base(base_revision).await?;
        let changes = locked_changes(&base.rules, &rules);
        if !changes.is_empty() {
            self.step_up(caller, step_up).await?;
            let analysis = self.analyse(&rules, &base).await;
            if preview_hash != Some(analysis.hash.as_str()) {
                return Err(PolicyCommandError::StalePreview);
            }
        }
        let (id, expected, revision) = match &current {
            Some(draft) => (
                draft.id,
                DraftGuard::Revision(draft.revision),
                draft.revision + 1,
            ),
            None => (PolicyVersionId::new(), DraftGuard::Absent, 1),
        };
        let (at, actor) = (self.clock.now(), caller.actor());
        let detail = json!({
            "revision": revision,
            "rules": rules.len(),
            "locked": rules.iter().filter(|rule| rule.locked).count(),
            "shadow": rules.iter().filter(|rule| rule.shadow).count(),
        });
        let mut tx = Transaction::new()
            .write(StateWrite::Policy(PolicyWrite::SaveDraft {
                id,
                expected,
                based_on: (base.active_number > 0).then_some(base.active_number),
                rules_hash: rules_hash(&stored),
                rules: stored,
                actor: actor.clone(),
                at,
            }))
            .audit(audit(&actor, at, AUDIT_DRAFT_SAVED, id, detail));
        for change in &changes {
            tx = tx.audit(lock_audit(&actor, at, id, change, "draft", None));
        }
        self.commit_draft(tx).await?;
        let record = self
            .repo
            .draft()
            .await?
            .ok_or(PolicyCommandError::NoDraft)?;
        Ok(SavedDraft {
            record,
            locked_changes: changes,
        })
    }

    /// Discards draft `revision`. It changes nothing live, so it needs no password.
    pub async fn discard_draft(
        &self,
        caller: &Caller,
        revision: u64,
    ) -> Result<(), PolicyCommandError> {
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
        let (at, actor) = (self.clock.now(), caller.actor());
        let tx = Transaction::new()
            .write(StateWrite::Policy(PolicyWrite::DiscardDraft {
                id: draft.id,
                expected_revision: revision,
            }))
            .audit(audit(
                &actor,
                at,
                AUDIT_DRAFT_DISCARDED,
                draft.id,
                json!({ "revision": revision }),
            ));
        self.commit_draft(tx).await
    }

    /// Asks for the password again. One function for every command that needs it, so the
    /// check cannot drift between them. It shares the login limiter.
    pub(super) async fn step_up(
        &self,
        caller: &Caller,
        proof: StepUp<'_>,
    ) -> Result<(), PolicyCommandError> {
        let Some(password) = proof.password else {
            return Err(PolicyCommandError::PasswordRequired);
        };
        match self
            .auth
            .confirm_password(proof.ip, caller.user(), password)
            .await
        {
            Ok(()) => Ok(()),
            Err(AuthError::LockedOut { retry_after_secs }) => {
                Err(PolicyCommandError::TooMany { retry_after_secs })
            }
            Err(_) => Err(PolicyCommandError::BadPassword),
        }
    }

    /// Reads a version's rules back from storage.
    pub(super) async fn rules_of(&self, id: PolicyVersionId) -> Result<Vec<Rule>, StoreError> {
        let stored = self.repo.rules(id).await?;
        decode_rules(&stored).map_err(|err| StoreError::Backend(err.to_string()))
    }

    /// The rules a change is measured against: the draft when `base_revision` names it, else
    /// the active version when none is named. A named revision that is not the stored draft is
    /// an error, never a silent fallback.
    pub(super) async fn load_base(
        &self,
        base_revision: Option<u64>,
    ) -> Result<Base, PolicyCommandError> {
        let active = self.repo.active().await?;
        let active_number = active.as_ref().and_then(|v| v.number).unwrap_or(0);
        let active_rules = match &active {
            Some(version) => self.rules_of(version.id).await?,
            None => Vec::new(),
        };
        let draft = match (base_revision, self.repo.draft().await?) {
            (None, _) => None,
            (Some(wanted), Some(draft)) if draft.revision == wanted => Some(draft),
            (Some(_), Some(draft)) => {
                return Err(PolicyCommandError::StaleDraft {
                    current: draft.revision,
                });
            }
            (Some(_), None) => return Err(PolicyCommandError::NoDraft),
        };
        let rules = match &draft {
            Some(draft) => self.rules_of(draft.id).await?,
            None => active_rules.clone(),
        };
        Ok(Base {
            rules,
            active_number,
        })
    }

    /// Validation, the publication review, locked changes and the preview hash of `rules`.
    pub(super) async fn analyse(&self, rules: &[Rule], base: &Base) -> Analysis {
        let hash_of_rules = rules_hash(&stored_rules(rules));
        let mut validation = match check_rules(rules) {
            Ok(()) => Vec::new(),
            Err(err) => issues_of(err),
        };
        let mut report = None;
        if validation.is_empty() {
            let index = self.env.publication_index().await;
            let ceilings = self.env.ceilings().await;
            match validate_publication(rules, &index, &ceilings) {
                Ok(found) => report = Some(found),
                Err(err) => validation.push(issue_of(&err)),
            }
        }
        let mut blocking: Vec<String> = report
            .iter()
            .flat_map(PublicationReport::blocking)
            .map(finding_key)
            .collect();
        blocking.sort();
        let changes = locked_changes(&base.rules, rules);
        let hash = preview_hash(base.active_number, &hash_of_rules, &blocking, &changes);
        Analysis {
            rules_hash: hash_of_rules,
            validation,
            report,
            blocking,
            changes,
            hash,
        }
    }

    /// Commits a draft change; a conflict means the draft moved, and says where it is now.
    async fn commit_draft(&self, tx: Transaction) -> Result<(), PolicyCommandError> {
        match self.uow.commit(tx).await {
            Ok(_) => Ok(()),
            Err(CommitError::Conflict { .. }) => Err(PolicyCommandError::StaleDraft {
                current: self.repo.draft().await?.map_or(0, |draft| draft.revision),
            }),
            Err(other) => Err(PolicyCommandError::Commit(other)),
        }
    }
}

/// Refuses a rule set that is too large, names a package contract or is not valid.
pub(super) fn check_rules(rules: &[Rule]) -> Result<(), PolicyCommandError> {
    if rules.len() > MAX_RULES {
        return Err(PolicyCommandError::TooManyRules { max: MAX_RULES });
    }
    for rule in rules {
        if rule.note.chars().count() > MAX_NOTE_CHARS {
            return Err(PolicyCommandError::NoteTooLong {
                rule: rule.id.to_string(),
            });
        }
        if let CapabilityScope::Contract(id) = &rule.capability
            && id.as_str().starts_with(PACKAGE_PREFIX)
        {
            return Err(PolicyCommandError::PackageContract {
                rule: rule.id.to_string(),
            });
        }
    }
    validate_rules(rules).map_err(|err| PolicyCommandError::Invalid(vec![issue_of(&err)]))
}

/// The rule a policy error is about, when it is about one.
fn rule_of(err: &PolicyError) -> Option<String> {
    match err {
        PolicyError::LockedNotDeny { rule, .. }
        | PolicyError::UnavailableRuleOutcome { rule }
        | PolicyError::EmptyTargetSet { rule }
        | PolicyError::DegenerateTimeWindow { rule, .. }
        | PolicyError::InvalidTimeOfDay { rule }
        | PolicyError::NegativeMaxTtl { rule }
        | PolicyError::BadArgKey { rule, .. } => Some(rule.to_string()),
        PolicyError::DuplicateRuleId(rule) => Some(rule.to_string()),
        _ => None,
    }
}

/// An issue for a policy error.
pub(super) fn issue_of(err: &PolicyError) -> RuleIssue {
    RuleIssue {
        rule: rule_of(err),
        message: err.to_string(),
    }
}

/// The issues a refused rule set carries.
fn issues_of(err: PolicyCommandError) -> Vec<RuleIssue> {
    let message = err.to_string();
    match err {
        PolicyCommandError::Invalid(issues) => issues,
        PolicyCommandError::NoteTooLong { rule } | PolicyCommandError::PackageContract { rule } => {
            vec![RuleIssue {
                rule: Some(rule),
                message,
            }]
        }
        _ => vec![RuleIssue {
            rule: None,
            message,
        }],
    }
}

/// An audit row about version `id`.
pub(super) fn audit(
    actor: &domovoi_core::Actor,
    at: jiff::Timestamp,
    kind: &str,
    id: PolicyVersionId,
    detail: Value,
) -> AuditDraft {
    AuditDraft::new(
        actor.clone(),
        at,
        kind.to_owned(),
        format!("policy:{id}"),
        detail,
    )
}

/// The audit row of one change to a locked prohibition.
pub(super) fn lock_audit(
    actor: &domovoi_core::Actor,
    at: jiff::Timestamp,
    id: PolicyVersionId,
    change: &LockChange,
    scope: &str,
    version: Option<u32>,
) -> AuditDraft {
    let mut detail = json!({
        "rule": change.rule.as_str(),
        "change": change.change.as_str(),
        "scope": scope,
    });
    if let (Some(number), Some(map)) = (version, detail.as_object_mut()) {
        map.insert("version".to_owned(), json!(number));
    }
    audit(actor, at, AUDIT_LOCK_CHANGED, id, detail)
}
