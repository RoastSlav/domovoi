//! How an operation command's refusal answers an HTTP request.

use domovoi_core::ExecutionError;
use domovoi_engine::CommandError;
use domovoi_engine::operations::OperationError;

use super::error::ApiError;
use crate::services::OperationCommandError;

/// What an operation command's refusal means for the caller.
pub(crate) fn operation_error(err: OperationError) -> ApiError {
    match err {
        OperationError::NotFound(_) => ApiError::NotFound,
        OperationError::BadToken => {
            ApiError::Forbidden("the approval link is not valid".to_owned())
        }
        OperationError::Execution(ExecutionError::NeedsTypedConfirmation) => {
            ApiError::Forbidden("type the skill's name to approve this action".to_owned())
        }
        OperationError::Execution(ExecutionError::PlanChanged) => ApiError::Conflict(
            "the plan changed since you opened it; reload the page and review it again".to_owned(),
        ),
        OperationError::Execution(ExecutionError::Expired) => {
            ApiError::Conflict("the approval window has closed".to_owned())
        }
        OperationError::Execution(ExecutionError::AlreadyAuthorized) => {
            ApiError::Conflict("this step was already approved".to_owned())
        }
        OperationError::Execution(ExecutionError::NotYetSupported) => ApiError::Conflict(
            "policy authorizations are not supported yet; a person must approve".to_owned(),
        ),
        OperationError::Execution(ExecutionError::PermitNotAllowed) => ApiError::Conflict(
            "a policy permit can authorize only a write; a person must approve".to_owned(),
        ),
        OperationError::Decide(refused) => ApiError::Conflict(refused.to_string()),
        OperationError::Conflict | OperationError::StaleEffect => {
            ApiError::Conflict("another change landed first; reload and try again".to_owned())
        }
        OperationError::HostUnavailable(_) => {
            ApiError::Conflict("the host's agent is not connected".to_owned())
        }
        OperationError::Store(e) => ApiError::internal("operation change failed", e),
        OperationError::Unavailable => {
            ApiError::Unavailable("the operation service is restarting; try again".to_owned())
        }
    }
}

/// What a refused service command means for the caller.
pub(crate) fn command_error(err: OperationCommandError) -> ApiError {
    match err {
        OperationCommandError::NotEnabled => {
            ApiError::Unavailable("orchestration is not enabled".to_owned())
        }
        OperationCommandError::Invalid(text) => ApiError::BadRequest(text),
        OperationCommandError::TypedConfirmationRequired => {
            ApiError::Forbidden("type the skill's name to approve a destructive action".to_owned())
        }
        OperationCommandError::WebOnly => {
            ApiError::Forbidden("Approve this one in the web UI".to_owned())
        }
        OperationCommandError::Conflict(text) => ApiError::Conflict(text),
        OperationCommandError::NotFound => ApiError::NotFound,
        OperationCommandError::Operation(err) => operation_error(err),
    }
}

/// What a refused incident command means for the caller.
pub(crate) fn command(err: CommandError) -> ApiError {
    match err {
        CommandError::UnknownIncident(_) => ApiError::NotFound,
        CommandError::NotAllowed(why) => ApiError::Conflict(why),
        CommandError::Unavailable => {
            ApiError::Unavailable("the incident manager is not running".to_owned())
        }
        CommandError::Store(e) => ApiError::internal("incident command failed", e),
    }
}
