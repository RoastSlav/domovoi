//! The simulator: re-evaluates recorded Execution steps under a draft, a stored version or
//! inline rules, and shows each decision next to what was recorded. It never writes.

use domovoi_core::ExecutionId;
use domovoi_core::policy_store::{ShadowFilter, ShadowRecord, ShadowStatus};
use domovoi_core::ports::Page;
use domovoi_engine::policy::{Rule, RuleSelection, evaluate, evaluate_shadow};
use domovoi_engine::policy_shadow::{
    CandidateDoc, Comparison, DecisionEnvelope, Replayed, SimulationSummary, Verdict, build,
    compare_with_record, replay, summarise,
};
use jiff::Timestamp;

use super::policy::{PolicyService, check_rules};
use super::policy_types::PolicyCommandError;

/// Explicit steps a simulation takes at most.
pub const MAX_EXPLICIT_IDS: usize = 100;
/// Rows a simulation takes by default.
pub const DEFAULT_LIMIT: u16 = 200;
/// Rows a simulation takes at most.
pub const MAX_LIMIT: u16 = 500;

/// Which rules a simulation evaluates.
#[derive(Debug, Clone)]
pub enum RulesSource {
    /// The stored draft.
    Draft,
    /// A published version by number.
    Version(u32),
    /// Rules sent with the request, validated like a draft save.
    Inline(Vec<Rule>),
}

/// What the evaluation is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputsMode {
    /// Exactly what was recorded with each step.
    Recorded,
    /// The hub's view now, rebuilt from the stored step, at the current instant.
    Current,
}

/// Which of a row's two decisions to compute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionChoice {
    /// The enforced rules.
    Enforced,
    /// The shadow rules.
    Shadow,
    /// Both.
    Both,
}

/// A simulation to run.
#[derive(Debug, Clone)]
pub struct SimulationRequest {
    /// Whose rules.
    pub rules: RulesSource,
    /// Which decisions.
    pub selection: SelectionChoice,
    /// Recorded or current inputs.
    pub inputs: InputsMode,
    /// Which recorded rows; also applied to each explicit step, which is dropped when it fails.
    pub filter: ShadowFilter,
    /// Explicit steps; at most `MAX_EXPLICIT_IDS`.
    pub execution_ids: Vec<ExecutionId>,
    /// How many rows to take; 0 reads as the default, and the maximum is `MAX_LIMIT`.
    pub limit: u16,
}

/// One row with its re-run decisions.
#[derive(Debug, Clone)]
pub struct SimulationItem {
    /// The recorded row.
    pub record: ShadowRecord,
    /// The enforced-rules decision, when asked for.
    pub enforced: Option<Replayed>,
    /// The shadow-rules decision, when asked for.
    pub shadow: Option<Replayed>,
}

/// What a simulation found.
#[derive(Debug, Clone)]
pub struct SimulationResult {
    /// Which rules were simulated, in words.
    pub label: String,
    /// The rows.
    pub items: Vec<SimulationItem>,
    /// Totals over every re-run.
    pub summary: SimulationSummary,
    /// Whether more rows matched than were taken.
    pub truncated: bool,
    /// Standing notes.
    pub notes: Vec<&'static str>,
}

impl PolicyService {
    /// Replays or re-evaluates historical Execution steps. No password, no write.
    pub async fn simulate(
        &self,
        request: SimulationRequest,
    ) -> Result<SimulationResult, PolicyCommandError> {
        let (rules, label) = self.resolve_rules(request.rules).await?;
        let (rows, truncated) = self
            .load_rows(&request.filter, request.execution_ids, request.limit)
            .await?;
        let selections: &[RuleSelection] = match request.selection {
            SelectionChoice::Enforced => &[RuleSelection::Enforced],
            SelectionChoice::Shadow => &[RuleSelection::Shadow],
            SelectionChoice::Both => &[RuleSelection::Enforced, RuleSelection::Shadow],
        };
        let now = self.clock.now();
        let mut items = Vec::with_capacity(rows.len());
        let mut all = Vec::new();
        for row in rows {
            let mut item = SimulationItem {
                record: row,
                enforced: None,
                shadow: None,
            };
            for selection in selections {
                let replayed = match request.inputs {
                    InputsMode::Recorded => replay(&rules, &item.record, *selection),
                    InputsMode::Current => {
                        self.replay_current(&rules, &item.record, *selection, now)
                            .await
                    }
                };
                all.push(replayed.clone());
                match selection {
                    RuleSelection::Enforced => item.enforced = Some(replayed),
                    RuleSelection::Shadow => item.shadow = Some(replayed),
                }
            }
            items.push(item);
        }
        Ok(SimulationResult {
            label,
            items,
            summary: summarise(&all),
            truncated,
            notes: self.notes().await,
        })
    }

    /// What `rules` would do to the newest evaluated rows, enforced rules over recorded inputs.
    pub(super) async fn replay_recent(
        &self,
        rules: &[Rule],
        limit: u16,
    ) -> Result<SimulationSummary, PolicyCommandError> {
        let filter = ShadowFilter {
            status: Some(ShadowStatus::Evaluated),
            ..ShadowFilter::default()
        };
        let page = self
            .repo
            .shadow_records(&filter, &Page::first(limit))
            .await?;
        let replayed: Vec<Replayed> = page
            .items
            .iter()
            .map(|row| replay(rules, row, RuleSelection::Enforced))
            .collect();
        Ok(summarise(&replayed))
    }

    async fn resolve_rules(
        &self,
        source: RulesSource,
    ) -> Result<(Vec<Rule>, String), PolicyCommandError> {
        match source {
            RulesSource::Draft => {
                let draft = self
                    .repo
                    .draft()
                    .await?
                    .ok_or(PolicyCommandError::NoDraft)?;
                let label = format!("draft revision {}", draft.revision);
                Ok((self.rules_of(draft.id).await?, label))
            }
            RulesSource::Version(number) => {
                let version = self
                    .repo
                    .version(number)
                    .await?
                    .ok_or(PolicyCommandError::NotFound)?;
                Ok((
                    self.rules_of(version.id).await?,
                    format!("version {number}"),
                ))
            }
            RulesSource::Inline(rules) => {
                check_rules(&rules)?;
                Ok((rules, "inline rules".to_owned()))
            }
        }
    }

    async fn load_rows(
        &self,
        filter: &ShadowFilter,
        mut ids: Vec<ExecutionId>,
        limit: u16,
    ) -> Result<(Vec<ShadowRecord>, bool), PolicyCommandError> {
        if !ids.is_empty() {
            let truncated = ids.len() > MAX_EXPLICIT_IDS;
            ids.truncate(MAX_EXPLICIT_IDS);
            let mut rows = Vec::with_capacity(ids.len());
            for id in ids {
                if let Some(row) = self.repo.shadow(id).await?
                    && filter.matches(&row)
                {
                    rows.push(row);
                }
            }
            return Ok((rows, truncated));
        }
        let limit = if limit == 0 { DEFAULT_LIMIT } else { limit }.min(MAX_LIMIT);
        let page = self
            .repo
            .shadow_records(filter, &Page::first(limit))
            .await?;
        Ok((page.items, page.next_cursor.is_some()))
    }

    /// One row's decision from the hub's view now, next to what the row recorded.
    async fn replay_current(
        &self,
        rules: &[Rule],
        row: &ShadowRecord,
        selection: RuleSelection,
        now: Timestamp,
    ) -> Replayed {
        let not_replayable = || Replayed {
            simulated: DecisionEnvelope::Error {
                code: "not_replayable".to_owned(),
            },
            baseline: None,
            comparison: Comparison::NotReplayable,
        };
        if row.status != ShadowStatus::Evaluated {
            return not_replayable();
        }
        let Some(candidate) = serde_json::from_str::<CandidateDoc>(&row.candidate)
            .ok()
            .and_then(|doc| doc.to_candidate())
        else {
            return not_replayable();
        };
        let built = build(&candidate, rules, &*self.env, now).await;
        let (Verdict::Evaluate, Some(request)) = (built.verdict, built.request) else {
            return not_replayable();
        };
        let result = match selection {
            RuleSelection::Enforced => evaluate(rules, &request),
            RuleSelection::Shadow => evaluate_shadow(rules, &request),
        };
        compare_with_record(row, selection, DecisionEnvelope::from_result(&result))
    }
}
