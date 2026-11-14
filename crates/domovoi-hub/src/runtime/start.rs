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

/// The pre-migration backups to keep, from the layers readable before the store opens.
/// A value in `hub.toml` or the environment is the effective one; a value saved in the
/// web UI lives in the database being opened, so that case falls back to the documented
/// default.
fn startup_keep_pre_migrate(sources: &ConfigSources) -> usize {
    domovoi_config::bootstrap_keep_pre_migrate(sources.file.as_deref(), &*sources.env)
        .map_or(domovoi_config::FALLBACK_KEEP_PRE_MIGRATE as usize, |days| {
            days as usize
        })
}

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
        // Backups before a migration go to the default directory: `[backup] dir` may only
        // be known once the stored layer is read. The pre-migration count comes from the
        // file and the environment, the only layers readable before this database opens.
        let keep_pre_migrate = startup_keep_pre_migrate(&sources);
        let store = Arc::new(
            adapters::open_store_at(&data_dir, None, 7, keep_pre_migrate, &*deps.clock).await?,
        );
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
            settings::chain_for(primary, executor.clone(), clock)?;
            if let Some(secondary) = &config.llm.secondary {
                settings::chain_for(secondary, executor.clone(), clock)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use domovoi_config::MapEnv;

    use super::startup_keep_pre_migrate;
    use crate::settings::ConfigSources;

    fn sources(file: Option<&Path>, env: MapEnv) -> ConfigSources {
        ConfigSources {
            file: file.map(Path::to_path_buf),
            env: Arc::new(env),
        }
    }

    fn file_with(contents: &str) -> tempfile::NamedTempFile {
        let file = tempfile::NamedTempFile::new().expect("a temp file");
        std::fs::write(file.path(), contents).expect("write the file");
        file
    }

    #[test]
    fn startup_uses_the_file_keep_pre_migrate() {
        let file = file_with("[backup]\nkeep_pre_migrate = 5\n");
        let sources = sources(Some(file.path()), MapEnv::new());
        assert_eq!(startup_keep_pre_migrate(&sources), 5);
    }

    #[test]
    fn environment_overrides_the_file_for_startup() {
        let file = file_with("[backup]\nkeep_pre_migrate = 5\n");
        let env = MapEnv::new().with("DOMOVOI_BACKUP__KEEP_PRE_MIGRATE", "8");
        let sources = sources(Some(file.path()), env);
        assert_eq!(
            startup_keep_pre_migrate(&sources),
            8,
            "the environment outranks the file, so the file value must not win"
        );
    }

    #[test]
    fn startup_falls_back_to_three_without_a_file_or_environment_value() {
        assert_eq!(domovoi_config::FALLBACK_KEEP_PRE_MIGRATE, 3);
        assert_eq!(
            startup_keep_pre_migrate(&sources(None, MapEnv::new())),
            3,
            "a value saved in the UI lives in the database being opened"
        );
    }
}
