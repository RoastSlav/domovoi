//! Starting from layered settings, and checking new settings before a restart.

use std::sync::Arc;

use domovoi_config::hub::HubConfig;
use domovoi_core::ports::ProcessExecutor;
use domovoi_store::Store;

use super::{
    AdapterOverrides, HubDeps, HubLock, HubRuntime, RuntimeError, adapters, settings, support,
};
use crate::agent_server::ServerSettings;
use crate::settings::{ConfigSources, LoadError, SecretKey};

/// The data directory, locked and opened.
pub(crate) struct Opened {
    pub(crate) lock: HubLock,
    pub(crate) store: Arc<Store>,
    /// The secret key and layer sources, when settings are managed from the UI.
    pub(crate) settings: Option<(Arc<SecretKey>, ConfigSources)>,
}

impl HubRuntime {
    /// Starts from the settings layers: the file and environment in `sources`, and the
    /// values saved in the web UI, which are read from the hub's own database. The web
    /// UI can then change settings, which are applied through `deps.apply`.
    pub async fn start(sources: ConfigSources, deps: HubDeps) -> Result<Self, RuntimeError> {
        let data_dir = domovoi_config::bootstrap_data_dir(sources.file.as_deref(), &*sources.env)
            .map_err(LoadError::Invalid)?;
        let lock = HubLock::acquire(&data_dir)?;
        let key = Arc::new(SecretKey::load_or_create(&data_dir).map_err(LoadError::Crypto)?);
        // Backups before a migration go to the default directory: `[backup]` may only be
        // known once the stored layer is read.
        let store = Arc::new(adapters::open_store_at(&data_dir, None, 7, &*deps.clock).await?);
        let (layered, _) =
            crate::settings::apply::load_layers(&store, &key, &sources, &*deps.clock).await?;
        for issue in &layered.issues {
            tracing::warn!(%issue, "settings");
        }
        let opened = Opened {
            lock,
            store,
            settings: Some((key, sources)),
        };
        Self::assemble(opened, layered.config, deps, AdapterOverrides::default()).await
    }

    /// Checks that `config` can be built into a hub, without the store or listeners:
    /// the public address, the agent server settings, chat adapters and models. The
    /// supervisor runs this before it restarts the runtime with new settings.
    pub fn preflight(
        config: &HubConfig,
        clock: &Arc<dyn domovoi_core::ports::Clock>,
        executor: Option<Arc<dyn ProcessExecutor>>,
    ) -> Result<(), RuntimeError> {
        let public_url = support::public_url(config)?;
        ServerSettings::from_config(config)?;
        adapters::build_chat(config, clock, &public_url, &AdapterOverrides::default())?;
        if config.triage.enabled {
            let primary = config.llm.primary.as_ref().ok_or_else(|| {
                RuntimeError::Chat("triage is enabled without a primary provider".to_owned())
            })?;
            domovoi_llm::build(&settings::provider(primary), executor.clone())?;
            if let Some(secondary) = &config.llm.secondary {
                domovoi_llm::build(&settings::provider(secondary), executor)?;
            }
        }
        Ok(())
    }
}
