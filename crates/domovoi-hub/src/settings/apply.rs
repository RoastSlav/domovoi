//! Loading the effective settings, applying a change, and going back after a failed
//! apply. The `hub run` supervisor drives these; the hub never restarts itself.

use std::path::PathBuf;
use std::sync::Arc;

use domovoi_config::hub::HubConfig;
use domovoi_config::{ConfigIssue, EnvSource, Layered, LayeredError, load_hub_layered};
use domovoi_core::ports::{AdvisoryRepo, Clock, SettingsRepo, StoreError};
use domovoi_core::{Advisory, AdvisoryKind};
use domovoi_engine::advisories::advisory_id;
use domovoi_store::Store;
use serde_json::json;
use tokio::sync::oneshot;

use super::crypto::{CryptoError, SecretKey};
use super::stored::{CONFIG, LAST_APPLY, PREVIOUS, StoredDoc, StoredState};

/// Where the file and environment layers come from.
#[derive(Clone)]
pub struct ConfigSources {
    /// `hub.toml`, when there is one.
    pub file: Option<PathBuf>,
    /// The environment, for `DOMOVOI_*` overrides and `{ env = .. }` secrets.
    pub env: Arc<dyn EnvSource + Send + Sync>,
}

impl std::fmt::Debug for ConfigSources {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConfigSources")
            .field("file", &self.file)
            .finish_non_exhaustive()
    }
}

/// A saved change for the supervisor to apply. It answers on `reply` before restarting
/// the runtime: `Ok` once the new settings passed preflight, or why they did not (by
/// then the previous settings are stored again).
pub struct ApplyRequest {
    /// The new effective configuration.
    pub config: HubConfig,
    /// The answer for the waiting request.
    pub reply: oneshot::Sender<Result<(), String>>,
}

impl std::fmt::Debug for ApplyRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ApplyRequest(..)")
    }
}

/// Why the effective settings could not be loaded.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    /// The layers do not make a valid configuration.
    #[error("the settings are not valid:\n  {}", lines(.0))]
    Invalid(Vec<ConfigIssue>),
    /// The secret key could not be used.
    #[error(transparent)]
    Crypto(#[from] CryptoError),
    /// The store could not be read.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The stored document is not usable.
    #[error("the saved settings are damaged: {0}")]
    Damaged(String),
}

fn lines(issues: &[ConfigIssue]) -> String {
    issues
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n  ")
}

/// The stored layer and the merged configuration, from an open store.
pub(crate) async fn load_layers(
    store: &Store,
    key: &SecretKey,
    sources: &ConfigSources,
    clock: &dyn Clock,
) -> Result<(Layered, StoredState), LoadError> {
    let stored = StoredState::load(store, key).await?;
    sync_unreadable(store, clock, &stored.unreadable).await;
    let table = stored.doc.table().map_err(LoadError::Damaged)?;
    match load_hub_layered(sources.file.as_deref(), &table, &stored, &*sources.env) {
        Ok(layered) => Ok((layered, stored)),
        Err(LayeredError { issues, .. }) => Err(LoadError::Invalid(issues)),
    }
}

/// The effective configuration for a command that runs next to (or instead of) the hub:
/// the file and environment layers, plus the UI layer when the hub's database exists.
pub async fn load_effective(
    sources: &ConfigSources,
    clock: &dyn Clock,
) -> Result<(HubConfig, Vec<ConfigIssue>), LoadError> {
    let data_dir = domovoi_config::bootstrap_data_dir(sources.file.as_deref(), &*sources.env)
        .map_err(LoadError::Invalid)?;
    if !data_dir.join("hub.db").exists() {
        let layered = load_hub_layered(
            sources.file.as_deref(),
            &toml::Table::new(),
            &domovoi_config::NoStoredSecrets,
            &*sources.env,
        )
        .map_err(|err| LoadError::Invalid(err.issues))?;
        return Ok((layered.config, layered.issues));
    }
    let key = SecretKey::load_or_create(&data_dir)?;
    let store = open_store(&data_dir, clock).await?;
    let loaded = load_layers(&store, &key, sources, clock).await;
    store.close().await;
    let (layered, _) = loaded?;
    Ok((layered.config, layered.issues))
}

async fn open_store(data_dir: &std::path::Path, clock: &dyn Clock) -> Result<Store, LoadError> {
    let options = domovoi_store::StoreOptions {
        backup_dir: data_dir.join("backups"),
        keep_backups: 7,
        now: clock.now(),
    };
    Store::open(&data_dir.join("hub.db"), options)
        .await
        .map_err(|err| LoadError::Damaged(err.to_string()))
}

/// Restores the settings saved before the last change, records why, and raises the
/// `SettingsApplyFailed` advisory. Called by the supervisor with the runtime stopped.
pub async fn rollback(
    data_dir: &std::path::Path,
    clock: &dyn Clock,
    why: &str,
) -> Result<(), LoadError> {
    let store = open_store(data_dir, clock).await?;
    let result = restore_previous(&store, clock, why).await;
    store.close().await;
    result
}

pub(crate) async fn restore_previous(
    store: &Store,
    clock: &dyn Clock,
    why: &str,
) -> Result<(), LoadError> {
    let current = StoredDoc::from_value(SettingsRepo::get(store, CONFIG).await?);
    let previous = StoredDoc::from_value(SettingsRepo::get(store, PREVIOUS).await?);
    let restored = StoredDoc {
        revision: current.revision + 1,
        document: previous.document,
    };
    store
        .set_many(&[
            (CONFIG.to_owned(), Some(restored.to_value())),
            (
                LAST_APPLY.to_owned(),
                Some(json!({ "ok": false, "at": clock.now().to_string(), "message": why })),
            ),
        ])
        .await?;
    raise(
        store,
        clock,
        AdvisoryKind::SettingsApplyFailed,
        "Saved settings could not be applied",
        &format!("The hub went back to the previous settings. {why}"),
    )
    .await;
    Ok(())
}

/// Records a successful apply and clears a previous failure.
pub(crate) async fn applied(
    store: &dyn SettingsRepo,
    advisories: &dyn AdvisoryRepo,
    clock: &dyn Clock,
) {
    let record = json!({ "ok": true, "at": clock.now().to_string() });
    if let Err(err) = store.set(LAST_APPLY, &record).await {
        tracing::warn!(%err, "could not record the apply");
    }
    let id = advisory_id(AdvisoryKind::SettingsApplyFailed, None, "settings");
    if let Err(err) = advisories.resolve(id, clock.now()).await
        && !matches!(err, StoreError::NotFound)
    {
        tracing::warn!(%err, "could not clear the apply advisory");
    }
}

async fn sync_unreadable(store: &dyn AdvisoryRepo, clock: &dyn Clock, unreadable: &[String]) {
    if unreadable.is_empty() {
        let id = advisory_id(AdvisoryKind::StoredSecretUnreadable, None, "settings");
        if let Err(err) = store.resolve(id, clock.now()).await
            && !matches!(err, StoreError::NotFound)
        {
            tracing::warn!(%err, "could not clear the secret advisory");
        }
        return;
    }
    tracing::warn!(paths = ?unreadable, "saved secrets cannot be decrypted");
    raise(
        store,
        clock,
        AdvisoryKind::StoredSecretUnreadable,
        "Saved secrets cannot be decrypted",
        &format!(
            "The secret key in the data directory does not match. Enter these again in Settings: {}.",
            unreadable.join(", ")
        ),
    )
    .await;
}

async fn raise(
    store: &dyn AdvisoryRepo,
    clock: &dyn Clock,
    kind: AdvisoryKind,
    title: &str,
    detail: &str,
) {
    let id = advisory_id(kind, None, "settings");
    let created_at = store
        .list_open()
        .await
        .ok()
        .and_then(|open| open.into_iter().find(|a| a.id == id).map(|a| a.created_at))
        .unwrap_or_else(|| clock.now());
    let advisory = Advisory {
        id,
        kind,
        host: None,
        title: title.to_owned(),
        detail: detail.to_owned(),
        created_at,
        resolved_at: None,
    };
    if let Err(err) = store.upsert(&advisory).await {
        tracing::warn!(%err, "could not record a settings advisory");
    }
}
