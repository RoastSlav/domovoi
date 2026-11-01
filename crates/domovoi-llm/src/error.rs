//! Build errors and the redacted excerpts that go into turn errors.

use secrecy::{ExposeSecret, SecretString};

/// The longest excerpt of a response body or stderr kept in an error, in characters.
pub const EXCERPT_CHARS: usize = 300;

/// Why [`build`](crate::build) could not make a provider from its settings.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LlmBuildError {
    /// The adapter needs a model name.
    #[error("a model is required for this provider")]
    MissingModel,
    /// The adapter needs a base URL.
    #[error("a base URL is required for this provider")]
    MissingBaseUrl,
    /// The adapter needs an API key.
    #[error("an API key is required for this provider")]
    MissingApiKey,
    /// The CLI provider needs a preset.
    #[error("the CLI provider needs a program and arguments")]
    MissingCli,
    /// The CLI provider needs a process executor, which only the binary supplies.
    #[error("the CLI provider needs a process executor")]
    MissingExecutor,
    /// The CLI program is not an absolute path.
    #[error("the CLI program must be an absolute path: {0}")]
    RelativeProgram(String),
    /// The HTTP client could not be created.
    #[error("cannot create the HTTP client: {0}")]
    Client(String),
}

/// At most [`EXCERPT_CHARS`] characters of `text` for an error message: `secret` replaced
/// wherever it appears, control characters turned into spaces, cut with an ellipsis.
#[must_use]
pub fn excerpt(text: &str, secret: Option<&SecretString>) -> String {
    let mut text = text.to_owned();
    if let Some(secret) = secret.map(ExposeSecret::expose_secret)
        && !secret.is_empty()
    {
        text = text.replace(secret, "[redacted]");
    }
    let cleaned: String = text
        .trim()
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if cleaned.chars().count() <= EXCERPT_CHARS {
        return cleaned;
    }
    let mut cut: String = cleaned.chars().take(EXCERPT_CHARS - 1).collect();
    cut.push('…');
    cut
}
