//! Ports: the traits the engine and runtimes depend on (ARCHITECTURE.md §6).
//!
//! Every port is an object-safe `async_trait` trait that is `Send + Sync`, so it can be
//! held as `Arc<dyn Port>`. Adapters live in other crates; in-memory fakes and contract
//! suites live in `domovoi-testkit`.

pub mod agent_link;
pub mod capability;
pub mod caps;
pub mod chat;
pub mod clock;
pub mod context;
pub mod experience;
pub mod hostfs;
pub mod knowledge;
pub mod live;
pub mod llm;
pub mod logs;
pub mod process;
pub mod reconcile;
pub mod redact;
pub mod repos;
pub mod store_health;
pub mod target;
pub mod uow;

pub use agent_link::{
    ActionAnswer, ActionAnswerEntry, ActionAnswers, AgentLink, AgentSnapshot,
    CATALOGUE_UNSUPPORTED, CheckRecheck, HostAvailability, IngestError, LinkError, Presence,
    SignalSink, SkillDispatch, SkillHandoff, SkillOutput, ToolOutput, UnknownReason,
};
pub use capability::{
    CapabilityRegistry, DISABLED_REASON, HelperState, HostView, HostWords, RunnerState,
};
pub use caps::{
    ActionCaps, AvailabilityProbe, CheckRerun, EvidenceCaps, HandoffRunner, HostObservation,
    HostRead, JournalStatus,
};
pub use chat::{
    ChatError, ChatGateway, InteractionSink, MessageRef, Notifier, OutboundMessage, ThreadRef,
};
pub use clock::Clock;
pub use context::ContextSource;
pub use experience::ExperienceRead;
pub use hostfs::{FsError, HostFs};
pub use knowledge::{FactFilter, FactHit, KnowledgeRead, ManagementCoverage, RelationFilter};
pub use live::{LiveEnd, LiveEvent, LivePublisher};
pub use llm::{
    AnswerSink, LlmCapability, LlmError, LlmMessage, LlmProvider, LlmRequest, LlmResponse, Part,
    Role, StopReason, ToolPhase, ToolSpec, Usage, UsageSource,
};
pub use logs::{LogLine, LogSource};
pub use process::{ProcessError, ProcessExecutor, ProcessOutput, ProcessSpec};
pub use reconcile::HostReconciler;
pub use redact::Redactor;
pub use repos::*;
pub use store_health::StoreHealth;
pub use target::TargetResolver;
pub use uow::*;
