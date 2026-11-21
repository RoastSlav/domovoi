//! The package store: its status, the imported packages, importing a signed archive and
//! deleting one. Reads work while packages are disabled; writes answer 409 `packages_disabled`.

use async_trait::async_trait;
use axum::Json;
use axum::extract::multipart::{Field, MultipartError};
use axum::extract::{Multipart, Path, State};
use axum::http::StatusCode;
use domovoi_core::package::{MAX_SIGNATURE_FILE_BYTES, PackageName, PackageVersion};
use utoipa::ToSchema;

use super::dto_packages::{PackageDetailDto, PackageDto, PackagesStatusDto};
use super::error::{ApiError, ErrorBody};
use super::packages_error::api_error;
use super::{ApiState, Authed};
use crate::packages::{ArchiveSource, StagedArchive};
use crate::services::{Caller, PackageCommands};

/// The most a package upload may weigh on top of the archive itself: the signature file and
/// the form around the two files.
pub const UPLOAD_OVERHEAD_BYTES: usize = 64 * 1024;

/// The largest upload any configuration allows: the biggest archive a hub accepts (256 MiB)
/// and the form around it.
pub const UPLOAD_CEILING_BYTES: usize = 256 * 1024 * 1024 + UPLOAD_OVERHEAD_BYTES;

/// The request body limit of the upload route for a hub that accepts archives of up to
/// `max_artifact_bytes`.
#[must_use]
pub fn upload_limit(max_artifact_bytes: u64) -> usize {
    usize::try_from(max_artifact_bytes)
        .unwrap_or(usize::MAX)
        .saturating_add(UPLOAD_OVERHEAD_BYTES)
        .min(UPLOAD_CEILING_BYTES)
}

/// The form an import posts. Documents the request; the handler reads it field by field.
#[derive(Debug, ToSchema)]
pub struct PackageImportForm {
    /// The `<name>-<version>.tar.zst` archive.
    #[schema(value_type = String, format = Binary)]
    pub archive: Vec<u8>,
    /// The detached `.sig.toml` signature file.
    #[schema(value_type = String, format = Binary)]
    pub signature: Vec<u8>,
}

/// What the hub holds, how it is limited and which keys it trusts.
#[utoipa::path(
    get, path = "/api/v1/packages/status", operation_id = "packages_status", tag = "packages",
    responses(
        (status = 200, description = "The package store's status", body = PackagesStatusDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
    )
)]
pub(super) async fn status(
    State(state): State<ApiState>,
    _auth: Authed,
) -> Result<Json<PackagesStatusDto>, ApiError> {
    let status = state
        .services()
        .packages
        .status()
        .await
        .map_err(api_error)?;
    Ok(Json(status.into()))
}

/// Every imported package.
#[utoipa::path(
    get, path = "/api/v1/packages", operation_id = "list_packages", tag = "packages",
    responses(
        (status = 200, description = "The imported packages", body = Vec<PackageDto>),
        (status = 401, description = "Not logged in", body = ErrorBody),
    )
)]
pub(super) async fn list(
    State(state): State<ApiState>,
    _auth: Authed,
) -> Result<Json<Vec<PackageDto>>, ApiError> {
    let rows = state.services().packages.list().await.map_err(api_error)?;
    Ok(Json(
        rows.iter().map(|row| PackageDto::of(row, false)).collect(),
    ))
}

fn parse_package(name: &str, version: &str) -> Result<(PackageName, PackageVersion), ApiError> {
    let name = PackageName::parse(name).map_err(|_| ApiError::Coded {
        status: StatusCode::NOT_FOUND,
        code: "no_such_package",
        message: "no such package".to_owned(),
    })?;
    let version = PackageVersion::parse(version).map_err(|_| ApiError::Coded {
        status: StatusCode::NOT_FOUND,
        code: "no_such_package",
        message: "no such package".to_owned(),
    })?;
    Ok((name, version))
}

/// One imported package with its signature file, the hosts known to have it and whether its
/// archive file is in the store.
#[utoipa::path(
    get, path = "/api/v1/packages/{name}/{version}", operation_id = "get_package", tag = "packages",
    params(
        ("name" = String, Path, description = "Package name"),
        ("version" = String, Path, description = "Package version"),
    ),
    responses(
        (status = 200, description = "The package", body = PackageDetailDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such package", body = ErrorBody),
    )
)]
pub(super) async fn detail(
    State(state): State<ApiState>,
    _auth: Authed,
    Path((name, version)): Path<(String, String)>,
) -> Result<Json<PackageDetailDto>, ApiError> {
    let (name, version) = parse_package(&name, &version)?;
    let detail = state
        .services()
        .packages
        .detail(&name, &version)
        .await
        .map_err(api_error)?;
    Ok(Json(detail.into()))
}

/// Removes an imported package from the hub. Copies already installed on hosts stay.
#[utoipa::path(
    delete, path = "/api/v1/packages/{name}/{version}", operation_id = "delete_package", tag = "packages",
    params(
        ("name" = String, Path, description = "Package name"),
        ("version" = String, Path, description = "Package version"),
    ),
    responses(
        (status = 204, description = "Deleted"),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such package", body = ErrorBody),
        (status = 409, description = "Packages are disabled, or a running install job uses it", body = ErrorBody),
    )
)]
pub(super) async fn delete(
    State(state): State<ApiState>,
    auth: Authed,
    Path((name, version)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let (name, version) = parse_package(&name, &version)?;
    state
        .services()
        .packages
        .delete(&auth.caller(), &name, &version)
        .await
        .map_err(api_error)?;
    Ok(StatusCode::NO_CONTENT)
}

fn bad_form(message: &str) -> ApiError {
    ApiError::BadRequest(message.to_owned())
}

fn too_large() -> ApiError {
    ApiError::Coded {
        status: StatusCode::PAYLOAD_TOO_LARGE,
        code: "too_large",
        message: "the upload is larger than this hub accepts".to_owned(),
    }
}

fn multipart_error(err: &MultipartError) -> ApiError {
    if err.status() == StatusCode::PAYLOAD_TOO_LARGE {
        too_large()
    } else {
        ApiError::BadRequest(format!("the upload could not be read: {}", err.body_text()))
    }
}

/// The archive field as the importer's source: one chunk at a time, never the whole body.
struct FieldSource<'a, 'f> {
    field: &'f mut Field<'a>,
    /// Set when the body limit, not the importer's, ended the stream.
    over_limit: bool,
}

#[async_trait]
impl ArchiveSource for FieldSource<'_, '_> {
    async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, std::io::Error> {
        match self.field.chunk().await {
            Ok(chunk) => Ok(chunk.map(|bytes| bytes.to_vec())),
            Err(err) => {
                self.over_limit = err.status() == StatusCode::PAYLOAD_TOO_LARGE;
                Err(std::io::Error::other(err.body_text()))
            }
        }
    }
}

/// The signature file, read in memory. A file past the limit is cut to one byte over it, so
/// the importer refuses it by name and the rest is skipped.
async fn read_signature(field: &mut Field<'_>) -> Result<Vec<u8>, ApiError> {
    let mut bytes = Vec::new();
    while let Some(chunk) = field.chunk().await.map_err(|err| multipart_error(&err))? {
        let room = (MAX_SIGNATURE_FILE_BYTES + 1).saturating_sub(bytes.len());
        bytes.extend_from_slice(&chunk[..chunk.len().min(room)]);
    }
    Ok(bytes)
}

/// Reads the form's two files, in either order.
async fn read_fields(
    form: &mut Multipart,
    commands: &PackageCommands,
    caller: &Caller,
    staged: &mut Option<StagedArchive>,
    signature: &mut Option<Vec<u8>>,
) -> Result<(), ApiError> {
    while let Some(mut field) = form
        .next_field()
        .await
        .map_err(|err| multipart_error(&err))?
    {
        let name = field.name().map(str::to_owned);
        match name.as_deref() {
            Some("archive") => {
                if staged.is_some() {
                    return Err(bad_form("the archive was sent twice"));
                }
                let mut source = FieldSource {
                    field: &mut field,
                    over_limit: false,
                };
                match commands.stage(caller, &mut source).await {
                    Ok(archive) => *staged = Some(archive),
                    Err(_) if source.over_limit => return Err(too_large()),
                    Err(err) => return Err(api_error(err)),
                }
            }
            Some("signature") => {
                if signature.is_some() {
                    return Err(bad_form("the signature file was sent twice"));
                }
                *signature = Some(read_signature(&mut field).await?);
            }
            Some(_) => {
                return Err(bad_form(
                    "the form has a field other than archive and signature",
                ));
            }
            None => return Err(bad_form("the form has a field without a name")),
        }
    }
    Ok(())
}

/// Reads both files; the staged archive is deleted when anything fails.
async fn receive(
    mut form: Multipart,
    commands: &PackageCommands,
    caller: &Caller,
) -> Result<(StagedArchive, Vec<u8>), ApiError> {
    let mut staged = None;
    let mut signature = None;
    let read = read_fields(&mut form, commands, caller, &mut staged, &mut signature).await;
    match (read, staged, signature) {
        (Ok(()), Some(archive), Some(signature)) => Ok((archive, signature)),
        (Ok(()), staged, _) => {
            if let Some(archive) = staged {
                archive.discard().await;
            }
            Err(bad_form(
                "send exactly two files: the archive and its signature",
            ))
        }
        (Err(err), staged, _) => {
            if let Some(archive) = staged {
                archive.discard().await;
            }
            Err(err)
        }
    }
}

/// Imports a signed package: the archive and its detached signature file as one multipart
/// form with the fields `archive` and `signature`. The hub checks the signature file, the
/// archive's digest and the signing key, then stores the archive under its digest. It never
/// opens the archive. No password is asked for: importing installs nothing.
#[utoipa::path(
    post, path = "/api/v1/packages/import", operation_id = "import_package", tag = "packages",
    request_body(content = PackageImportForm, content_type = "multipart/form-data"),
    responses(
        (status = 201, description = "Imported", body = PackageDto),
        (status = 200, description = "The same archive was already imported; nothing changed", body = PackageDto),
        (status = 400, description = "Not exactly the two files `archive` and `signature`", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 409, description = "Packages are disabled, the store is full, or the name and version belong to another archive", body = ErrorBody),
        (status = 413, description = "The archive is larger than the hub accepts", body = ErrorBody),
        (status = 422, description = "The signature file, the signature, the key or the archive is not acceptable", body = ErrorBody),
    )
)]
pub(super) async fn import(
    State(state): State<ApiState>,
    auth: Authed,
    form: Multipart,
) -> Result<(StatusCode, Json<PackageDto>), ApiError> {
    let caller = auth.caller();
    let commands = &state.services().packages;
    commands.stage_allowed(&caller).map_err(api_error)?;
    let (staged, signature) = receive(form, commands, &caller).await?;
    let outcome = commands
        .finish(&caller, staged, &signature)
        .await
        .map_err(api_error)?;
    let status = if outcome.already_imported {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    Ok((
        status,
        Json(PackageDto::of(&outcome.artifact, outcome.already_imported)),
    ))
}
