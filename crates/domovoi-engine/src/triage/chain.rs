//! The one provider-chain loop shared by triage, follow-up and chat: try the primary, then
//! each fallback, rebuild an attempt once per capability a provider refuses, and collect the
//! reason every failed provider gave.

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::ports::{LlmCapability, LlmError, LlmMessage, LlmProvider, Part, Role};

use super::Shared;
use super::agent::{self, ToolUse};
use super::meta::EvidenceRecord;
use super::run::provider_name;

/// Why one provider's attempt failed.
pub(crate) enum AttemptError {
    /// The provider failed.
    Provider(LlmError),
    /// The answer could not be used.
    Invalid(String),
}

impl From<LlmError> for AttemptError {
    fn from(err: LlmError) -> Self {
        Self::Provider(err)
    }
}

impl std::fmt::Display for AttemptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Provider(err) => write!(f, "{err}"),
            Self::Invalid(why) => f.write_str(why),
        }
    }
}

/// What every attempt of one run shares: the budget and deadline, the recorded tool use and
/// the evidence records.
pub(crate) struct ChainState {
    /// The output allowance and deadline, shared across the providers of one run.
    pub(crate) run: agent::RunBudget,
    /// The tool calls made so far.
    pub(crate) used: Vec<ToolUse>,
    /// Every parsed read made so far.
    pub(crate) evidence: Vec<EvidenceRecord>,
}

impl ChainState {
    /// A fresh state under the run budget of `shared`.
    pub(crate) fn new(shared: &Shared) -> Self {
        Self {
            run: agent::RunBudget::for_run(shared),
            used: Vec::new(),
            evidence: Vec::new(),
        }
    }

    /// A fresh state whose model requests stream their text to `sink`.
    pub(crate) fn streaming(
        shared: &Shared,
        sink: Arc<dyn domovoi_core::ports::AnswerSink>,
    ) -> Self {
        Self {
            run: agent::RunBudget::for_run(shared).with_sink(sink),
            used: Vec::new(),
            evidence: Vec::new(),
        }
    }
}

/// One path's try with one provider.
#[async_trait]
pub(crate) trait Attempt: Send + Sync {
    /// What a successful attempt produces.
    type Output: Send;

    /// One provider's whole try: rebuilds the messages from the top and runs them. It is
    /// called again after a capability rebuild.
    async fn attempt(
        &self,
        provider: &Arc<dyn LlmProvider>,
        state: &mut ChainState,
    ) -> Result<Self::Output, AttemptError>;
}

/// Tries the primary, then each fallback, and returns the first success, or every
/// provider's redacted reason in order.
pub(crate) async fn run_chain<A: Attempt>(
    shared: &Shared,
    state: &mut ChainState,
    attempt: &A,
) -> Result<A::Output, Vec<String>> {
    let chain = shared.chain();
    let mut reasons: Vec<String> = Vec::new();
    for (i, provider) in chain.iter().enumerate() {
        // A provider that refuses a capability changes how the attempt is built (the opening
        // gathers tool output itself when tools are gone), so the attempt is rebuilt from
        // the top once per distinct capability; the reads of a rebuilt attempt are
        // forgotten together with their tool use.
        let used_before = state.used.len();
        let evidence_before = state.evidence.len();
        let mut handled: Vec<LlmCapability> = Vec::new();
        let why = loop {
            match attempt.attempt(provider, state).await {
                Ok(output) => return Ok(output),
                Err(AttemptError::Provider(LlmError::CapabilityRejected {
                    capability, ..
                })) if !handled.contains(&capability) => {
                    handled.push(capability);
                    state.used.truncate(used_before);
                    state.evidence.truncate(evidence_before);
                    tracing::info!(
                        provider = provider.id(),
                        %capability,
                        "the provider refused a capability; rebuilding the run without it",
                    );
                }
                Err(err) => {
                    break shared
                        .deps
                        .redactor
                        .redact(&format!("{} failed: {err}", provider_name(provider)));
                }
            }
        };
        if i + 1 < chain.len() {
            tracing::info!(
                provider = provider.id(),
                reason = why.as_str(),
                "the model falls back to the next provider",
            );
        }
        reasons.push(why);
    }
    Err(reasons)
}

/// The messages of a conversation turn: the opening, the remembered turns, the gathered read
/// output when the provider has no tools, and the new question.
pub(crate) fn thread_messages(
    opening: &str,
    history: &[LlmMessage],
    gathered: Option<&str>,
    question: &str,
) -> Vec<LlmMessage> {
    let text = |role, text: &str| LlmMessage {
        role,
        parts: vec![Part::Text(text.to_owned())],
    };
    let mut messages = vec![text(Role::User, opening)];
    messages.extend(history.iter().cloned());
    if let Some(gathered) = gathered.filter(|g| !g.is_empty()) {
        messages.push(text(Role::User, gathered));
    }
    messages.push(text(Role::User, question));
    messages
}

#[cfg(test)]
mod tests {
    use domovoi_core::ports::{LlmMessage, Part, Role};

    use super::thread_messages;

    fn text(role: Role, text: &str) -> LlmMessage {
        LlmMessage {
            role,
            parts: vec![Part::Text(text.to_owned())],
        }
    }

    #[test]
    fn thread_messages_keeps_the_conversation_shape() {
        let history = vec![
            text(Role::User, "earlier question"),
            text(Role::Assistant, "earlier answer"),
        ];
        let with = thread_messages("opening", &history, Some("gathered"), "question");
        assert_eq!(
            with,
            vec![
                text(Role::User, "opening"),
                text(Role::User, "earlier question"),
                text(Role::Assistant, "earlier answer"),
                text(Role::User, "gathered"),
                text(Role::User, "question"),
            ]
        );
        let without = thread_messages("opening", &history, None, "question");
        assert_eq!(without.len(), 4);
        assert_eq!(without.last(), Some(&text(Role::User, "question")));
        let empty = thread_messages("opening", &[], Some(""), "question");
        assert_eq!(
            empty,
            vec![text(Role::User, "opening"), text(Role::User, "question")],
            "an empty gathered output adds no message"
        );
    }
}
