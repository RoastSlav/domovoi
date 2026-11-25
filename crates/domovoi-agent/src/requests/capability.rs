//! `run_capability`: the `agent.package.*` backend and the journal flow around it.
//!
//! Every other contract is still refused as unknown. A package request is checked in a
//! fixed order, cheapest and most general first, and nothing reaches the journal until the
//! arguments parsed: a malformed request leaves no record. From acceptance on, the work
//! runs in-process on a blocking thread, so the journal's `started` record carries pid 0 on
//! purpose; recording the agent's own pid could let recovery inspect or signal a live group.

use std::collections::BTreeMap;
use std::sync::Arc;

use domovoi_core::package::{PackageName, PackageVersion, keys};
use domovoi_proto::Message;
use domovoi_proto::Open;
use domovoi_proto::messages::{
    ActionState, AuthorizationKind, CapabilityFailureCode, CapabilityOutcome, CapabilityResult,
    ErrorMessage, RefusalCode, RunCapability,
};
use uuid::Uuid;

use super::{RequestCtx, refused};
use crate::journal::{AcceptOutcome, AcceptedRequest};
use crate::packages::{
    InstallRequest, PackageError, PackageService, PackageSurface, UninstallRequest,
};

const INSTALL: &str = "agent.package.install";
const UNINSTALL: &str = "agent.package.uninstall";
const LIST: &str = "agent.package.list";

/// Which package capability a request names.
#[derive(Debug, Clone)]
enum Operation {
    Install(InstallRequest),
    Uninstall(UninstallRequest),
    List,
}

impl RequestCtx {
    /// The package capabilities this agent offers; nothing when `[packages]` is absent.
    #[must_use]
    pub fn package_surface(&self) -> PackageSurface {
        self.packages
            .as_ref()
            .map(|service| service.surface())
            .unwrap_or_default()
    }
}

pub(super) fn bad_request(message: impl Into<String>) -> Message {
    Message::Error(ErrorMessage {
        code: "bad_request".to_owned(),
        message: message.into(),
    })
}

pub(super) fn unavailable(message: &str) -> Message {
    Message::Error(ErrorMessage {
        code: "unavailable".to_owned(),
        message: message.to_owned(),
    })
}

/// Exactly the keys `wanted`, no more and no fewer.
fn exact_args(run: &RunCapability, wanted: &[&str]) -> Result<(), String> {
    for key in run.args.keys() {
        if !wanted.contains(&key.as_str()) {
            return Err(format!("unexpected argument {key:?}"));
        }
    }
    for key in wanted {
        if !run.args.contains_key(*key) {
            return Err(format!("missing argument {key}"));
        }
    }
    Ok(())
}

fn arg<'a>(run: &'a RunCapability, key: &str) -> &'a str {
    run.args.get(key).map_or("", String::as_str)
}

fn transfer_id(run: &RunCapability, key: &str) -> Result<Uuid, String> {
    Uuid::parse_str(arg(run, key)).map_err(|_| format!("{key} is not a transfer id"))
}

/// Parses the arguments of `run` for the contract it names.
fn parse_operation(run: &RunCapability) -> Result<Operation, String> {
    let name = || PackageName::parse(arg(run, keys::ARG_PACKAGE)).map_err(|e| e.to_string());
    let version = || PackageVersion::parse(arg(run, keys::ARG_VERSION)).map_err(|e| e.to_string());
    match run.contract.as_str() {
        INSTALL => {
            exact_args(
                run,
                &[
                    keys::ARG_PACKAGE,
                    keys::ARG_VERSION,
                    keys::ARG_ARTIFACT_TRANSFER,
                    keys::ARG_SIGNATURE_TRANSFER,
                ],
            )?;
            Ok(Operation::Install(InstallRequest {
                package: name()?,
                version: version()?,
                artifact_transfer: transfer_id(run, keys::ARG_ARTIFACT_TRANSFER)?,
                signature_transfer: transfer_id(run, keys::ARG_SIGNATURE_TRANSFER)?,
            }))
        }
        UNINSTALL => {
            exact_args(run, &[keys::ARG_PACKAGE, keys::ARG_VERSION])?;
            Ok(Operation::Uninstall(UninstallRequest {
                package: name()?,
                version: version()?,
            }))
        }
        _ => {
            exact_args(run, &[])?;
            Ok(Operation::List)
        }
    }
}

pub(super) fn failed(code: CapabilityFailureCode, message: String) -> CapabilityResult {
    CapabilityResult {
        outcome: CapabilityOutcome::Failed {
            code: Open::new(
                serde_json::to_value(code)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "refused".to_owned()),
                Some(code),
            ),
            message,
        },
    }
}

/// Runs `operation` on a blocking thread and turns its outcome into a wire result.
async fn execute(
    service: Arc<PackageService>,
    execution_id: Uuid,
    operation: Operation,
) -> CapabilityResult {
    let joined =
        tokio::task::spawn_blocking(move || -> Result<BTreeMap<String, String>, PackageError> {
            match operation {
                Operation::Install(req) => service.install(execution_id, req).map(|o| o.outputs()),
                Operation::Uninstall(req) => service.uninstall(req).map(|o| o.outputs()),
                Operation::List => service.list().map(|o| o.outputs()),
            }
        })
        .await;
    match joined {
        Ok(Ok(outputs)) => CapabilityResult {
            outcome: CapabilityOutcome::Succeeded { outputs },
        },
        Ok(Err(error)) => failed(CapabilityFailureCode::Refused, error.message()),
        Err(_) => failed(
            CapabilityFailureCode::Refused,
            "storage: the package task stopped unexpectedly".to_owned(),
        ),
    }
}

/// Serves a `run_capability` request, in the order the brief fixes.
pub async fn run_capability(ctx: &RequestCtx, run: &RunCapability) -> Message {
    // 1. A fenced journal refuses every capability.
    if ctx
        .journal
        .as_ref()
        .is_some_and(|journal| journal.corruption().is_some())
    {
        return refused(
            "journal_corrupt",
            RefusalCode::JournalCorrupt,
            "the action journal is corrupt; a local owner reset is required",
        );
    }
    // 2. The unit contracts are served by their own module; only the three package contracts
    // are served here.
    let contract = run.contract.as_str();
    if super::unit::is_unit_contract(contract) {
        return super::unit::run_unit(ctx, run).await;
    }
    if ![INSTALL, UNINSTALL, LIST].contains(&contract) {
        return refused(
            "contract_unknown",
            RefusalCode::ContractUnknown,
            format!(
                "this agent has no capability registry, so {} is unknown",
                run.contract
            ),
        );
    }
    // 3. Only version 1 exists.
    if run.version != 1 {
        return refused(
            "contract_version_unsupported",
            RefusalCode::ContractVersionUnsupported,
            format!("{contract} is only offered at version 1"),
        );
    }
    let mutating = contract != LIST;
    // 4. The owner's switch, applied again at execution.
    let surface = ctx.package_surface();
    let service = match &ctx.packages {
        Some(service) if (mutating && surface.mutate) || (!mutating && surface.list) => {
            service.clone()
        }
        _ => {
            return refused(
                "contract_unknown",
                RefusalCode::ContractUnknown,
                "package installation is not enabled in agent.toml",
            );
        }
    };
    if mutating {
        // 5. Package changes need a human.
        if run.authorization_kind == AuthorizationKind::Policy {
            return refused(
                "allow_risk_denied",
                RefusalCode::AllowRiskDenied,
                "package changes need a human authorization",
            );
        }
        // 6. And an authorization still in date.
        if run.expires_at <= ctx.clock.now() {
            return refused(
                "authorization_expired",
                RefusalCode::AuthorizationExpired,
                "the authorization for this package change has expired",
            );
        }
    }
    // 7. Arguments, before anything is journaled.
    let operation = match parse_operation(run) {
        Ok(operation) => operation,
        Err(message) => return bad_request(message),
    };
    // 8. No journal or no boot snapshot: nothing is accepted.
    let Some(journal) = ctx.journal.clone() else {
        return unavailable("the action journal is not available");
    };
    let Ok(snapshot) = crate::boot::boot_snapshot(&*ctx.fs).await else {
        return unavailable("the boot identity cannot be read, so no change is accepted");
    };
    // 9. Acceptance: deduplication, the mutation limiter and the durable `accepted` record.
    let plan_hash = run.plan_hash.to_string();
    let request = AcceptedRequest {
        execution_id: run.execution_id,
        contract,
        version: run.version,
        target: run.target.as_str(),
        plan_hash: &plan_hash,
        authorization_kind: run.authorization_kind,
        mutating,
    };
    let accepted = journal.accept(
        &request,
        ctx.clock.now(),
        &snapshot.boot_id.to_string(),
        snapshot.monotonic_ns,
    );
    match accepted {
        Ok(AcceptOutcome::Accepted) => {}
        Ok(AcceptOutcome::Refused(refusal)) => return Message::Refused(*refusal),
        Ok(AcceptOutcome::Duplicate(answer)) => {
            return match answer.state {
                ActionState::Finished { result } => Message::CapabilityResult(result),
                ActionState::Running => Message::Error(ErrorMessage {
                    code: "in_progress".to_owned(),
                    message: "this execution is still running".to_owned(),
                }),
                ActionState::NotFound | ActionState::OutcomeUnknown { .. } => {
                    Message::Error(ErrorMessage {
                        code: "outcome_unknown".to_owned(),
                        message: "this execution id was used before and its outcome is unknown; check agent.package.list and retry with a new id".to_owned(),
                    })
                }
            };
        }
        Err(error) => return unavailable(&format!("the action journal refused: {error}")),
    }
    // 10. In-process work: pid 0 on purpose.
    if let Err(error) = journal.started(run.execution_id, 0, 0) {
        return unavailable(&format!("the action journal refused: {error}"));
    }
    // 11. The work.
    let mut result = execute(service, run.execution_id, operation).await;
    if let CapabilityOutcome::Failed { code, message } = result.outcome {
        result.outcome = CapabilityOutcome::Failed {
            code,
            message: ctx.redactor.redact(&message),
        };
    }
    // 12. Durable result; a failed append is logged and the real result still goes out.
    if let Err(error) = journal.finished(run.execution_id, result.clone()) {
        tracing::error!(%error, execution = %run.execution_id, "the journal could not record a package result");
    }
    // 13.
    Message::CapabilityResult(result)
}
