//! The helper call of a containment request and the meaning of its answer.

use domovoi_core::helper::{BlockSource, HelperResponse, UnavailableReason};
use domovoi_proto::Open;
use domovoi_proto::messages::ExpectedSafety;
use domovoi_proto::messages::{CapabilityFailureCode, CapabilityOutcome, CapabilityResult};

use super::args::{BlockArgs, Phase};
use crate::helper_client::{FirewallCall, HelperClientError};

/// The helper call a parsed block request becomes. `None` only when the canonical text is not a
/// shape the helper accepts, which `args::parse` already rules out.
#[must_use]
pub fn call_for(args: &BlockArgs, expected_safety: Option<ExpectedSafety>) -> Option<FirewallCall> {
    let source = BlockSource::new(&args.net.canonical()).ok()?;
    let (block, ttl_s) = (args.block, args.ttl_s);
    Some(match args.phase {
        Phase::Apply => FirewallCall::Apply {
            block,
            source,
            ttl_s,
            expected_safety,
        },
        Phase::Confirm => FirewallCall::Confirm {
            block,
            source,
            ttl_s,
            expected_safety,
        },
        Phase::Rollback => FirewallCall::Rollback {
            block,
            source,
            ttl_s,
        },
    })
}

fn failed(code: CapabilityFailureCode, message: String) -> CapabilityResult {
    let text = serde_json::to_value(code)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "refused".to_owned());
    CapabilityResult {
        outcome: CapabilityOutcome::Failed {
            code: Open::new(text, Some(code)),
            message,
        },
    }
}

/// What the helper's answer means as a capability result. A helper token comes back as
/// `Failed { Refused, "<token>: <detail>" }`; a helper that cannot be reached is
/// `helper_unavailable`. The agent adds nothing to a success and interprets nothing.
#[must_use]
pub fn map_answer(answer: Result<HelperResponse, HelperClientError>) -> CapabilityResult {
    match answer {
        Ok(HelperResponse::Outputs { outputs }) => CapabilityResult {
            outcome: CapabilityOutcome::Succeeded { outputs },
        },
        Ok(HelperResponse::ContainRefused { token, detail }) => {
            let message = match detail {
                Some(detail) => format!("{token}: {detail}"),
                None => token,
            };
            failed(CapabilityFailureCode::Refused, message)
        }
        Ok(HelperResponse::Unavailable { reason, .. }) => match reason {
            UnavailableReason::WriteGroupNeedsOptIn => failed(
                CapabilityFailureCode::Refused,
                "platform_untested".to_owned(),
            ),
            UnavailableReason::PlatformBlocked => failed(
                CapabilityFailureCode::Refused,
                "platform_blocked".to_owned(),
            ),
            UnavailableReason::GroupDisabled | UnavailableReason::NotImplemented => failed(
                CapabilityFailureCode::HelperUnavailable,
                "helper_unavailable: the helper does not offer this operation".to_owned(),
            ),
        },
        Ok(HelperResponse::Refused { token }) => {
            failed(CapabilityFailureCode::Refused, token.as_str().to_owned())
        }
        Ok(_) => failed(
            CapabilityFailureCode::Refused,
            "the helper answered something unexpected".to_owned(),
        ),
        Err(error) => failed(
            CapabilityFailureCode::HelperUnavailable,
            format!("helper_unavailable: {}", error.relay_code()),
        ),
    }
}
