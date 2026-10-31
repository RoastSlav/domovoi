//! `run_skill` and the read-skill tool: every request is re-validated against the local
//! catalogue and policy before the runner sees it (INV-3, INV-4).
//!
//! The order is fixed and each step stops the request:
//! 1. the skill is in the local catalogue, else `not_found`;
//! 2. its risk is in `[policy] allow_risk`, else `risk_not_allowed`;
//! 3. the arguments pass the local manifest, else `invalid_args`;
//! 4. the plan hash recomputed from the local manifest hash, this host and the validated
//!    arguments equals the request's, else `plan_mismatch`;
//! 5. only then does the runner execute it.
//!
//! Logs name the skill and the step, never argument values.

use domovoi_core::ports::SkillOutput;
use domovoi_core::{PlanHash, Risk, SkillName, ValidatedArgs};
use domovoi_proto::messages::{
    RunSkill, SkillFailure, SkillFailureCode, SkillOutcome, SkillSuccess, ToolFailureCode,
};
use domovoi_runner::{LoadedSkill, RunError};

use super::{RequestCtx, ToolFail};

fn fail(code: SkillFailureCode, message: impl Into<String>) -> SkillOutcome {
    SkillOutcome::Err(SkillFailure {
        code,
        message: message.into(),
    })
}

/// Why a skill was refused before running.
enum Refusal {
    NotFound,
    RiskNotAllowed(Risk),
    InvalidArgs(String),
}

/// Steps 1–3: the skill exists locally, its risk is allowed, its arguments are valid.
fn admit<'a>(
    ctx: &'a RequestCtx,
    name: &SkillName,
    args: &std::collections::BTreeMap<String, String>,
) -> Result<(&'a LoadedSkill, ValidatedArgs), Refusal> {
    let skill = ctx.catalog.get(name).ok_or(Refusal::NotFound)?;
    let risk = skill.manifest().risk;
    if !ctx.allow_risk.contains(&risk) {
        return Err(Refusal::RiskNotAllowed(risk));
    }
    let validated = skill
        .manifest()
        .validate_args(args)
        .map_err(|e| Refusal::InvalidArgs(e.to_string()))?;
    Ok((skill, validated))
}

fn redacted(ctx: &RequestCtx, mut output: SkillOutput) -> SkillOutput {
    output.stdout = ctx.redactor.redact(&output.stdout);
    output.stderr = ctx.redactor.redact(&output.stderr);
    output
}

/// Handles `run_skill`.
pub async fn run_skill(ctx: &RequestCtx, run: &RunSkill) -> SkillOutcome {
    let skill_name = run.skill.as_str();
    let (skill, validated) = match admit(ctx, &run.skill, &run.args) {
        Ok(admitted) => admitted,
        Err(Refusal::NotFound) => {
            tracing::warn!(
                skill = skill_name,
                step = "catalog",
                "refused a skill not in the local catalogue"
            );
            return fail(
                SkillFailureCode::NotFound,
                format!("no skill named {skill_name} on this host"),
            );
        }
        Err(Refusal::RiskNotAllowed(risk)) => {
            tracing::warn!(
                skill = skill_name,
                step = "policy",
                ?risk,
                "refused a skill whose risk is not allowed"
            );
            return fail(
                SkillFailureCode::RiskNotAllowed,
                format!("{skill_name} has risk {risk:?}, which this host's policy does not allow"),
            );
        }
        Err(Refusal::InvalidArgs(reason)) => {
            tracing::warn!(
                skill = skill_name,
                step = "args",
                "refused invalid arguments"
            );
            return fail(SkillFailureCode::InvalidArgs, reason);
        }
    };
    let expected = PlanHash::compute(&run.skill, skill.manifest_hash(), ctx.host_id, &validated);
    if expected != run.plan_hash {
        tracing::warn!(
            skill = skill_name,
            step = "plan_hash",
            "refused a plan that does not match the local manifest"
        );
        return fail(
            SkillFailureCode::PlanMismatch,
            "the approved plan does not match this host's skill; approve it again",
        );
    }
    tracing::info!(skill = skill_name, action = %run.action_id, "running an approved skill");
    match ctx.catalog.run_skill(skill, &validated, &*ctx.exec).await {
        Ok(output) => {
            tracing::info!(skill = skill_name, exit_code = ?output.exit_code, "skill finished");
            SkillOutcome::Ok(SkillSuccess::from(redacted(ctx, output)))
        }
        Err(RunError::TimedOut { .. }) => {
            tracing::warn!(skill = skill_name, "skill timed out");
            fail(SkillFailureCode::Timeout, format!("{skill_name} timed out"))
        }
        Err(err) => {
            tracing::warn!(skill = skill_name, %err, "skill could not run");
            fail(SkillFailureCode::Exec, err.to_string())
        }
    }
}

/// The `run_read_skill` tool: like `run_skill`, but only for skills whose risk is `read`, and
/// without a plan hash since nothing changes.
pub async fn run_read_skill(
    ctx: &RequestCtx,
    name: &SkillName,
    args: &std::collections::BTreeMap<String, String>,
) -> Result<(String, bool), ToolFail> {
    let skill = ctx.catalog.get(name).ok_or_else(|| {
        ToolFail::new(
            ToolFailureCode::InvalidArgs,
            format!("no skill named {name} on this host"),
        )
    })?;
    if skill.manifest().risk != Risk::Read {
        tracing::warn!(
            skill = name.as_str(),
            "refused a non-read skill as a read tool"
        );
        return Err(ToolFail::new(
            ToolFailureCode::InvalidArgs,
            format!("{name} changes the host; it needs an approved action"),
        ));
    }
    let (skill, validated) = admit(ctx, name, args).map_err(|refusal| match refusal {
        Refusal::NotFound => ToolFail::new(ToolFailureCode::InvalidArgs, "no such skill"),
        Refusal::RiskNotAllowed(_) => ToolFail::new(
            ToolFailureCode::Unavailable,
            "read skills are not allowed here",
        ),
        Refusal::InvalidArgs(reason) => ToolFail::new(ToolFailureCode::InvalidArgs, reason),
    })?;
    match ctx.catalog.run_skill(skill, &validated, &*ctx.exec).await {
        Ok(output) => {
            let mut text = output.stdout;
            if !output.stderr.is_empty() {
                text.push_str("\n[stderr]\n");
                text.push_str(&output.stderr);
            }
            Ok((text, output.truncated))
        }
        Err(RunError::TimedOut { .. }) => Err(ToolFail::new(
            ToolFailureCode::Timeout,
            "the skill timed out",
        )),
        Err(err) => Err(ToolFail::new(ToolFailureCode::Exec, err.to_string())),
    }
}
