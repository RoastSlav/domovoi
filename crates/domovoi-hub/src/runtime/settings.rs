//! Conversion from validated hub configuration to engine and adapter settings.

use std::sync::Arc;

use domovoi_config::hub::{HubConfig, LlmKind, LlmProvider};
use domovoi_core::IncidentPolicy;
use domovoi_engine::notify::{NotifySettings, QuietHours};
use domovoi_engine::triage::TriageSettings;
use domovoi_engine::{ActionSettings, EnginePolicy};
use domovoi_llm::{Price, ProviderKind, ProviderSettings};
use jiff::SignedDuration;

pub(super) fn engine(config: &HubConfig) -> EnginePolicy {
    EnginePolicy {
        incident: IncidentPolicy {
            confirm_runs: config.incidents.confirm_runs,
            reopen_window: config.incidents.reopen_window.as_signed(),
            ttl: config.incidents.default_ttl.map(|ttl| ttl.as_signed()),
        },
        grouping_window: config.incidents.grouping_window.as_signed(),
        tick_every: SignedDuration::from_secs(30),
    }
}

pub(super) fn notify(config: &HubConfig) -> NotifySettings {
    let mut settings = NotifySettings::new(config.hub.timezone.zone().clone());
    settings.quiet_hours = config.notify.quiet_hours.map(|hours| QuietHours {
        start: hours.start.time(),
        end: hours.end.time(),
    });
    settings.remind_warning = config.notify.renotify.warning.as_signed();
    settings.remind_critical = config.notify.renotify.critical.as_signed();
    settings
}

pub(super) fn triage(config: &HubConfig) -> TriageSettings {
    TriageSettings {
        concurrency: config.triage.concurrency,
        queue: domovoi_engine::triage::DEFAULT_QUEUE,
        per_hour_runs: config.triage.per_hour_runs,
        monthly_budget_micros: config.triage.monthly_budget_usd.and_then(|budget| {
            (budget.is_finite() && budget >= 0.0).then_some((budget * 1_000_000.0).round() as u64)
        }),
        cache_days: config.triage.cache_days,
        budget: domovoi_core::TriageBudget {
            max_tool_calls: config.triage.per_run.tool_calls,
            max_seconds: config.triage.per_run.seconds,
            max_output_tokens: config.triage.per_run.output_tokens,
        },
    }
}

pub(crate) fn provider(config: &LlmProvider) -> ProviderSettings {
    let kind = match config.kind {
        LlmKind::Anthropic => ProviderKind::Anthropic,
        LlmKind::OpenaiCompatible => ProviderKind::OpenaiCompatible,
        LlmKind::Cli => ProviderKind::Cli,
    };
    ProviderSettings {
        kind,
        model: config.model.clone(),
        base_url: config.base_url.clone(),
        api_key: config.api_key.as_ref().map(|key| key.value().clone()),
        cli: config.cli.as_ref().and_then(|cli| {
            let program = cli
                .command
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned());
            let preset = match cli.preset {
                Some(domovoi_config::hub::CliPreset::Claude) => {
                    domovoi_llm::CliPreset::claude(program.unwrap_or_else(|| "claude".to_owned()))
                }
                Some(domovoi_config::hub::CliPreset::Codex) => {
                    domovoi_llm::CliPreset::codex(program.unwrap_or_else(|| "codex".to_owned()))
                }
                None => return None,
            };
            Some(preset.with_args(cli.args.clone()))
        }),
        price: config.price.map(|price| Price {
            input_per_mtok_usd: price.input_per_mtok_usd,
            output_per_mtok_usd: price.output_per_mtok_usd,
        }),
        timeout: config
            .timeout
            .map_or(domovoi_llm::DEFAULT_TIMEOUT, |timeout| timeout.as_signed()),
    }
}

pub(super) fn actions() -> ActionSettings {
    ActionSettings::default()
}

/// The provider chain for one configured slot: the slot's own provider first, then one
/// per fallback model. Built eagerly so a bad fallback model is a start-up error.
pub(crate) fn chain_for(
    config: &LlmProvider,
    executor: Option<Arc<dyn domovoi_core::ports::ProcessExecutor>>,
) -> Result<Vec<Arc<dyn domovoi_core::ports::LlmProvider>>, super::RuntimeError> {
    let base = provider(config);
    let all: Vec<ProviderSettings> = std::iter::chain(
        std::iter::once(base.clone()),
        config.fallback_models.iter().map(|model| {
            let mut fallback = base.clone();
            fallback.model = Some(model.clone());
            fallback
        }),
    )
    .collect();
    let mut chain: Vec<Arc<dyn domovoi_core::ports::LlmProvider>> = Vec::new();
    for settings in all.iter() {
        match domovoi_llm::build(settings, executor.clone()) {
            Ok(provider) => chain.push(provider),
            Err(err) => return Err(super::RuntimeError::Llm(err)),
        }
    }
    Ok(chain)
}

/// The providers for `config`'s fallback models alone, in order, without the slot's
/// own provider, which an override can replace without rebuilding.
pub(crate) fn fallback_models_for(
    config: &LlmProvider,
    executor: Option<Arc<dyn domovoi_core::ports::ProcessExecutor>>,
) -> Result<Vec<Arc<dyn domovoi_core::ports::LlmProvider>>, super::RuntimeError> {
    let base = provider(config);
    let mut chain: Vec<Arc<dyn domovoi_core::ports::LlmProvider>> = Vec::new();
    for model in config.fallback_models.iter() {
        let mut settings = base.clone();
        settings.model = Some(model.clone());
        match domovoi_llm::build(&settings, executor.clone()) {
            Ok(provider) => chain.push(provider),
            Err(err) => return Err(super::RuntimeError::Llm(err)),
        }
    }
    Ok(chain)
}
