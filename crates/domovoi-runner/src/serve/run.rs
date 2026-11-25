//! Handling a `Run` request: validate against the files the runner can see, then run.

use std::sync::Arc;

use domovoi_core::action::ImplHash;
use domovoi_core::identity::SKILL_ACCOUNT;
use domovoi_core::ports::Redactor;
use domovoi_core::skill::ArgError;
use domovoi_core::skill_runner::{RefusalCode, RunReply, RunRequest, RunnerReply, SkillSource};
use jiff::SignedDuration;
use tokio::io::{AsyncRead, AsyncReadExt};

use super::ServeContext;
use super::connection::{Answer, refused};
use super::roots::root_for;
use crate::catalog::checks::{FolderError, resolve_in_root};
use crate::{RunCaps, RunEnd, RunError, SandboxExecutor, SkillCatalog};

/// The runner holds no secrets; the agent redacts what comes back.
struct PassThrough;

impl Redactor for PassThrough {
    fn redact(&self, text: &str) -> String {
        text.to_owned()
    }
}

/// Resolves, checks and runs one skill, in the order of the protocol description.
pub(super) async fn handle<R>(request: RunRequest, reader: &mut R, ctx: &ServeContext) -> Answer
where
    R: AsyncRead + Unpin + Send,
{
    let not_found = || {
        refused(
            RefusalCode::SkillNotFound,
            "no such skill folder under the configured root",
        )
    };
    let Some(root) = root_for(&request.source, &ctx.skills_root, &ctx.packages_root) else {
        return not_found();
    };
    match resolve_in_root(&root, request.skill.as_str()) {
        Ok(_) => {}
        Err(FolderError::Missing | FolderError::Outside) => return not_found(),
    }
    let (catalog, skill) = match SkillCatalog::load_one(&root, &request.skill) {
        Ok(loaded) => loaded,
        Err(issue) => return refused(RefusalCode::SkillInvalid, &issue.message),
    };
    let recomputed = ImplHash::from_parts(skill.manifest_hash(), skill.entry_script_digest());
    if recomputed != request.impl_hash {
        return refused(
            RefusalCode::ImplHashMismatch,
            "the skill on disk differs from the one the agent hashed",
        );
    }
    let validated = match skill.manifest().validate_args(&request.args) {
        Ok(validated) => validated,
        Err(err) => return refused(RefusalCode::ArgsInvalid, &args_detail(&err)),
    };
    if !run_as_allowed(&request.source, skill.manifest().run_as.as_deref()) {
        return refused(
            RefusalCode::RunAsUnsupported,
            "the manifest names a run_as this host does not support",
        );
    }
    let caps = RunCaps {
        timeout: Some(SignedDuration::from_secs(i64::from(request.timeout_secs))),
        max_output_bytes: Some(request.max_output_bytes),
    };
    let executor = SandboxExecutor::new(Arc::new(PassThrough));
    // EOF, any byte or an error on the connection cancels the run.
    let watcher = async {
        let mut byte = [0_u8; 1];
        let _ = reader.read(&mut byte).await;
    };
    tracing::info!(execution = %request.execution_id, skill = %request.skill, "running a skill");
    match catalog
        .run_skill_cancellable(&skill, &validated, &executor, caps, watcher)
        .await
    {
        Ok(run) => match run.end {
            RunEnd::Cancelled => {
                tracing::info!(execution = %request.execution_id, "run cancelled by the peer");
                Answer::Silent
            }
            RunEnd::Finished(out) => Answer::Reply(RunnerReply::Run(RunReply {
                exit_code: out.exit_code,
                signal: out.signal,
                stdout: out.stdout,
                stderr: out.stderr,
                truncated: out.truncated,
                timed_out: out.timed_out,
                duration_ms: out.duration_ms,
                argv_hash: hex::encode(run.argv_hash),
            })),
        },
        Err(RunError::EntryChanged(detail)) => refused(RefusalCode::EntryChanged, &detail),
        Err(RunError::Process(err)) => refused(RefusalCode::SpawnFailed, &err.to_string()),
        Err(err @ RunError::UnknownSkill(_)) => {
            refused(RefusalCode::SkillInvalid, &err.to_string())
        }
        Err(err @ RunError::TimedOut { .. }) => refused(RefusalCode::SpawnFailed, &err.to_string()),
    }
}

/// A skills-directory skill may name the skill account or nothing; a package skill nothing.
fn run_as_allowed(source: &SkillSource, run_as: Option<&str>) -> bool {
    match (source, run_as) {
        (_, None) => true,
        (SkillSource::SkillsDir, Some(name)) => name == SKILL_ACCOUNT,
        (SkillSource::Package { .. }, Some(_)) => false,
    }
}

/// Names the parameter at fault and never a value.
fn args_detail(err: &ArgError) -> String {
    match err {
        ArgError::Missing(param) => format!("required argument `{param}` is missing"),
        ArgError::Unknown(param) => format!("argument `{param}` is not declared by the skill"),
        ArgError::Invalid { param, .. } => format!("argument `{param}` is not valid"),
    }
}
