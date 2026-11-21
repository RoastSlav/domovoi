//! How a refused package command is answered: each refusal keeps the stable code the command
//! line prints too.

use axum::http::StatusCode;
use domovoi_engine::packages::PackageError;

use super::error::ApiError;
use crate::packages::ImportError;
use crate::services::PackageCommandError;

fn coded(status: StatusCode, code: &'static str, message: String) -> ApiError {
    ApiError::Coded {
        status,
        code,
        message,
    }
}

/// The answer to a refused import or delete.
pub(super) fn import_error(err: ImportError) -> ApiError {
    let message = err.to_string();
    let unprocessable = |code| coded(StatusCode::UNPROCESSABLE_ENTITY, code, message.clone());
    match &err {
        ImportError::PackagesDisabled => {
            coded(StatusCode::CONFLICT, "packages_disabled", message.clone())
        }
        ImportError::SignatureFileInvalid(_) => unprocessable("signature_file_invalid"),
        ImportError::NotZstd => unprocessable("not_zstd"),
        ImportError::UnknownKey(_) => unprocessable("unknown_key"),
        ImportError::RevokedKey(_) => unprocessable("revoked_key"),
        ImportError::BadSignature => unprocessable("bad_signature"),
        ImportError::DigestMismatch => unprocessable("digest_mismatch"),
        ImportError::TooLarge { .. } => {
            coded(StatusCode::PAYLOAD_TOO_LARGE, "too_large", message.clone())
        }
        ImportError::StoreFull(_) => coded(StatusCode::CONFLICT, "store_full", message.clone()),
        ImportError::PackageConflict { .. } => {
            coded(StatusCode::CONFLICT, "package_conflict", message.clone())
        }
        ImportError::PackageInUse => coded(StatusCode::CONFLICT, "package_in_use", message.clone()),
        ImportError::NoSuchPackage => {
            coded(StatusCode::NOT_FOUND, "no_such_package", message.clone())
        }
        ImportError::Upload(_) => coded(StatusCode::BAD_REQUEST, "upload_failed", message.clone()),
        ImportError::Io(_) | ImportError::Store(_) | ImportError::Commit(_) => {
            ApiError::internal("package store failed", &err)
        }
    }
}

fn install_error(err: PackageError) -> ApiError {
    let message = err.to_string();
    match err {
        PackageError::NoSuchPackage { .. } => {
            coded(StatusCode::NOT_FOUND, "no_such_package", message)
        }
        PackageError::NoSuchJob(_) => coded(StatusCode::NOT_FOUND, "no_such_job", message),
        PackageError::NoSuchTarget(_)
        | PackageError::UnknownHost(_)
        | PackageError::NotEnrolled(_) => ApiError::NotFound,
        PackageError::DuplicateHost(_) | PackageError::HostCount => ApiError::BadRequest(message),
        PackageError::PlanChanged => coded(StatusCode::CONFLICT, "plan_changed", message),
        PackageError::HostBusy(_) => coded(StatusCode::CONFLICT, "host_busy", message),
        PackageError::Conflict(_) => coded(StatusCode::CONFLICT, "package_conflict", message),
        // The agent is not connected, did not answer or is too old: the same answer as the
        // host log endpoint gives.
        PackageError::Link(_) => ApiError::Conflict(message),
        PackageError::BadAnswer(_) => ApiError::Conflict(message),
        PackageError::Unavailable => ApiError::Unavailable(message),
        PackageError::Store(_) => ApiError::internal("install service store failed", message),
    }
}

/// The answer to a refused package command or read.
pub(super) fn api_error(err: PackageCommandError) -> ApiError {
    match err {
        PackageCommandError::Disabled => {
            coded(StatusCode::CONFLICT, "packages_disabled", err.to_string())
        }
        PackageCommandError::WebOnly
        | PackageCommandError::PasswordRequired
        | PackageCommandError::BadPassword
        | PackageCommandError::ConfirmationMismatch => ApiError::Forbidden(err.to_string()),
        PackageCommandError::BadRequest(message) => ApiError::BadRequest(message),
        PackageCommandError::TooMany { retry_after_secs } => ApiError::TooMany(retry_after_secs),
        PackageCommandError::NoSuchHost => ApiError::NotFound,
        PackageCommandError::Import(err) => import_error(err),
        PackageCommandError::Install(err) => install_error(err),
        PackageCommandError::Store(err) => ApiError::from(err),
    }
}
