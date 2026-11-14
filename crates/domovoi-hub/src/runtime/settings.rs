//! Conversion from validated hub configuration to engine and adapter settings.

use std::sync::Arc;

use domovoi_config::hub::{HubConfig, LlmKind, LlmProvider};
use domovoi_core::IncidentPolicy;
use domovoi_core::ports::Clock;
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
        reserved_for_incidents: config.triage.runs_reserved_for_incidents(),
        monthly_budget_micros: config.triage.monthly_budget_usd.and_then(|budget| {
            (budget.is_finite() && budget >= 0.0).then_some((budget * 1_000_000.0).round() as u64)
        }),
        cache_days: config.triage.cache_days,
        max_prompt_bytes: config.triage.per_run.prompt_bytes as usize,
        budget: triage_budget(config),
    }
}

/// The triage budget: the configured per-turn allowance clamped to the engine's supported
/// maximum, and the cumulative budget, which defaults to every normal turn plus one
/// full-ceiling turn for a growth retry or the report repair.
fn triage_budget(config: &HubConfig) -> domovoi_core::TriageBudget {
    let per_run = &config.triage.per_run;
    let per_turn = per_run
        .output_tokens
        .min(domovoi_engine::triage::MAX_OUTPUT_ALLOWANCE);
    let total = per_run.total_tokens.unwrap_or_else(|| {
        domovoi_engine::triage::default_total_output_tokens(per_turn, per_run.tool_calls)
    });
    domovoi_core::TriageBudget {
        max_tool_calls: per_run.tool_calls,
        max_seconds: per_run.seconds,
        max_output_tokens: per_turn,
        max_total_output_tokens: total,
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

/// The purge policy from `[retention]`. The three keys that cover data not built yet are
/// not read.
pub(super) fn retention(config: &HubConfig) -> domovoi_store::RetentionPolicy {
    let days = |n: u32| SignedDuration::from_hours(24 * i64::from(n));
    domovoi_store::RetentionPolicy {
        signals: days(config.retention.signals_days),
        timeline: days(config.retention.incident_timeline_days),
        conversations: days(config.retention.conversations_days),
        notifications: days(config.retention.notifications_days),
        triage: days(config.retention.triage_cache_days),
        resolved_advisories: days(config.retention.resolved_advisories_days),
    }
}

/// The hard per-call deadline for a configured provider.
fn call_deadline(config: &LlmProvider) -> SignedDuration {
    config
        .call_deadline
        .map_or(domovoi_llm::DEFAULT_CALL_DEADLINE, |deadline| {
            deadline.as_signed()
        })
}

/// The provider chain for one configured slot: the slot's own provider first, then one
/// per fallback model. Built eagerly so a bad fallback model is a start-up error, and every
/// built provider carries the configured hard per-call deadline.
pub(crate) fn chain_for(
    config: &LlmProvider,
    executor: Option<Arc<dyn domovoi_core::ports::ProcessExecutor>>,
    clock: &Arc<dyn Clock>,
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
            Ok(provider) => chain.push(domovoi_llm::with_deadline(
                provider,
                call_deadline(config),
                clock.clone(),
            )),
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
    clock: &Arc<dyn Clock>,
) -> Result<Vec<Arc<dyn domovoi_core::ports::LlmProvider>>, super::RuntimeError> {
    let base = provider(config);
    let mut chain: Vec<Arc<dyn domovoi_core::ports::LlmProvider>> = Vec::new();
    for model in config.fallback_models.iter() {
        let mut settings = base.clone();
        settings.model = Some(model.clone());
        match domovoi_llm::build(&settings, executor.clone()) {
            Ok(provider) => chain.push(domovoi_llm::with_deadline(
                provider,
                call_deadline(config),
                clock.clone(),
            )),
            Err(err) => return Err(super::RuntimeError::Llm(err)),
        }
    }
    Ok(chain)
}

#[cfg(test)]
mod tests {
    use domovoi_config::{HubConfig, MapEnv};
    use jiff::SignedDuration;

    use super::{retention, triage_budget};

    fn config(text: &str) -> HubConfig {
        domovoi_config::parse::<HubConfig>(text, &MapEnv::new())
            .expect("valid test config")
            .0
    }

    fn budget(toml: &str) -> domovoi_core::TriageBudget {
        let config: HubConfig = toml::from_str(toml).expect("a config");
        triage_budget(&config)
    }

    #[test]
    fn retention_policy_comes_from_the_config() {
        let policy = retention(&config(
            "[triage]\nenabled = false\n[retention]\nsignals_days = 14\nconversations_days = 365\n",
        ));
        assert_eq!(policy.signals, SignedDuration::from_hours(24 * 14));
        assert_eq!(policy.conversations, SignedDuration::from_hours(24 * 365));
        assert_eq!(policy.timeline, SignedDuration::from_hours(24 * 90));
        assert_eq!(policy.notifications, SignedDuration::from_hours(24 * 30));
        assert_eq!(policy.triage, SignedDuration::from_hours(24 * 90));
        assert_eq!(
            policy.resolved_advisories,
            SignedDuration::from_hours(24 * 30)
        );
    }

    #[test]
    fn declared_retention_keys_are_applied_to_nothing() {
        let base = config("[triage]\nenabled = false\n");
        let declared = config(
            "[triage]\nenabled = false\n[retention]\noperation_journal_days = 1\n\
             execution_outputs_days = 2\nretracted_facts_days = 3\n",
        );
        assert_eq!(retention(&base), retention(&declared));
    }

    #[test]
    fn the_default_run_budget_covers_every_turn_a_ladder_and_the_repair() {
        let budget = budget("");
        assert_eq!(budget.max_output_tokens, 2000);
        assert_eq!(
            budget.max_total_output_tokens,
            2000 * 16 + (8000 + 16_000) + (2000 + 8000 + 16_000),
            "16 normal turns, one turn retried to the ceiling, and a repair retried to the ceiling"
        );
    }

    #[test]
    fn an_output_allowance_above_the_maximum_is_clamped() {
        let budget = budget("[triage.per_run]\noutput_tokens = 32000\n");
        assert_eq!(budget.max_output_tokens, 16_000);
        assert_eq!(
            budget.max_total_output_tokens,
            16_000 * 16 + 16_000,
            "a full ladder at the ceiling is its first request, with no retries"
        );
    }

    #[test]
    fn an_explicit_total_is_kept() {
        let budget = budget("[triage.per_run]\ntotal_tokens = 5000\n");
        assert_eq!(budget.max_total_output_tokens, 5000);
    }
}
