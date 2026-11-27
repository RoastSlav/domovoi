//! Text and JSON output of a self-test report.

use std::fmt::Write as _;

use domovoi_core::identity::{FaultKind, PathFacts, PathFault, PathKind, SKILL_ACCOUNT};
use domovoi_core::isolation::{
    BoundaryReason, BoundaryState, CheckId, CheckResult, Outcome, SelftestReport, TierDecision,
};
use serde::Serialize;

/// A skill whose manifest does not run it as the skill account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AffectedSkill {
    /// The skill's name.
    pub name: String,
    /// What its manifest says, `None` when it has no `run_as`.
    pub run_as: Option<String>,
}

/// Picks the skills whose `run_as` is not the skill account from `(name, run_as)` pairs.
///
/// The caller reads the manifests through the catalog loader; enforcement is the runner's.
#[must_use]
pub fn affected_skills(skills: &[(String, Option<String>)]) -> Vec<AffectedSkill> {
    skills
        .iter()
        .filter(|(_, run_as)| run_as.as_deref() != Some(SKILL_ACCOUNT))
        .map(|(name, run_as)| AffectedSkill {
            name: name.clone(),
            run_as: run_as.clone(),
        })
        .collect()
}

fn facts_text(f: &PathFacts) -> String {
    let kind = match f.kind {
        Some(PathKind::Dir) => "directory",
        Some(PathKind::File) => "file",
        Some(PathKind::Socket) => "socket",
        None => "other",
    };
    let link = if f.symlink { " (symlink)" } else { "" };
    format!("{kind}{link} mode {:04o} owner {}:{}", f.mode, f.uid, f.gid)
}

const fn why_text(why: FaultKind) -> &'static str {
    match why {
        FaultKind::Missing => "missing",
        FaultKind::WrongType => "wrong type",
        FaultKind::Symlink => "symlink",
        FaultKind::Mode => "wrong mode",
        FaultKind::Owner => "wrong owner",
        FaultKind::Group => "wrong group",
        FaultKind::WriteBitInTree => "forbidden entry in tree",
        FaultKind::Unreadable => "unreadable",
    }
}

/// One fault as a line: the path, what was found and what is expected.
#[must_use]
pub fn render_fault(fault: &PathFault) -> String {
    let found = fault
        .found
        .as_ref()
        .map_or_else(|| "nothing readable".to_owned(), facts_text);
    format!(
        "{} ({}): found {found}; expected {}",
        fault.path.display(),
        why_text(fault.why),
        facts_text(&fault.expected)
    )
}

/// One result: `ST-10 fail path_mode_wrong ...`, then a line per fault.
#[must_use]
pub fn render_result(result: &CheckResult) -> String {
    let id = result.id;
    let mut out = match &result.outcome {
        Outcome::Pass => format!("{id} pass"),
        Outcome::Fail { token, detail } => format!("{id} fail {token} {detail}"),
        Outcome::Warn { token, detail } => format!("{id} warn {token} {detail}"),
        Outcome::Info { detail } => format!("{id} info {detail}"),
        Outcome::Nd { reason } => format!("{id} nd {reason}"),
    };
    for fault in &result.faults {
        let _ = write!(out, "\n  {}", render_fault(fault));
    }
    out
}

fn snake<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn boundary_text(boundary: &BoundaryState) -> String {
    match boundary {
        BoundaryState::InEffect => "boundary: in effect".to_owned(),
        BoundaryState::NotInEffect { reason } => {
            let why = match reason {
                BoundaryReason::AgentIsRoot => "the agent runs as root".to_owned(),
                BoundaryReason::AgentNotDomovoiAgent { uid } => {
                    format!("uid {uid} is not the agent account")
                }
                BoundaryReason::AgentGroupsUnexpected { gids } => {
                    format!("unexpected groups {gids:?}")
                }
                BoundaryReason::AccountMissing => "the agent account is missing".to_owned(),
            };
            format!("boundary: {} ({why})", BoundaryState::NOT_IN_EFFECT_LABEL)
        }
    }
}

/// The listing row SK-01 for one skill that does not run as the skill account.
///
/// Not a check: it never gates the platform tier. A named `run_as` is `warn` with the token
/// `run_as_unsupported` on an agent with the boundary in effect, where the runner refuses it;
/// a legacy root agent honours it, and a skill without `run_as` is only noted.
#[must_use]
pub fn skill_row(skill: &AffectedSkill, legacy_root: bool) -> String {
    let who = skill.run_as.as_deref().unwrap_or("the agent (no run_as)");
    let head = format!("skill {} runs as {who}, not {SKILL_ACCOUNT}", skill.name);
    match (&skill.run_as, legacy_root) {
        (_, true) => format!("SK-01 info {head}: honoured by the legacy path"),
        (None, false) => format!("SK-01 info {head}"),
        (Some(run_as), false) => format!(
            "SK-01 warn run_as_unsupported {head} (names run_as {run_as}; only {SKILL_ACCOUNT} is supported on this host)"
        ),
    }
}

/// The command whose run decides the tier of an installed host.
pub const VERDICT_COMMAND: &str = "sudo domovoi agent selftest";

/// The required checks a run without root could not judge, when that is the only thing in the
/// way of a verdict. `None` means the report decides the tier: it ran as root, or a check this
/// account could make really failed.
///
/// ST-01 failing for an account that is neither root nor the agent describes the account the
/// command ran as, so it does not count as a failure of the install.
#[must_use]
pub fn unjudged_checks(report: &SelftestReport, decision: &TierDecision) -> Option<Vec<CheckId>> {
    let ran_without_root = report
        .results
        .iter()
        .any(|r| matches!(&r.outcome, Outcome::Nd { reason } if reason.starts_with("needs_root")));
    if !ran_without_root || decision.no_systemd || decision.blocking.is_empty() {
        return None;
    }
    let other_account = matches!(
        report.boundary,
        BoundaryState::NotInEffect {
            reason: BoundaryReason::AgentNotDomovoiAgent { .. }
        }
    );
    let judged_as_failed = decision.blocking.iter().any(|id| {
        report
            .result(*id)
            .is_some_and(|r| r.outcome.is_fail() && !(*id == CheckId::St01 && other_account))
    });
    (!judged_as_failed).then(|| decision.blocking.clone())
}

fn verdict_lines(report: &SelftestReport, decision: &TierDecision) -> Vec<String> {
    let list = |ids: &[CheckId]| {
        ids.iter()
            .map(|i| i.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    if let Some(unjudged) = unjudged_checks(report, decision) {
        return vec![
            format!(
                "tier: not decided: this account cannot run {}",
                list(&unjudged)
            ),
            format!(
                "verdict: run `{VERDICT_COMMAND}`; it runs those checks as root and the agent checks as domovoi-agent"
            ),
        ];
    }
    let mut lines = vec![
        format!("tier: {}", decision.tier.as_str()),
        format!(
            "decision: skills {}; helper reads {}; helper writes {}",
            snake(&decision.skills),
            snake(&decision.helper_read),
            snake(&decision.helper_write)
        ),
    ];
    if !decision.blocking.is_empty() {
        lines.push(format!("blocking: {}", list(&decision.blocking)));
    }
    if let Some(notice) = decision.notice {
        lines.push(format!("notice: {}", notice.text()));
    }
    lines
}

/// The whole report as text: a line per check, faults indented below it, then the platform, the
/// boundary label, the tier decision and the skills that do not run as the skill account.
#[must_use]
pub fn render_text(
    report: &SelftestReport,
    decision: &TierDecision,
    affected: &[AffectedSkill],
) -> String {
    let mut lines: Vec<String> = report.results.iter().map(render_result).collect();
    let p = &report.platform;
    lines.push(format!("platform: {} {} {}", p.os_id, p.version_id, p.arch));
    lines.push(format!(
        "systemd: {}",
        report.systemd_version.as_deref().unwrap_or("none")
    ));
    lines.push(boundary_text(&report.boundary));
    lines.extend(verdict_lines(report, decision));
    let legacy_root = matches!(
        report.boundary,
        BoundaryState::NotInEffect {
            reason: BoundaryReason::AgentIsRoot
        }
    );
    lines.extend(affected.iter().map(|skill| skill_row(skill, legacy_root)));
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// The report as the JSON `selftest --json` prints.
pub fn render_json(report: &SelftestReport) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(report)
}

/// The process exit code: 1 when any check failed, else 0. `nd` is neither.
#[must_use]
pub fn exit_code(report: &SelftestReport) -> i32 {
    i32::from(report.any_failed())
}
