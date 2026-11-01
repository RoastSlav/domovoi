//! LLM provider adapters.
//!
//! Implements the `LlmProvider` port for the Anthropic Messages API,
//! OpenAI-compatible Chat Completions and a CLI provider that runs through an
//! injected `ProcessExecutor`. See `ARCHITECTURE.md` §11 and ADR 0009.
//!
//! Adapters translate wire formats and report usage. Retries, fallback, budgets, prompts
//! and output parsing belong to the engine. API keys are [`secrecy::SecretString`]s,
//! exposed only while building a request header and removed from every error.
//!
//! The library installs no rustls crypto provider; the binary does that once at startup.

pub mod anthropic;
pub mod cli;
mod error;
mod factory;
mod http;
pub mod openai;
mod settings;

pub use cli::{CliOutput, CliPreset, PromptVia};
pub use error::{EXCERPT_CHARS, LlmBuildError, excerpt};
pub use factory::build;
pub use http::CONNECT_TIMEOUT;
pub use settings::{DEFAULT_TIMEOUT, Price, ProviderKind, ProviderSettings, cost_micros};
