//! The model's closed meta-tools: the investigation loop and the dispatch of each call.
//!
//! The model is offered only the [`MetaTool`]s of one [`ToolSet`]. Reads go through the one
//! evidence boundary as `request_evidence`; a suggested action goes through `propose_plan`,
//! where the plan validator decides which steps are staged and tells the model why the others
//! were refused. Staged steps are only data handed back to the caller: nothing here stores a
//! step, approves anything or reaches an agent other than through a read.

mod capabilities;
mod dispatch;
mod evidence;
mod finish;
mod loop_run;
mod record;

use std::sync::Arc;

use domovoi_core::meta::{MetaTool, ToolSet};
use domovoi_core::ports::{LlmError, LlmProvider, ToolSpec};
use domovoi_core::{ReadToolCall, SkillSummary, TriageReport};

use super::Shared;
use super::tools::{EvidenceClass, Scope};
use crate::plan::{PlanScope, PlannedStep};

pub(crate) use loop_run::{converse_read, investigate};

/// The most questions one operation may put to the person who runs the server.
pub(crate) const MAX_QUESTIONS_PER_OPERATION: u32 = 2;

/// The system prompt for providers that call tools, with the `ask_user` paragraph.
pub const META_SYSTEM_PROMPT: &str = include_str!("../../../prompts/triage_meta_system.md");

/// The marker line around the paragraph that describes `ask_user`.
const ASK_USER_MARK: &str = "<!-- ask_user -->";

/// The system prompt for an investigation offered `tools`: the `ask_user` paragraph is kept
/// only when that tool is offered, and the marker lines never reach the model.
pub(crate) fn meta_prompt(tools: ToolSet) -> String {
    let keep = tools.offers(MetaTool::AskUser);
    let mut inside = false;
    let mut out: Vec<&str> = Vec::new();
    for line in META_SYSTEM_PROMPT.lines() {
        if line.trim() == ASK_USER_MARK {
            inside = !inside;
            continue;
        }
        if inside && !keep {
            continue;
        }
        out.push(line);
    }
    let mut text = out.join("\n");
    if !keep {
        // The paragraph sat between two blank lines; leave one.
        text = text.replace("\n\n\n", "\n\n");
    }
    text.push('\n');
    text
}

/// The tools offered to a provider that supports them: the meta-tools of `set`, in
/// [`MetaTool::ALL`] order, and nothing else.
#[must_use]
pub fn offered_tools(set: ToolSet) -> Vec<ToolSpec> {
    set.tools().into_iter().map(ToolSpec::from).collect()
}

/// What one investigation loop may do.
pub(crate) struct LoopScope {
    /// The tools offered.
    pub(crate) tools: ToolSet,
    /// What a plan is checked against.
    pub(crate) plan: PlanScope,
    /// Reads the operation's evidence budget still allows; `None` is unlimited.
    pub(crate) evidence_left: Option<u32>,
    /// Questions the operation already asked.
    pub(crate) questions_asked: u32,
}

/// One parsed read the loop made, with what it came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EvidenceRecord {
    /// The read.
    pub(crate) call: ReadToolCall,
    /// What it came to.
    pub(crate) class: EvidenceClass,
    /// The first 280 characters of the redacted text.
    pub(crate) summary: String,
    /// The redacted text, at most 16 KiB.
    pub(crate) output: String,
}

/// How an investigation loop ended.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum LoopEnd {
    /// The model delivered its report.
    Report(TriageReport),
    /// The model asked the person who runs the server a question.
    Asked(String),
}

/// What an investigation loop produced. Its evidence is kept by the caller, so reads made
/// before a failure stay recorded.
#[derive(Debug, Clone)]
pub(crate) struct LoopOutput {
    /// How it ended.
    pub(crate) end: LoopEnd,
    /// The staged plan steps, at most three.
    pub(crate) plan: Vec<PlannedStep>,
    /// The "Proposals not offered" lines: refusals the model never corrected.
    pub(crate) dropped: Vec<String>,
}

/// Why one provider's attempt failed.
pub(crate) enum Failure {
    /// The provider failed.
    Provider(LlmError),
    /// The answer could not be used.
    Invalid(String),
}

impl From<LlmError> for Failure {
    fn from(err: LlmError) -> Self {
        Self::Provider(err)
    }
}

/// The fixed context of one conversation: the service, the provider, the system prompt, the
/// read scope and the skills the host catalogues.
pub(crate) struct Ctx<'a> {
    pub(crate) shared: &'a Shared,
    pub(crate) provider: &'a Arc<dyn LlmProvider>,
    pub(crate) system: &'a str,
    pub(crate) scope: Scope,
    pub(crate) skills: &'a [SkillSummary],
}

#[cfg(test)]
mod tests {
    use domovoi_core::meta::ToolSet;

    use super::{ASK_USER_MARK, META_SYSTEM_PROMPT, meta_prompt};

    #[test]
    fn the_meta_prompt_offers_ask_user_only_when_it_is_offered() {
        assert_eq!(
            META_SYSTEM_PROMPT.matches(ASK_USER_MARK).count(),
            2,
            "one marked paragraph"
        );
        let with = meta_prompt(ToolSet::Investigate { questions: true });
        let without = meta_prompt(ToolSet::Investigate { questions: false });
        let paragraph = "ask them one short question with `ask_user`";
        assert!(with.contains(paragraph), "{with}");
        assert!(!without.contains(paragraph), "{without}");
        for text in [&with, &without] {
            assert!(
                !text.contains(ASK_USER_MARK),
                "the marker never reaches the model"
            );
            assert!(!text.contains("\n\n\n"), "no doubled blank line");
        }
    }
}
