//! `GET /api/v1/settings`: the effective configuration with every secret shown only as
//! `"[set]"` or `"[unset]"` (INV-5), plus this month's model usage and heartbeat status.

use axum::Json;
use axum::extract::State;
use domovoi_config::hub::{LlmKind, LlmProvider, WebhookAuth, WebhookKind};
use domovoi_config::{HubConfig, Secret};
use domovoi_core::AdvisoryKind;
use serde::Serialize;
use utoipa::ToSchema;

use super::error::{ApiError, ErrorBody};
use super::{ApiState, Authed};

/// Shown for a secret that is configured.
pub const SET: &str = "[set]";
/// Shown for a secret that is not configured.
pub const UNSET: &str = "[unset]";

fn secret(value: Option<&Secret>) -> String {
    if value.is_some() { SET } else { UNSET }.to_owned()
}

/// The hub's identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct HubSettings {
    /// The hub's name.
    pub name: String,
    /// The time zone for quiet hours and the digest.
    pub timezone: String,
    /// Where the web UI listens.
    pub bind: String,
    /// The URL people open, if set.
    pub public_url: Option<String>,
    /// Proxies whose forwarded headers are trusted.
    pub trusted_proxies: Vec<String>,
}

/// One language model provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct LlmSettings {
    /// `anthropic`, `openai_compatible` or `cli`.
    pub kind: String,
    /// The model, if set.
    pub model: Option<String>,
    /// The API base URL, if set.
    pub base_url: Option<String>,
    /// `[set]` or `[unset]`.
    pub api_key: String,
}

/// Triage limits.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct TriageSettings {
    /// Whether new incidents are triaged.
    pub enabled: bool,
    /// Runs at the same time.
    pub concurrency: u8,
    /// Runs per hour at most.
    pub per_hour_runs: u32,
    /// Monthly spending cap in US dollars, if set.
    pub monthly_budget_usd: Option<f64>,
}

/// Discord.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct DiscordSettings {
    /// Whether the bot runs.
    pub enabled: bool,
    /// The channel cards go to.
    pub channel_id: String,
    /// Discord user ids allowed to press buttons.
    pub allowed_user_ids: Vec<String>,
    /// `[set]` or `[unset]`.
    pub bot_token: String,
    /// `[set]` or `[unset]`.
    pub webhook_url: String,
}

/// One inbound webhook.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct WebhookSettings {
    /// The name in its URL.
    pub name: String,
    /// `generic`, `alertmanager` or `uptime_kuma`.
    pub kind: String,
    /// `hmac` or `bearer`.
    pub auth: String,
    /// `[set]` or `[unset]`.
    pub secret: String,
    /// The host used when the payload names none.
    pub default_host: Option<String>,
}

/// The dead man's switch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct HeartbeatSettings {
    /// Whether a heartbeat URL is configured.
    pub configured: bool,
    /// `[set]` or `[unset]`.
    pub url: String,
    /// How often it is pinged, if configured.
    pub interval: Option<String>,
    /// Whether the heartbeat-missing advisory is open.
    pub failing: bool,
}

/// Model usage this calendar month (UTC).
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct UsageSettings {
    /// Prompt tokens.
    pub tokens_in: u64,
    /// Generated tokens.
    pub tokens_out: u64,
    /// Cost in US dollars, as far as prices are configured.
    pub cost_usd: f64,
}

/// The effective settings.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct SettingsDto {
    /// The hub and web listener.
    pub hub: HubSettings,
    /// The primary model provider, if configured.
    pub llm_primary: Option<LlmSettings>,
    /// The fallback model provider, if configured.
    pub llm_secondary: Option<LlmSettings>,
    /// Triage limits.
    pub triage: TriageSettings,
    /// Discord, if configured.
    pub discord: Option<DiscordSettings>,
    /// Inbound webhooks.
    pub webhooks: Vec<WebhookSettings>,
    /// The heartbeat.
    pub heartbeat: HeartbeatSettings,
    /// Model usage this month.
    pub usage: UsageSettings,
}

fn llm(provider: &LlmProvider) -> LlmSettings {
    LlmSettings {
        kind: match provider.kind {
            LlmKind::Anthropic => "anthropic",
            LlmKind::OpenaiCompatible => "openai_compatible",
            LlmKind::Cli => "cli",
        }
        .to_owned(),
        model: provider.model.clone(),
        base_url: provider.base_url.as_ref().map(ToString::to_string),
        api_key: secret(provider.api_key.as_ref()),
    }
}

/// The settings view of `config`, with secrets hidden.
#[must_use]
pub fn view(config: &HubConfig) -> (HubSettings, Option<DiscordSettings>, Vec<WebhookSettings>) {
    let hub = HubSettings {
        name: config.hub.name.clone(),
        timezone: config.hub.timezone.name().to_owned(),
        bind: config.web.bind.to_string(),
        public_url: config.web.public_url.as_ref().map(ToString::to_string),
        trusted_proxies: config
            .web
            .trusted_proxies
            .iter()
            .map(ToString::to_string)
            .collect(),
    };
    let discord = config.discord.as_ref().map(|d| DiscordSettings {
        enabled: d.enabled,
        channel_id: d.channel_id.to_string(),
        allowed_user_ids: d.allowed_user_ids.iter().map(ToString::to_string).collect(),
        bot_token: secret(Some(&d.bot_token)),
        webhook_url: secret(d.webhook_url.as_ref()),
    });
    let webhooks = config
        .webhooks
        .iter()
        .map(|(name, w)| WebhookSettings {
            name: name.clone(),
            kind: match w.kind {
                WebhookKind::Generic => "generic",
                WebhookKind::Alertmanager => "alertmanager",
                WebhookKind::UptimeKuma => "uptime_kuma",
            }
            .to_owned(),
            auth: match w.auth {
                WebhookAuth::Hmac => "hmac",
                WebhookAuth::Bearer => "bearer",
            }
            .to_owned(),
            secret: secret(Some(&w.secret)),
            default_host: w.default_host.as_ref().map(ToString::to_string),
        })
        .collect();
    (hub, discord, webhooks)
}

/// The effective configuration, secrets hidden.
#[utoipa::path(
    get, path = "/api/v1/settings", tag = "system",
    responses(
        (status = 200, description = "The settings", body = SettingsDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
    )
)]
pub(super) async fn show(
    State(state): State<ApiState>,
    _auth: Authed,
) -> Result<Json<SettingsDto>, ApiError> {
    let deps = &state.deps;
    let config = &deps.config;
    let (hub, discord, webhooks) = view(config);
    let month = deps.clock.now().to_zoned(jiff::tz::TimeZone::UTC).date();
    let usage = deps.usage.month_total(month).await?;
    let failing = deps
        .advisories
        .list_open()
        .await?
        .iter()
        .any(|a| a.kind == AdvisoryKind::HeartbeatMissing);
    let triage = &config.triage;
    let cost_usd = usage.cost_micros as f64 / 1_000_000.0;
    Ok(Json(SettingsDto {
        hub,
        llm_primary: config.llm.primary.as_ref().map(llm),
        llm_secondary: config.llm.secondary.as_ref().map(llm),
        triage: TriageSettings {
            enabled: triage.enabled,
            concurrency: triage.concurrency,
            per_hour_runs: triage.per_hour_runs,
            monthly_budget_usd: triage.monthly_budget_usd,
        },
        discord,
        webhooks,
        heartbeat: HeartbeatSettings {
            configured: config.heartbeat.is_some(),
            url: secret(config.heartbeat.as_ref().map(|h| &h.url)),
            interval: config.heartbeat.as_ref().map(|h| h.interval.to_string()),
            failing,
        },
        usage: UsageSettings {
            tokens_in: usage.tokens_in,
            tokens_out: usage.tokens_out,
            cost_usd,
        },
    }))
}
