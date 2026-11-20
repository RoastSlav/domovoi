//! Contracts for resolution summaries: revisions written through the unit of work and read back
//! through `ExperienceRead`. Run against the in-memory store and SQLite.
//!
//! They assert membership, scope, pass order and the documented tie-breaks, never raw ranking
//! scores: the in-memory store cannot reproduce them.

use async_trait::async_trait;
use domovoi_core::experience::{
    ExecutedContract, ExperienceQuery, ExperienceScope, MatchPass, ResolutionOutcome,
    ResolutionSummary, SummaryField, SummarySource, SummarySubject,
};
use domovoi_core::ports::{
    CommitError, Expectation, ExperienceRead, Observed, OperationWrite, StateWrite,
    SummaryFieldWrite, Transaction, UnitOfWork,
};
use domovoi_core::{
    Budgets, CheckKey, HostId, Operation, OperationId, OperationKind, OperationStatus,
    PinnedSubject, TargetKey,
};
use serde_json::{Value, json};

use super::{ContractError, ContractResult};
use crate::InMemoryStore;
use crate::build::at;

/// One store the experience contracts run against.
#[async_trait]
pub trait ExperienceSubject: Send + Sync {
    /// The unit of work.
    fn uow(&self) -> &dyn UnitOfWork;
    /// Summary reads.
    fn experience(&self) -> &dyn ExperienceRead;
}

impl ExperienceSubject for InMemoryStore {
    fn uow(&self) -> &dyn UnitOfWork {
        self
    }
    fn experience(&self) -> &dyn ExperienceRead {
        self
    }
}

fn setup(error: impl std::fmt::Display) -> ContractError {
    ContractError::Setup(error.to_string())
}

/// What a finished operation looked like.
pub(super) struct Spec<'a> {
    pub(super) hosts: Vec<HostId>,
    pub(super) check: &'a str,
    pub(super) target: Option<&'a str>,
    pub(super) goal: &'a str,
    pub(super) secs: i64,
    pub(super) contracts: Vec<&'a str>,
    pub(super) tags: Vec<&'a str>,
    pub(super) narrative: [Option<&'a str>; 3],
}

impl<'a> Spec<'a> {
    pub(super) fn new(host: HostId, goal: &'a str, secs: i64) -> Self {
        Self {
            hosts: vec![host],
            check: "container.down",
            target: Some("plex"),
            goal,
            secs,
            contracts: Vec::new(),
            tags: Vec::new(),
            narrative: [None; 3],
        }
    }
}

pub(super) async fn commit(
    s: &dyn ExperienceSubject,
    writes: Vec<OperationWrite>,
) -> Result<(), CommitError> {
    let tx = writes.into_iter().fold(Transaction::new(), |tx, write| {
        tx.write(StateWrite::Operation(write))
    });
    s.uow().commit(tx).await.map(|_| ())
}

pub(super) fn revision(
    operation: OperationId,
    field: SummaryField,
    expected: u32,
    value: Value,
    source: SummarySource,
    secs: i64,
    tags: Option<Vec<String>>,
) -> OperationWrite {
    OperationWrite::SummaryField(SummaryFieldWrite {
        operation,
        field,
        expected_revision: expected,
        value,
        source,
        actor: match source {
            SummarySource::User => "user:tester".to_owned(),
            _ => "system:operations".to_owned(),
        },
        at: at(secs),
        tags,
    })
}

pub(super) fn operation_of(
    spec: &Spec<'_>,
    status: OperationStatus,
) -> Result<Operation, ContractError> {
    let mut operation = Operation::queued(
        OperationId::new(),
        OperationKind::Investigate,
        spec.goal,
        vec![PinnedSubject {
            host: spec.hosts[0],
            check: CheckKey::parse(spec.check).map_err(setup)?,
            target: spec
                .target
                .map(TargetKey::parse)
                .transpose()
                .map_err(setup)?,
        }],
        None,
        Budgets::default(),
        at(spec.secs),
    )
    .map_err(setup)?;
    if status.is_terminal() {
        operation.ended_at = Some(at(spec.secs + 1));
        operation.updated_at = at(spec.secs + 1);
    }
    operation.status = status;
    Ok(operation)
}

/// Stores a finished operation with no summary.
pub(super) async fn bare(
    s: &dyn ExperienceSubject,
    spec: &Spec<'_>,
) -> Result<Operation, ContractError> {
    let operation = operation_of(spec, OperationStatus::Succeeded)?;
    commit(
        s,
        vec![OperationWrite::Operation {
            operation: operation.clone(),
            expected: domovoi_core::ports::Expected::New,
        }],
    )
    .await?;
    Ok(operation)
}

/// Stores a finished operation and all five code fields and the narrative.
pub(super) async fn finish(
    s: &dyn ExperienceSubject,
    spec: &Spec<'_>,
) -> Result<OperationId, ContractError> {
    let operation = bare(s, spec).await?;
    let id = operation.id;
    let tags: Vec<String> = spec.tags.iter().map(|t| (*t).to_owned()).collect();
    let subjects = vec![SummarySubject {
        host: spec.hosts[0],
        check: CheckKey::parse(spec.check).map_err(setup)?,
        target: spec
            .target
            .map(TargetKey::parse)
            .transpose()
            .map_err(setup)?,
    }];
    let contracts: Vec<ExecutedContract> = spec
        .contracts
        .iter()
        .map(|c| ExecutedContract {
            contract: (*c).to_owned(),
            host: spec.hosts[0],
            target: None,
            outcome: "succeeded".to_owned(),
        })
        .collect();
    let code = |field, value| {
        revision(
            id,
            field,
            0,
            value,
            SummarySource::Code,
            spec.secs + 2,
            Some(tags.clone()),
        )
    };
    let mut writes = vec![
        code(SummaryField::Outcome, json!("succeeded")),
        code(SummaryField::Verified, json!(true)),
        code(SummaryField::Subjects, json!(subjects)),
        code(SummaryField::ContractsExecuted, json!(contracts)),
        code(SummaryField::Hosts, json!(spec.hosts)),
    ];
    for (field, text) in [
        SummaryField::Problem,
        SummaryField::Cause,
        SummaryField::Fix,
    ]
    .into_iter()
    .zip(spec.narrative)
    {
        if let Some(text) = text {
            writes.push(revision(
                id,
                field,
                0,
                json!(text),
                SummarySource::Model,
                spec.secs + 2,
                None,
            ));
        }
    }
    commit(s, writes).await?;
    Ok(id)
}

/// Stores one finished operation on `host`, with its five code fields and the given
/// problem, cause and fix texts as model drafts, so a reader's tests have a past operation to
/// find. It ended `secs` seconds after the test epoch.
///
/// # Errors
///
/// Fails when the store refuses the writes.
pub async fn seed_finished(
    s: &dyn ExperienceSubject,
    host: HostId,
    goal: &str,
    secs: i64,
    narrative: [Option<&str>; 3],
) -> Result<OperationId, ContractError> {
    let mut spec = Spec::new(host, goal, secs);
    spec.narrative = narrative;
    finish(s, &spec).await
}

pub(super) fn is_constraint(result: &Result<(), CommitError>) -> bool {
    matches!(result, Err(CommitError::Constraint(_)))
}

pub(super) async fn search(
    s: &dyn ExperienceSubject,
    scope: ExperienceScope,
    text: &str,
    exclude: Option<OperationId>,
) -> Result<Vec<(OperationId, MatchPass)>, ContractError> {
    let hits = s
        .experience()
        .search(&ExperienceQuery {
            scope,
            text: text.to_owned(),
            limit: 50,
            exclude,
        })
        .await?;
    Ok(hits
        .into_iter()
        .map(|h| (h.summary.operation, h.pass))
        .collect())
}

pub(super) fn ids(hits: &[(OperationId, MatchPass)]) -> Vec<OperationId> {
    hits.iter().map(|(id, _)| *id).collect()
}

/// Writing `problem` first is refused; writing `outcome` first works.
pub async fn first_revision_must_be_the_outcome(s: &dyn ExperienceSubject) -> ContractResult {
    let operation = bare(s, &Spec::new(HostId::new(), "first", 10)).await?;
    let narrative = revision(
        operation.id,
        SummaryField::Problem,
        0,
        json!("late"),
        SummarySource::Model,
        20,
        None,
    );
    assert!(is_constraint(&commit(s, vec![narrative]).await));
    assert!(s.experience().summary(operation.id).await?.is_none());
    let outcome = revision(
        operation.id,
        SummaryField::Outcome,
        0,
        json!("failed"),
        SummarySource::Code,
        20,
        None,
    );
    commit(s, vec![outcome]).await?;
    let summary = s.experience().summary(operation.id).await?;
    assert_eq!(summary.map(|x| x.outcome), Some(ResolutionOutcome::Failed));
    Ok(())
}

/// An operation that has not finished takes no revision.
pub async fn a_summary_needs_a_finished_operation(s: &dyn ExperienceSubject) -> ContractResult {
    let operation = operation_of(
        &Spec::new(HostId::new(), "open", 10),
        OperationStatus::Running,
    )?;
    commit(
        s,
        vec![OperationWrite::Operation {
            operation: operation.clone(),
            expected: domovoi_core::ports::Expected::New,
        }],
    )
    .await?;
    let outcome = revision(
        operation.id,
        SummaryField::Outcome,
        0,
        json!("succeeded"),
        SummarySource::Code,
        20,
        None,
    );
    assert!(is_constraint(&commit(s, vec![outcome]).await));
    Ok(())
}

/// Revisions are dense; a write that expects a stale revision conflicts and changes nothing.
pub async fn revisions_are_dense_and_a_stale_revision_conflicts(
    s: &dyn ExperienceSubject,
) -> ContractResult {
    let id = finish(s, &Spec::new(HostId::new(), "dense", 10)).await?;
    let again = |expected| {
        revision(
            id,
            SummaryField::Outcome,
            expected,
            json!("failed"),
            SummarySource::Code,
            30,
            None,
        )
    };
    commit(s, vec![again(1)]).await?;
    let stale = commit(s, vec![again(1)]).await;
    let Err(CommitError::Conflict {
        expected, found, ..
    }) = stale
    else {
        return Err(setup("a stale revision was accepted"));
    };
    assert_eq!(expected, Expectation::Version(1));
    assert_eq!(found, Observed::Version(2));
    let all = s.experience().revisions(id).await?;
    let outcomes: Vec<u32> = all
        .iter()
        .filter(|r| r.field == SummaryField::Outcome)
        .map(|r| r.revision)
        .collect();
    assert_eq!(outcomes, [1, 2]);
    Ok(())
}

/// A person cannot write a code field and code cannot write a narrative one.
pub async fn source_must_match_field_ownership(s: &dyn ExperienceSubject) -> ContractResult {
    let id = finish(s, &Spec::new(HostId::new(), "owners", 10)).await?;
    let by_user = revision(
        id,
        SummaryField::Outcome,
        1,
        json!("failed"),
        SummarySource::User,
        30,
        None,
    );
    assert!(is_constraint(&commit(s, vec![by_user]).await));
    let by_code = revision(
        id,
        SummaryField::Problem,
        0,
        json!("x"),
        SummarySource::Code,
        30,
        None,
    );
    assert!(is_constraint(&commit(s, vec![by_code]).await));
    Ok(())
}

/// `current` has the newest revision per field in field order; `revisions` has them all.
pub async fn current_returns_the_newest_revision_of_each_field_in_order(
    s: &dyn ExperienceSubject,
) -> ContractResult {
    let mut spec = Spec::new(HostId::new(), "order", 10);
    spec.narrative = [Some("p"), Some("c"), None];
    let id = finish(s, &spec).await?;
    let edit = revision(
        id,
        SummaryField::Problem,
        1,
        json!("edited"),
        SummarySource::User,
        40,
        None,
    );
    let fix = revision(
        id,
        SummaryField::Fix,
        0,
        json!("by hand"),
        SummarySource::User,
        41,
        None,
    );
    commit(s, vec![edit, fix]).await?;
    let current = s.experience().current(id).await?;
    let fields: Vec<_> = current.iter().map(|r| r.field).collect();
    assert_eq!(fields, SummaryField::ALL);
    let problem = &current[5];
    assert_eq!((problem.revision, problem.source), (2, SummarySource::User));
    let all = s.experience().revisions(id).await?;
    assert_eq!(all.len(), 9);
    let keys: Vec<_> = all.iter().map(|r| (r.field, r.revision)).collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
    Ok(())
}

/// The summary equals the fold of the newest revisions.
pub async fn the_projection_equals_the_fold_of_the_revisions(
    s: &dyn ExperienceSubject,
) -> ContractResult {
    let host = HostId::new();
    let mut spec = Spec::new(host, "fold", 10);
    spec.narrative = [Some("p"), Some("c"), Some("f")];
    let id = finish(s, &spec).await?;
    let user = revision(
        id,
        SummaryField::Cause,
        1,
        json!("better cause"),
        SummarySource::User,
        40,
        None,
    );
    let code = revision(
        id,
        SummaryField::Verified,
        1,
        json!(false),
        SummarySource::Code,
        41,
        None,
    );
    commit(s, vec![user, code]).await?;
    let summary = s
        .experience()
        .summary(id)
        .await?
        .ok_or_else(|| setup("no summary"))?;
    let current = s.experience().current(id).await?;
    let folded = ResolutionSummary::fold(&summary.goal, summary.ended_at, &current)
        .map_err(setup)?
        .ok_or_else(|| setup("no fold"))?;
    assert_eq!(summary, folded);
    assert!(!summary.verified);
    assert_eq!(
        summary.cause.map(|c| c.text),
        Some("better cause".to_owned())
    );
    assert_eq!(summary.hosts, [host]);
    Ok(())
}
