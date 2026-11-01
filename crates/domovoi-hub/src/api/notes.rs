//! Notes: host and global notes the triage agent can search.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use domovoi_core::{HostId, Note, NoteId};

use super::dto_admin::{NoteDto, NoteHitDto, NoteInput, NoteListQuery, NoteSearchQuery};
use super::error::{ApiError, ErrorBody};
use super::{ApiState, Authed, host_named};

/// The longest note title, in characters.
pub const MAX_TITLE_CHARS: usize = 200;

/// The largest note body, in bytes.
pub const MAX_BODY_BYTES: usize = 64 * 1024;

/// The most hits a search returns.
pub const MAX_HITS: u16 = 20;

fn dto(note: &Note) -> NoteDto {
    NoteDto {
        id: note.id.to_string(),
        host_id: note.host.map(|h| h.to_string()),
        title: note.title.clone(),
        body_markdown: note.body_markdown.clone(),
        tags: note.tags.clone(),
        updated_at: note.updated_at.to_string(),
    }
}

fn parse_id(id: &str) -> Result<NoteId, ApiError> {
    id.parse().map_err(|_| ApiError::NotFound)
}

async fn host_id(state: &ApiState, name: Option<&str>) -> Result<Option<HostId>, ApiError> {
    match name {
        Some(name) => Ok(Some(host_named(state, name).await?.id)),
        None => Ok(None),
    }
}

async fn validated(state: &ApiState, id: NoteId, input: NoteInput) -> Result<Note, ApiError> {
    let title = input.title.trim();
    if title.is_empty() || title.chars().count() > MAX_TITLE_CHARS {
        return Err(ApiError::BadRequest(format!(
            "the title must have 1 to {MAX_TITLE_CHARS} characters"
        )));
    }
    if input.body_markdown.len() > MAX_BODY_BYTES {
        return Err(ApiError::BadRequest("the note is too long".to_owned()));
    }
    Ok(Note {
        id,
        host: host_id(state, input.host.as_deref()).await?,
        title: title.to_owned(),
        body_markdown: input.body_markdown,
        tags: input.tags,
        updated_at: state.deps.clock.now(),
    })
}

/// Notes about one host, or global notes when no host is given.
#[utoipa::path(
    get, path = "/api/v1/notes", operation_id = "list_notes", tag = "notes",
    params(NoteListQuery),
    responses(
        (status = 200, description = "The notes", body = Vec<NoteDto>),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such host", body = ErrorBody),
    )
)]
pub(super) async fn list(
    State(state): State<ApiState>,
    _auth: Authed,
    Query(query): Query<NoteListQuery>,
) -> Result<Json<Vec<NoteDto>>, ApiError> {
    let host = host_id(&state, query.host.as_deref()).await?;
    let notes = state.deps.notes.list(host).await?;
    Ok(Json(notes.iter().map(dto).collect()))
}

/// Creates a note.
#[utoipa::path(
    post, path = "/api/v1/notes", operation_id = "create_note", tag = "notes",
    request_body = NoteInput,
    responses(
        (status = 201, description = "Created", body = NoteDto),
        (status = 400, description = "Invalid note", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
    )
)]
pub(super) async fn create(
    State(state): State<ApiState>,
    _auth: Authed,
    Json(input): Json<NoteInput>,
) -> Result<(StatusCode, Json<NoteDto>), ApiError> {
    let note = validated(&state, NoteId::new(), input).await?;
    state.deps.notes.upsert(&note).await?;
    Ok((StatusCode::CREATED, Json(dto(&note))))
}

/// Full-text search over notes.
#[utoipa::path(
    get, path = "/api/v1/notes/search", operation_id = "search_notes", tag = "notes",
    params(NoteSearchQuery),
    responses(
        (status = 200, description = "Hits, best first", body = Vec<NoteHitDto>),
        (status = 401, description = "Not logged in", body = ErrorBody),
    )
)]
pub(super) async fn search(
    State(state): State<ApiState>,
    _auth: Authed,
    Query(query): Query<NoteSearchQuery>,
) -> Result<Json<Vec<NoteHitDto>>, ApiError> {
    let host = host_id(&state, query.host.as_deref()).await?;
    let hits = state.deps.notes.search(&query.q, host, MAX_HITS).await?;
    Ok(Json(
        hits.iter()
            .map(|h| NoteHitDto {
                note: dto(&h.note),
                snippet: h.snippet.clone(),
                score: h.score,
            })
            .collect(),
    ))
}

/// One note.
#[utoipa::path(
    get, path = "/api/v1/notes/{id}", operation_id = "get_note", tag = "notes",
    params(("id" = String, Path, description = "Note id")),
    responses(
        (status = 200, description = "The note", body = NoteDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such note", body = ErrorBody),
    )
)]
pub(super) async fn get(
    State(state): State<ApiState>,
    _auth: Authed,
    Path(id): Path<String>,
) -> Result<Json<NoteDto>, ApiError> {
    let note = state
        .deps
        .notes
        .get(parse_id(&id)?)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(dto(&note)))
}

/// Replaces a note.
#[utoipa::path(
    put, path = "/api/v1/notes/{id}", operation_id = "replace_note", tag = "notes",
    params(("id" = String, Path, description = "Note id")),
    request_body = NoteInput,
    responses(
        (status = 200, description = "Replaced", body = NoteDto),
        (status = 400, description = "Invalid note", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such note", body = ErrorBody),
    )
)]
pub(super) async fn replace(
    State(state): State<ApiState>,
    _auth: Authed,
    Path(id): Path<String>,
    Json(input): Json<NoteInput>,
) -> Result<Json<NoteDto>, ApiError> {
    let id = parse_id(&id)?;
    if state.deps.notes.get(id).await?.is_none() {
        return Err(ApiError::NotFound);
    }
    let note = validated(&state, id, input).await?;
    state.deps.notes.upsert(&note).await?;
    Ok(Json(dto(&note)))
}

/// Deletes a note.
#[utoipa::path(
    delete, path = "/api/v1/notes/{id}", operation_id = "delete_note", tag = "notes",
    params(("id" = String, Path, description = "Note id")),
    responses(
        (status = 204, description = "Deleted"),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such note", body = ErrorBody),
    )
)]
pub(super) async fn delete(
    State(state): State<ApiState>,
    _auth: Authed,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state.deps.notes.delete(parse_id(&id)?).await?;
    Ok(StatusCode::NO_CONTENT)
}
