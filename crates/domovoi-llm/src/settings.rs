//! What a provider needs to be built, independent of how it was configured.

use domovoi_core::ports::Usage;
use jiff::SignedDuration;
use secrecy::SecretString;
use url::Url;

use crate::cli::CliPreset;

/// The total time one request may take when the caller sets nothing else.
pub const DEFAULT_TIMEOUT: SignedDuration = SignedDuration::from_secs(120);

/// Which adapter talks to the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    /// The Anthropic Messages API.
    Anthropic,
    /// Any server speaking the OpenAI Chat Completions API (Ollama, LM Studio, OpenRouter).
    OpenaiCompatible,
    /// A local coding-agent CLI run in print mode.
    Cli,
}

/// Everything [`build`](crate::build) needs for one provider. The hub fills it from
/// `hub.toml`; the API key stays a [`SecretString`] and prints as redacted.
#[derive(Debug, Clone)]
pub struct ProviderSettings {
    /// Which adapter to build.
    pub kind: ProviderKind,
    /// Model name; required for the HTTP adapters.
    pub model: Option<String>,
    /// API base URL; required for `OpenaiCompatible`, optional for `Anthropic`.
    pub base_url: Option<Url>,
    /// API key; required for `Anthropic`, optional for `OpenaiCompatible`.
    pub api_key: Option<SecretString>,
    /// How to run the CLI; required for `Cli`.
    pub cli: Option<CliPreset>,
    /// Prices for computing [`Usage::cost_micros`]; without them cost is unknown.
    pub price: Option<Price>,
    /// Total time one request (or CLI run) may take.
    pub timeout: SignedDuration,
}

impl ProviderSettings {
    /// Settings for `kind` with nothing else set and the default timeout.
    #[must_use]
    pub fn new(kind: ProviderKind) -> Self {
        Self {
            kind,
            model: None,
            base_url: None,
            api_key: None,
            cli: None,
            price: None,
            timeout: DEFAULT_TIMEOUT,
        }
    }
}

/// What a provider charges, in US dollars per million tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Price {
    /// Dollars per million input tokens.
    pub input_per_mtok_usd: f64,
    /// Dollars per million output tokens.
    pub output_per_mtok_usd: f64,
}

/// The cost of `usage` in millionths of a dollar, or `None` without a price.
///
/// A dollar per million tokens is exactly one micro-dollar per token, so the price is the
/// per-token cost in micros.
#[must_use]
pub fn cost_micros(price: Option<&Price>, usage: &Usage) -> Option<u64> {
    let price = price?;
    let micros = f64::from(usage.input_tokens) * price.input_per_mtok_usd
        + f64::from(usage.output_tokens) * price.output_per_mtok_usd;
    if !micros.is_finite() || micros < 0.0 {
        return None;
    }
    // `as` saturates, and a cost beyond u64 is not a real bill.
    Some(micros.round() as u64)
}
