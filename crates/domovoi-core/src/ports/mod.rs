//! Ports: the traits the engine and runtimes depend on (ARCHITECTURE.md §6).
//!
//! Every port is an object-safe `async_trait` trait that is `Send + Sync`, so it can be
//! held as `Arc<dyn Port>`. Adapters live in other crates; in-memory fakes and contract
//! suites live in `domovoi-testkit`.

pub mod agent_link;
pub mod chat;
pub mod clock;
pub mod hostfs;
pub mod llm;
pub mod process;
pub mod redact;
pub mod repos;

pub use agent_link::{
    AgentLink, CheckRecheck, IngestError, LinkError, Presence, SignalSink, SkillDispatch,
    SkillOutput, ToolOutput,
};
pub use chat::{
    ChatError, ChatGateway, InteractionSink, MessageRef, Notifier, OutboundMessage, ThreadRef,
};
pub use clock::Clock;
pub use hostfs::{FsError, HostFs};
pub use llm::{
    LlmError, LlmMessage, LlmProvider, LlmRequest, LlmResponse, Part, Role, StopReason, ToolSpec,
    Usage,
};
pub use process::{ProcessError, ProcessExecutor, ProcessOutput, ProcessSpec};
pub use redact::Redactor;
pub use repos::*;
