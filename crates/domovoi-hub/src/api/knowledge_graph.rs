//! Knowledge relations and entities.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use domovoi_core::knowledge::{FactSource, RelationKind};
use domovoi_core::ports::{Page, RelationFilter};
use domovoi_engine::knowledge::RelationDraft;

use super::dto_knowledge::{
    EntityCreate, EntityDeleteQuery, EntityDto, EntityRename, RelationDto, RelationInput,
    RelationListQuery, RelationPageDto, RelationRetractInput,
};
use super::error::{ApiError, ErrorBody};
use super::knowledge::{command_error, evidence, limit_of, parse_id, subject};
use super::{ApiState, Authed};

const MAX_LIMIT: u16 = 200;

fn kind(text: &str) -> Result<RelationKind, ApiError> {
    RelationKind::parse(text)
        .ok_or_else(|| ApiError::BadRequest("unknown relation kind".to_owned()))
}

/// Lists relations.
#[utoipa::path(
    get, path = "/api/v1/knowledge/relations", operation_id = "list_relations", tag = "knowledge",
    params(RelationListQuery),
    responses(
        (status = 200, description = "The relations, newest first", body = RelationPageDto),
        (status = 400, description = "A filter value is not valid", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
    )
)]
pub(super) async fn list_relations(
    State(state): State<ApiState>,
    _auth: Authed,
    Query(query): Query<RelationListQuery>,
) -> Result<Json<RelationPageDto>, ApiError> {
    let filter = RelationFilter {
        subject: query.subject.as_deref().map(subject).transpose()?,
        kind: query.kind.as_deref().map(kind).transpose()?,
        include_retracted: query.include_retracted.unwrap_or(false),
    };
    let page = Page {
        cursor: query.cursor,
        limit: limit_of(query.limit, MAX_LIMIT),
    };
    let knowledge = &state.services().knowledge;
    let page = knowledge
        .relations(&filter, &page)
        .await
        .map_err(command_error)?;
    let versioned = knowledge
        .relations_with_versions(&page.items)
        .await
        .map_err(command_error)?;
    let now = state.deps.clock.now();
    Ok(Json(RelationPageDto {
        items: versioned.iter().map(|r| RelationDto::new(r, now)).collect(),
        next_cursor: page.next_cursor,
    }))
}

/// States a relation as the owner.
#[utoipa::path(
    post, path = "/api/v1/knowledge/relations", operation_id = "assert_relation", tag = "knowledge",
    request_body = RelationInput,
    responses(
        (status = 201, description = "Recorded", body = RelationDto),
        (status = 400, description = "The relation is not valid", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
    )
)]
pub(super) async fn assert_relation(
    State(state): State<ApiState>,
    auth: Authed,
    Json(input): Json<RelationInput>,
) -> Result<(StatusCode, Json<RelationDto>), ApiError> {
    let draft = RelationDraft {
        from: subject(&input.from)?,
        kind: kind(&input.kind)?,
        to: subject(&input.to)?,
        source: FactSource::User,
        observed_at: None,
        evidence: evidence(input.evidence.as_deref())?,
    };
    let relation = state
        .services()
        .knowledge
        .assert_relation(&auth.caller(), draft)
        .await
        .map_err(command_error)?;
    Ok((
        StatusCode::CREATED,
        Json(RelationDto::new(&relation, state.deps.clock.now())),
    ))
}

/// Withdraws a relation.
#[utoipa::path(
    post, path = "/api/v1/knowledge/relations/{id}/retract", operation_id = "retract_relation",
    tag = "knowledge",
    params(("id" = String, Path, description = "The relation id")),
    request_body = RelationRetractInput,
    responses(
        (status = 200, description = "Retracted", body = RelationDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such relation", body = ErrorBody),
        (status = 409, description = "Not live, or someone changed it meanwhile", body = ErrorBody),
    )
)]
pub(super) async fn retract_relation(
    State(state): State<ApiState>,
    auth: Authed,
    Path(id): Path<String>,
    Json(input): Json<RelationRetractInput>,
) -> Result<Json<RelationDto>, ApiError> {
    let relation = state
        .services()
        .knowledge
        .retract_relation(&auth.caller(), parse_id(&id)?, input.expected_version)
        .await
        .map_err(command_error)?;
    Ok(Json(RelationDto::new(&relation, state.deps.clock.now())))
}

/// Lists entities.
#[utoipa::path(
    get, path = "/api/v1/knowledge/entities", operation_id = "list_entities", tag = "knowledge",
    responses(
        (status = 200, description = "The entities, by name", body = Vec<EntityDto>),
        (status = 401, description = "Not logged in", body = ErrorBody),
    )
)]
pub(super) async fn list_entities(
    State(state): State<ApiState>,
    _auth: Authed,
) -> Result<Json<Vec<EntityDto>>, ApiError> {
    let entities = state
        .services()
        .knowledge
        .entities()
        .await
        .map_err(command_error)?;
    Ok(Json(entities.iter().map(EntityDto::from).collect()))
}

/// Creates an entity.
#[utoipa::path(
    post, path = "/api/v1/knowledge/entities", operation_id = "create_entity", tag = "knowledge",
    request_body = EntityCreate,
    responses(
        (status = 201, description = "Created", body = EntityDto),
        (status = 400, description = "The kind or name is not valid", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 409, description = "An entity with this name exists", body = ErrorBody),
    )
)]
pub(super) async fn create_entity(
    State(state): State<ApiState>,
    auth: Authed,
    Json(input): Json<EntityCreate>,
) -> Result<(StatusCode, Json<EntityDto>), ApiError> {
    let entity = state
        .services()
        .knowledge
        .create_entity(&auth.caller(), &input.kind, &input.name)
        .await
        .map_err(command_error)?;
    Ok((StatusCode::CREATED, Json(EntityDto::from(&entity))))
}

/// Renames an entity.
#[utoipa::path(
    put, path = "/api/v1/knowledge/entities/{id}", operation_id = "rename_entity",
    tag = "knowledge",
    params(("id" = String, Path, description = "The entity id")),
    request_body = EntityRename,
    responses(
        (status = 200, description = "Renamed", body = EntityDto),
        (status = 400, description = "The name is not valid", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such entity", body = ErrorBody),
        (status = 409, description = "The name is taken, or someone changed it meanwhile", body = ErrorBody),
    )
)]
pub(super) async fn rename_entity(
    State(state): State<ApiState>,
    auth: Authed,
    Path(id): Path<String>,
    Json(input): Json<EntityRename>,
) -> Result<Json<EntityDto>, ApiError> {
    let entity = state
        .services()
        .knowledge
        .rename_entity(
            &auth.caller(),
            parse_id(&id)?,
            input.expected_version,
            &input.name,
        )
        .await
        .map_err(command_error)?;
    Ok(Json(EntityDto::from(&entity)))
}

/// Deletes an entity nothing refers to.
#[utoipa::path(
    delete, path = "/api/v1/knowledge/entities/{id}", operation_id = "delete_entity",
    tag = "knowledge",
    params(("id" = String, Path, description = "The entity id"), EntityDeleteQuery),
    responses(
        (status = 204, description = "Deleted"),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such entity", body = ErrorBody),
        (status = 409, description = "Still in use, or someone changed it meanwhile", body = ErrorBody),
    )
)]
pub(super) async fn delete_entity(
    State(state): State<ApiState>,
    auth: Authed,
    Path(id): Path<String>,
    Query(query): Query<EntityDeleteQuery>,
) -> Result<StatusCode, ApiError> {
    state
        .services()
        .knowledge
        .delete_entity(&auth.caller(), parse_id(&id)?, query.expected_version)
        .await
        .map_err(command_error)?;
    Ok(StatusCode::NO_CONTENT)
}
