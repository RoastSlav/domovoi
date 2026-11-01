//! Turns [`ProviderSettings`] into a provider.

use std::sync::Arc;

use domovoi_core::ports::{LlmProvider, ProcessExecutor};

use crate::anthropic::Anthropic;
use crate::cli::Cli;
use crate::error::LlmBuildError;
use crate::openai::OpenAiCompatible;
use crate::settings::{ProviderKind, ProviderSettings};

/// The provider `settings` describe. The CLI kind needs `executor`, which only the binary
/// supplies; the HTTP kinds ignore it.
pub fn build(
    settings: &ProviderSettings,
    executor: Option<Arc<dyn ProcessExecutor>>,
) -> Result<Arc<dyn LlmProvider>, LlmBuildError> {
    let model = || settings.model.clone().ok_or(LlmBuildError::MissingModel);
    let provider: Arc<dyn LlmProvider> = match settings.kind {
        ProviderKind::Anthropic => Arc::new(Anthropic::new(
            settings.base_url.as_ref(),
            model()?,
            settings
                .api_key
                .clone()
                .ok_or(LlmBuildError::MissingApiKey)?,
            settings.price,
            settings.timeout,
        )?),
        ProviderKind::OpenaiCompatible => Arc::new(OpenAiCompatible::new(
            settings
                .base_url
                .as_ref()
                .ok_or(LlmBuildError::MissingBaseUrl)?,
            model()?,
            settings.api_key.clone(),
            settings.price,
            settings.timeout,
        )?),
        ProviderKind::Cli => Arc::new(Cli::new(
            executor.ok_or(LlmBuildError::MissingExecutor)?,
            settings.cli.clone().ok_or(LlmBuildError::MissingCli)?,
            settings.timeout,
        )?),
    };
    Ok(provider)
}
