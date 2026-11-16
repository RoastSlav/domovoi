//! Every message kind exchanged between hub and agent.
//!
//! A connection starts with either `enroll` (once, with a token) or `hello`, followed by
//! `challenge`, `challenge_response` and `welcome` (or `reject`). After `welcome` both
//! sides may send any message. Requests (`run_tool`, `run_skill`, `run_check`, `ping`)
//! are answered by a frame whose envelope `reply_to` is the request's `id`.

mod actions;
mod catalog;
mod handshake;
mod stream;
mod transfer;
mod work;

use serde_json::Value;

pub use actions::{
    ActionState, ActionStatusAnswer, ActionStatusAnswers, ActionStatusQuery, AuthorizationKind,
    CapabilityFailureCode, CapabilityOutcome, CapabilityResult, ExpectedSafety,
    OutcomeUnknownReason, RefusalCode, RefusalScope, Refused, RunCapability,
};
pub use catalog::{CatalogEntry, CatalogPage, GetCatalog};
pub use handshake::{
    AUTHORITATIVE_ABSENCE, AgentSettings, Challenge, ChallengeResponse, CheckInfo, CheckOverride,
    DockerReport, Enroll, Enrolled, Hello, HelperIdentity, HelperStatus, HostFacts, Reject,
    RejectReason, SessionId, SkillRunnerStatus, Welcome,
};
pub use stream::{
    AgentStatus, AttestSafety, ConfigUpdate, DigestFacts, ErrorMessage, Ping, Pong,
    SafetyAttestation, Signals, SignalsAck, Unsupported,
};
pub use transfer::{
    TransferBegin, TransferChunk, TransferEnd, TransferFailureCode, TransferKind, TransferOutcome,
    TransferResult,
};
pub use work::{
    CheckObservation, CheckResult, GetLogs, Logs, RunCheck, RunSkill, RunTool, SkillFailure,
    SkillFailureCode, SkillOutcome, SkillResult, SkillSuccess, ToolFailure, ToolFailureCode,
    ToolOutcome, ToolResult, ToolSuccess,
};

macro_rules! messages {
    ($( $(#[$doc:meta])* $variant:ident($ty:ty) = $kind:literal, )*) => {
        /// A typed protocol message. The envelope's `kind` selects the variant.
        #[derive(Debug, Clone, PartialEq)]
        pub enum Message {
            $( $(#[$doc])* $variant($ty), )*
            /// A kind this build does not know; the receiver replies `unsupported`.
            Unknown {
                /// The kind as received.
                kind: String,
            },
        }

        impl Message {
            /// Every kind this build knows, in declaration order.
            pub const KINDS: &'static [&'static str] = &[$($kind),*];

            /// The envelope `kind` for this message.
            #[must_use]
            pub fn kind(&self) -> &str {
                match self {
                    $( Self::$variant(_) => $kind, )*
                    Self::Unknown { kind } => kind,
                }
            }

            pub(crate) fn payload(&self) -> Result<Value, serde_json::Error> {
                match self {
                    $( Self::$variant(m) => serde_json::to_value(m), )*
                    Self::Unknown { .. } => Ok(Value::Object(serde_json::Map::new())),
                }
            }

            pub(crate) fn from_payload(kind: &str, payload: Value) -> Result<Self, serde_json::Error> {
                match kind {
                    $( $kind => serde_json::from_value(payload).map(Self::$variant), )*
                    other => Ok(Self::Unknown { kind: other.to_owned() }),
                }
            }
        }

        $(
            impl From<$ty> for Message {
                fn from(message: $ty) -> Self {
                    Self::$variant(message)
                }
            }
        )*
    };
}

messages! {
    /// `enroll`, agent → hub.
    Enroll(Enroll) = "enroll",
    /// `enrolled`, hub → agent.
    Enrolled(Enrolled) = "enrolled",
    /// `hello`, agent → hub.
    Hello(Hello) = "hello",
    /// `challenge`, hub → agent.
    Challenge(Challenge) = "challenge",
    /// `challenge_response`, agent → hub.
    ChallengeResponse(ChallengeResponse) = "challenge_response",
    /// `welcome`, hub → agent.
    Welcome(Welcome) = "welcome",
    /// `reject`, hub → agent.
    Reject(Reject) = "reject",
    /// `ping`, either side.
    Ping(Ping) = "ping",
    /// `pong`, either side.
    Pong(Pong) = "pong",
    /// `signals`, agent → hub.
    Signals(Signals) = "signals",
    /// `signals_ack`, hub → agent.
    SignalsAck(SignalsAck) = "signals_ack",
    /// `digest_facts`, agent → hub.
    DigestFacts(DigestFacts) = "digest_facts",
    /// `run_tool`, hub → agent.
    RunTool(RunTool) = "run_tool",
    /// `tool_result`, agent → hub.
    ToolResult(ToolResult) = "tool_result",
    /// `get_logs`, hub → agent.
    GetLogs(GetLogs) = "get_logs",
    /// `logs`, agent → hub.
    Logs(Logs) = "logs",
    /// `run_skill`, hub → agent.
    RunSkill(RunSkill) = "run_skill",
    /// `skill_result`, agent → hub.
    SkillResult(SkillResult) = "skill_result",
    /// `run_check`, hub → agent.
    RunCheck(RunCheck) = "run_check",
    /// `check_result`, agent → hub.
    CheckResult(CheckResult) = "check_result",
    /// `config_update`, hub → agent.
    ConfigUpdate(ConfigUpdate) = "config_update",
    /// `error`, either side.
    Error(ErrorMessage) = "error",
    /// `unsupported`, either side.
    Unsupported(Unsupported) = "unsupported",
    /// `agent_status`, agent → hub.
    AgentStatus(AgentStatus) = "agent_status",
    /// `run_capability`, hub → agent.
    RunCapability(RunCapability) = "run_capability",
    /// `capability_result`, agent → hub.
    CapabilityResult(CapabilityResult) = "capability_result",
    /// `refused`, agent → hub.
    Refused(Refused) = "refused",
    /// `action_status`, hub → agent.
    ActionStatus(ActionStatusQuery) = "action_status",
    /// `action_status_answers`, agent → hub.
    ActionStatusAnswers(ActionStatusAnswers) = "action_status_answers",
    /// `get_catalog`, hub → agent.
    GetCatalog(GetCatalog) = "get_catalog",
    /// `catalog_page`, agent → hub.
    CatalogPage(CatalogPage) = "catalog_page",
    /// `transfer_begin`, hub → agent.
    TransferBegin(TransferBegin) = "transfer_begin",
    /// `transfer_chunk`, hub → agent.
    TransferChunk(TransferChunk) = "transfer_chunk",
    /// `transfer_end`, hub → agent.
    TransferEnd(TransferEnd) = "transfer_end",
    /// `transfer_result`, agent → hub.
    TransferResult(TransferResult) = "transfer_result",
    /// `attest_safety`, hub → agent.
    AttestSafety(AttestSafety) = "attest_safety",
    /// `safety_attestation`, agent → hub.
    SafetyAttestation(SafetyAttestation) = "safety_attestation",
}
