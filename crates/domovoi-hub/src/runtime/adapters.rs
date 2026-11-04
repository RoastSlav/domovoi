//! Adapters and stored state shared by the runtime, offline admin commands and selftest.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use domovoi_config::hub::HubConfig;
use domovoi_core::ports::{ChatGateway, Clock, HostRepo, InteractionSink, Notifier, SettingsRepo};
use domovoi_core::{Host, HostId, HostKind, HostName};
use domovoi_store::{Store, StoreOptions};

use super::{AdapterOverrides, RuntimeError};
use crate::api::ApiState;
use crate::auth::interactions::{ChatDeps, InteractionAuthorizer};

/// Opens (and migrates) `<data_dir>/hub.db` with the configured backups.
pub(crate) async fn open_store(
    config: &HubConfig,
    clock: &dyn Clock,
) -> Result<Store, RuntimeError> {
    open_store_at(
        &config.hub.data_dir,
        config.backup.dir.as_deref(),
        config.backup.keep as usize,
        clock,
    )
    .await
}

/// Opens (and migrates) `<data_dir>/hub.db`, keeping `keep` backups in `backup_dir`
/// (default `<data_dir>/backups`).
pub(crate) async fn open_store_at(
    data_dir: &std::path::Path,
    backup_dir: Option<&std::path::Path>,
    keep: usize,
    clock: &dyn Clock,
) -> Result<Store, RuntimeError> {
    let options = StoreOptions {
        backup_dir: backup_dir
            .map_or_else(|| data_dir.join("backups"), std::path::Path::to_path_buf),
        keep_backups: keep,
        now: clock.now(),
    };
    Ok(Store::open(&data_dir.join("hub.db"), options).await?)
}

pub(crate) async fn ensure_hub_host(
    store: &Store,
    config: &HubConfig,
    now: jiff::Timestamp,
) -> Result<(), RuntimeError> {
    if hub_host(store).await.is_ok() {
        return Ok(());
    }
    let name = HostName::parse(&config.hub.name)
        .or_else(|_| HostName::parse("domovoi"))
        .map_err(|err| RuntimeError::Url(err.to_string()))?;
    let id = if let Some(existing) = store.get_host_by_name(&name).await? {
        existing.id
    } else {
        let id = HostId::new();
        store
            .upsert_host(&Host {
                id,
                name,
                kind: HostKind::Local,
                labels: BTreeMap::new(),
                created_at: now,
            })
            .await?;
        id
    };
    SettingsRepo::set(store, "hub_host_id", &serde_json::json!(id.to_string())).await?;
    Ok(())
}

pub(crate) async fn hub_host(store: &Store) -> Result<HostId, RuntimeError> {
    let value = SettingsRepo::get(store, "hub_host_id")
        .await?
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or_else(|| RuntimeError::Serve("the hub host id is not initialized".to_owned()))?;
    value
        .parse()
        .map_err(|err| RuntimeError::Serve(format!("invalid stored hub host id: {err}")))
}

pub(crate) type ChatAdapters = (Vec<Arc<dyn ChatGateway>>, Vec<Arc<dyn Notifier>>);

pub(crate) fn build_chat(
    config: &HubConfig,
    clock: &Arc<dyn Clock>,
    public_url: &url::Url,
    overrides: &AdapterOverrides,
) -> Result<ChatAdapters, RuntimeError> {
    if overrides.gateways.is_some() || overrides.notifiers.is_some() {
        return Ok((
            overrides.gateways.clone().unwrap_or_default(),
            overrides.notifiers.clone().unwrap_or_default(),
        ));
    }
    let mut gateways: Vec<Arc<dyn ChatGateway>> = Vec::new();
    let mut notifiers: Vec<Arc<dyn Notifier>> = Vec::new();
    if let Some(discord) = &config.discord {
        let bot = discord.enabled.then(|| {
            Arc::new(domovoi_chat::discord::DiscordGateway::new(
                "discord-gateway",
                discord.bot_token.value().clone(),
                domovoi_chat::discord::DiscordSettings {
                    channel_id: discord.channel_id,
                    web_base: public_url.clone(),
                },
                clock.clone(),
            ))
        });
        // The webhook or the bot, never both: every notifier gets every message, so with
        // both each digest would land in the channel twice.
        if let Some(url) = &discord.webhook_url {
            notifiers.push(Arc::new(
                domovoi_chat::discord::DiscordWebhookNotifier::new(
                    "discord-webhook",
                    url.value().clone(),
                    clock.clone(),
                )
                .map_err(|err| RuntimeError::Chat(err.to_string()))?,
            ));
        } else if let Some(bot) = &bot {
            notifiers.push(bot.clone());
        }
        if let Some(bot) = bot {
            gateways.push(bot);
        }
    }
    Ok((gateways, notifiers))
}

pub(crate) fn gateway_sinks(
    config: &HubConfig,
    api: &ApiState,
    store: &Arc<Store>,
    gateways: Vec<Arc<dyn ChatGateway>>,
) -> Vec<(Arc<dyn ChatGateway>, Arc<dyn InteractionSink>)> {
    let allowed = config
        .discord
        .as_ref()
        .map(|discord| {
            discord
                .allowed_user_ids
                .iter()
                .map(|id| id.to_string())
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    gateways
        .into_iter()
        .map(|gateway| {
            let sink: Arc<dyn InteractionSink> = Arc::new(InteractionAuthorizer::new(
                api,
                ChatDeps {
                    users: store.clone(),
                    notify: store.clone(),
                    audit: store.clone(),
                    gateway: gateway.clone(),
                    allowed: allowed.clone(),
                },
            ));
            (gateway, sink)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use domovoi_config::MapEnv;
    use domovoi_config::hub::HubConfig;
    use domovoi_core::ports::{Clock, Notifier};
    use domovoi_testkit::FakeClock;

    use super::{ChatAdapters, build_chat};
    use crate::runtime::AdapterOverrides;

    fn config(discord: &str) -> HubConfig {
        let text = format!(
            r#"
[hub]
name = "adapters-hub"
data_dir = "/nonexistent/never-used"

[web]
bind = "127.0.0.1:0"

[agent_server]
bind = "127.0.0.1:0"

[discord]
bot_token = {{ env = "BOT_TOKEN" }}
channel_id = 1234
allowed_user_ids = [5678]
{discord}
"#
        );
        let env = MapEnv::new()
            .with("BOT_TOKEN", "bot-token")
            .with("HOOK", "https://discord.com/api/webhooks/1/x");
        domovoi_config::parse(&text, &env).unwrap().0
    }

    fn adapters(discord: &str) -> ChatAdapters {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let start = "2026-11-01T08:00:00Z".parse().unwrap();
        let clock: Arc<dyn Clock> = Arc::new(FakeClock::new(start));
        let url = url::Url::parse("https://domovoi.example.net").unwrap();
        build_chat(&config(discord), &clock, &url, &AdapterOverrides::default()).unwrap()
    }

    fn ids(notifiers: &[Arc<dyn Notifier>]) -> Vec<&str> {
        notifiers.iter().map(|notifier| notifier.id()).collect()
    }

    #[tokio::test]
    async fn bot_mode_registers_gateway_notifier_for_digests() {
        let (gateways, notifiers) = adapters("");
        assert_eq!(gateways.len(), 1);
        assert_eq!(ids(&notifiers), ["discord-gateway"]);
        assert!(!notifiers[0].replaces_held_cards());
    }

    #[tokio::test]
    async fn webhook_carries_one_way_messages_instead_of_the_bot() {
        let (gateways, notifiers) = adapters("webhook_url = { env = \"HOOK\" }");
        assert_eq!(gateways.len(), 1, "the bot still posts cards");
        assert_eq!(ids(&notifiers), ["discord-webhook"]);
    }

    #[tokio::test]
    async fn disabled_bot_is_neither_gateway_nor_notifier() {
        let (gateways, notifiers) = adapters("enabled = false");
        assert!(gateways.is_empty());
        assert!(notifiers.is_empty());
    }
}
