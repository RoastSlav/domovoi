//! The closed meta-tool catalogue.

/// The closed set of tools the model is offered. No variant approves, authorizes, runs or writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MetaTool {
    /// List what can be done on the host.
    ListCapabilities,
    /// Show one capability in full.
    DescribeCapability,
    /// Read evidence from the host.
    RequestEvidence,
    /// Suggest actions for a person to review.
    ProposePlan,
    /// Ask the owner one question.
    AskUser,
    /// End the investigation with a report.
    Finish,
}

impl MetaTool {
    /// Every tool, in the order the model is shown them.
    pub const ALL: [MetaTool; 6] = [
        MetaTool::ListCapabilities,
        MetaTool::DescribeCapability,
        MetaTool::RequestEvidence,
        MetaTool::ProposePlan,
        MetaTool::AskUser,
        MetaTool::Finish,
    ];

    /// The tools that only look.
    pub const READ: [MetaTool; 3] = [
        MetaTool::ListCapabilities,
        MetaTool::DescribeCapability,
        MetaTool::RequestEvidence,
    ];

    /// The name the model calls.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::ListCapabilities => "list_capabilities",
            Self::DescribeCapability => "describe_capability",
            Self::RequestEvidence => "request_evidence",
            Self::ProposePlan => "propose_plan",
            Self::AskUser => "ask_user",
            Self::Finish => "finish",
        }
    }

    /// What the model reads about the tool.
    #[must_use]
    pub const fn description(&self) -> &'static str {
        match self {
            Self::ListCapabilities => {
                "List what can be done on the incident's host: the read-only evidence you can request and the actions you can propose, each with its effect class and whether it is available now. Nothing is run."
            }
            Self::DescribeCapability => {
                "Show one capability in full: what it does, its arguments as a JSON schema, its effect class and whether it is available. Nothing is run."
            }
            Self::RequestEvidence => {
                "Read evidence from the incident's host: containers, logs, events, the journal, the host overview, listening ports, disk health, earlier incidents, notes, or a read-only skill. This only looks; it changes nothing."
            }
            Self::ProposePlan => {
                "Suggest up to 3 actions for a person to review. A suggestion is not run and not approved: a person decides, outside this conversation. Each step names a capability from list_capabilities and its arguments; a step that does not validate comes back with the reason."
            }
            Self::AskUser => {
                "Ask the person who runs the server one short question when the answer decides what to do and the evidence cannot. Use it rarely: the investigation waits for the answer."
            }
            Self::Finish => {
                "End the investigation with your report. Call it once, when you know enough."
            }
        }
    }

    /// Looks a tool up by the name the model used.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tool| tool.name() == name)
    }
}

/// Which meta-tools one conversation is offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolSet {
    /// Follow-up and chat: look, never suggest.
    Read,
    /// An investigation: every tool, except `ask_user` unless `questions` is true.
    Investigate {
        /// Whether the model may ask the owner a question.
        questions: bool,
    },
}

impl ToolSet {
    /// The offered tools, in [`MetaTool::ALL`] order.
    #[must_use]
    pub fn tools(&self) -> Vec<MetaTool> {
        match self {
            Self::Read => MetaTool::READ.to_vec(),
            Self::Investigate { questions } => MetaTool::ALL
                .into_iter()
                .filter(|tool| *questions || *tool != MetaTool::AskUser)
                .collect(),
        }
    }

    /// Whether `tool` is offered.
    #[must_use]
    pub fn offers(&self, tool: MetaTool) -> bool {
        self.tools().contains(&tool)
    }
}
