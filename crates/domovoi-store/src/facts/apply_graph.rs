//! Applying relation and entity writes.

use domovoi_core::knowledge::{Entity, KnowledgeStatus, Relation, RelationUpdate};
use domovoi_core::ports::{AggregateRef, CommitError, CommitReceipt, Expectation, Observed};
use domovoi_core::{EntityId, RelationId};
use sqlx::SqliteConnection;

use crate::codec::{ms, uint};
use crate::uow::backend;

fn relation_live(status: KnowledgeStatus) -> bool {
    matches!(status, KnowledgeStatus::Current | KnowledgeStatus::Stale)
}

/// Inserts a new live relation at version 1.
pub(super) async fn insert_relation(
    conn: &mut SqliteConnection,
    relation: &Relation,
    receipt: &mut CommitReceipt,
) -> Result<(), CommitError> {
    let (Some(from), Some(to)) = (&relation.from, &relation.to) else {
        return Err(CommitError::Constraint(
            "a relation needs both ends".to_owned(),
        ));
    };
    if !relation_live(relation.status) {
        return Err(CommitError::Constraint(
            "a new relation must be current or stale".to_owned(),
        ));
    }
    let existing: Option<(i64,)> = sqlx::query_as("SELECT version FROM relations WHERE id = ?")
        .bind(relation.id.to_string())
        .fetch_optional(&mut *conn)
        .await
        .map_err(backend)?;
    if let Some((version,)) = existing {
        return Err(CommitError::Conflict {
            aggregate: AggregateRef::relation(relation.id),
            expected: Expectation::New,
            found: Observed::Version(uint(version)),
        });
    }
    sqlx::query(
        "INSERT INTO relations (id, from_ref, to_ref, kind, source, status, live, observed_at, \
         evidence_ref, actor, created_at, retracted_at, retracted_by, content_purged_at, \
         version) VALUES (?, ?, ?, ?, ?, ?, 1, ?, ?, ?, ?, ?, ?, ?, 1)",
    )
    .bind(relation.id.to_string())
    .bind(from.to_string())
    .bind(to.to_string())
    .bind(relation.kind.as_str())
    .bind(relation.source.as_str())
    .bind(relation.status.as_str())
    .bind(ms(relation.observed_at))
    .bind(relation.evidence.as_ref().map(ToString::to_string))
    .bind(relation.actor.to_string())
    .bind(ms(relation.created_at))
    .bind(relation.retracted_at.map(ms))
    .bind(relation.retracted_by.as_ref().map(ToString::to_string))
    .bind(relation.content_purged_at.map(ms))
    .execute(&mut *conn)
    .await
    .map_err(backend)?;
    receipt
        .versions
        .push((AggregateRef::relation(relation.id), 1));
    Ok(())
}

/// Changes the mutable columns of a live relation.
pub(super) async fn update_relation(
    conn: &mut SqliteConnection,
    id: RelationId,
    expected: u64,
    update: &RelationUpdate,
    receipt: &mut CommitReceipt,
) -> Result<(), CommitError> {
    let aggregate = AggregateRef::relation(id);
    let row: Option<(i64, String)> =
        sqlx::query_as("SELECT version, status FROM relations WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&mut *conn)
            .await
            .map_err(backend)?;
    let Some((version, status)) = row else {
        return Err(CommitError::Conflict {
            aggregate,
            expected: Expectation::Version(expected),
            found: Observed::Missing,
        });
    };
    let version = uint(version);
    if version != expected {
        return Err(CommitError::Conflict {
            aggregate,
            expected: Expectation::Version(expected),
            found: Observed::Version(version),
        });
    }
    let stored = KnowledgeStatus::parse(&status)
        .ok_or_else(|| CommitError::Backend(format!("unknown stored relation status {status}")))?;
    if !relation_live(stored) {
        return Err(CommitError::Constraint(format!("the relation is {status}")));
    }
    let retracting = update.status == Some(KnowledgeStatus::Retracted);
    if retracting != update.retraction.is_some() {
        return Err(CommitError::Constraint(
            "retraction and the retracted status go together".to_owned(),
        ));
    }
    let new_status = update.status.unwrap_or(stored);
    sqlx::query(
        "UPDATE relations SET status = ?, live = ?, observed_at = COALESCE(?, observed_at), \
         retracted_at = COALESCE(?, retracted_at), retracted_by = COALESCE(?, retracted_by), \
         version = version + 1 WHERE id = ?",
    )
    .bind(new_status.as_str())
    .bind(i64::from(relation_live(new_status)))
    .bind(update.observed_at.map(ms))
    .bind(update.retraction.as_ref().map(|(at, _)| ms(*at)))
    .bind(update.retraction.as_ref().map(|(_, by)| by.to_string()))
    .bind(id.to_string())
    .execute(&mut *conn)
    .await
    .map_err(backend)?;
    receipt.versions.push((aggregate, expected + 1));
    Ok(())
}

/// Inserts a new entity at version 1.
pub(super) async fn insert_entity(
    conn: &mut SqliteConnection,
    entity: &Entity,
    receipt: &mut CommitReceipt,
) -> Result<(), CommitError> {
    let existing: Option<(i64,)> = sqlx::query_as("SELECT version FROM entities WHERE id = ?")
        .bind(entity.id.to_string())
        .fetch_optional(&mut *conn)
        .await
        .map_err(backend)?;
    if let Some((version,)) = existing {
        return Err(CommitError::Conflict {
            aggregate: AggregateRef::entity(entity.id),
            expected: Expectation::New,
            found: Observed::Version(uint(version)),
        });
    }
    sqlx::query(
        "INSERT INTO entities (id, kind, name, created_by, created_at, version) \
         VALUES (?, ?, ?, ?, ?, 1)",
    )
    .bind(entity.id.to_string())
    .bind(&entity.kind)
    .bind(&entity.name)
    .bind(entity.created_by.to_string())
    .bind(ms(entity.created_at))
    .execute(&mut *conn)
    .await
    .map_err(backend)?;
    receipt.versions.push((AggregateRef::entity(entity.id), 1));
    Ok(())
}

/// Checks an entity's version.
async fn check_entity(
    conn: &mut SqliteConnection,
    id: EntityId,
    expected: u64,
) -> Result<(), CommitError> {
    let aggregate = AggregateRef::entity(id);
    let row: Option<(i64,)> = sqlx::query_as("SELECT version FROM entities WHERE id = ?")
        .bind(id.to_string())
        .fetch_optional(&mut *conn)
        .await
        .map_err(backend)?;
    match row {
        None => Err(CommitError::Conflict {
            aggregate,
            expected: Expectation::Version(expected),
            found: Observed::Missing,
        }),
        Some((version,)) if uint(version) != expected => Err(CommitError::Conflict {
            aggregate,
            expected: Expectation::Version(expected),
            found: Observed::Version(uint(version)),
        }),
        Some(_) => Ok(()),
    }
}

/// Renames an entity.
pub(super) async fn rename_entity(
    conn: &mut SqliteConnection,
    id: EntityId,
    expected: u64,
    name: &str,
    receipt: &mut CommitReceipt,
) -> Result<(), CommitError> {
    check_entity(conn, id, expected).await?;
    sqlx::query("UPDATE entities SET name = ?, version = version + 1 WHERE id = ?")
        .bind(name)
        .bind(id.to_string())
        .execute(&mut *conn)
        .await
        .map_err(backend)?;
    receipt
        .versions
        .push((AggregateRef::entity(id), expected + 1));
    Ok(())
}

/// Deletes an entity.
pub(super) async fn delete_entity(
    conn: &mut SqliteConnection,
    id: EntityId,
    expected: u64,
) -> Result<(), CommitError> {
    check_entity(conn, id, expected).await?;
    sqlx::query("DELETE FROM entities WHERE id = ?")
        .bind(id.to_string())
        .execute(&mut *conn)
        .await
        .map_err(backend)?;
    Ok(())
}
