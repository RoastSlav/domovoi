//! The start-up banner: one INFO line that says what build and configuration is running.
//!
//! A support log must be able to say which build, config file, data directory, addresses
//! and settings layers a hub ran with. Only names appear here, never values: a setting that
//! holds a secret is listed by its key, never by what it holds.

use std::path::PathBuf;

use domovoi_config::hub::HubConfig;
use domovoi_config::{Provenance, Source};

use super::HubRuntime;

/// How many setting names the banner lists before it says how many more there are.
const MAX_LISTED_KEYS: usize = 40;

/// What the hub learned while loading its settings that the banner reports.
#[derive(Debug, Clone, Default)]
pub struct StartupFacts {
    /// The `hub.toml` that was read, if any.
    pub config_file: Option<PathBuf>,
    /// The revision of the settings saved from the web UI (0 when none were saved).
    pub revision: Option<u64>,
    /// Settings set by the file or the environment, as `key(file)` or `key(env)`. Names only.
    pub locked_keys: Vec<String>,
}

impl StartupFacts {
    /// The facts from the layers just loaded.
    #[must_use]
    pub fn from_layers(
        config_file: Option<PathBuf>,
        revision: u64,
        provenance: &Provenance,
    ) -> Self {
        let locked_keys = provenance
            .iter()
            .filter_map(|(key, source)| match source {
                Source::File => Some(format!("{key}(file)")),
                Source::Env(_) => Some(format!("{key}(env)")),
                Source::Default | Source::Ui => None,
            })
            .collect();
        Self {
            config_file,
            revision: Some(revision),
            locked_keys,
        }
    }

    /// The keys as one line, capped so a big file does not make a huge banner.
    fn keys_text(&self) -> String {
        if self.locked_keys.is_empty() {
            return "none".to_owned();
        }
        let shown = self.locked_keys.iter().take(MAX_LISTED_KEYS);
        let mut text = shown.cloned().collect::<Vec<_>>().join(",");
        if self.locked_keys.len() > MAX_LISTED_KEYS {
            text.push_str(&format!(
                ",+{} more",
                self.locked_keys.len() - MAX_LISTED_KEYS
            ));
        }
        text
    }
}

/// The first 8 characters of the `token` query value in a setup link, so a later line can
/// say which link is meant without writing the token itself.
#[must_use]
pub fn setup_token_prefix(link: &str) -> String {
    link.split("token=")
        .nth(1)
        .map(|t| t.chars().take(8).collect())
        .unwrap_or_default()
}

/// The account the hub runs as, from the environment the service manager sets, or `unknown`.
fn account_name() -> String {
    ["USER", "LOGNAME"]
        .iter()
        .find_map(|key| std::env::var(key).ok().filter(|v| !v.is_empty()))
        .unwrap_or_else(|| "unknown".to_owned())
}

/// Logs that the one-time setup link was printed on standard output, naming the token by
/// its first characters only.
pub fn log_setup_pending(setup_url: &str) {
    tracing::info!(
        token_prefix = %setup_token_prefix(setup_url),
        "no account exists yet; the one-time setup link (valid 24 hours) was printed on standard output, not logged"
    );
}

impl HubRuntime {
    /// The first-run setup link while no account exists, issued once; `None` afterwards.
    pub async fn setup_link(&self) -> Result<Option<String>, super::RuntimeError> {
        self.auth
            .open_setup()
            .await
            .map_err(|err| super::RuntimeError::Serve(format!("could not open setup: {err}")))
    }

    /// The revision of the saved settings this runtime started with.
    #[must_use]
    pub fn settings_revision(&self) -> Option<u64> {
        self.startup.revision
    }

    /// Logs the one start-up banner. `commit` is the build's git commit, known only to the
    /// binary.
    pub fn log_banner(&self, commit: &str) {
        let config: &HubConfig = &self.config;
        let public_url = super::support_public_url(config)
            .map(|url| url.to_string())
            .unwrap_or_else(|err| format!("unusable ({err})"));
        let fds = super::fd_limit::current();
        tracing::info!(
            version = %crate::version::HUB_VERSION,
            commit = %commit,
            config_file = %self
                .startup
                .config_file
                .as_ref()
                .map_or_else(|| "none".to_owned(), |p| p.display().to_string()),
            data_dir = %config.hub.data_dir.display(),
            web_bind = %config.web.bind,
            agent_bind = %config.agent_server.bind,
            public_url = %public_url,
            tls_fingerprint = %self.agent_server.fingerprint_text(),
            settings_revision = self.startup.revision.unwrap_or(0),
            settings_from_file_or_env = %self.startup.keys_text(),
            user = %account_name(),
            open_files_soft_hard = %fds.as_ref().map_or_else(|| "unknown".to_owned(), super::fd_limit::FdLimit::text),
            "domovoi hub starting",
        );
        if let Some(limit) = &fds {
            super::fd_limit::warn_if_low(limit);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_token_prefix_is_short_and_never_the_whole_token() {
        let link = "https://hub.example/setup?token=0123456789abcdef0123456789abcdef";
        assert_eq!(setup_token_prefix(link), "01234567");
        assert_eq!(setup_token_prefix("https://hub.example/setup"), "");
    }

    #[test]
    fn a_long_key_list_is_capped() {
        let facts = StartupFacts {
            locked_keys: (0..45).map(|n| format!("k{n}(file)")).collect(),
            ..StartupFacts::default()
        };
        assert!(facts.keys_text().ends_with(",+5 more"));
        assert_eq!(StartupFacts::default().keys_text(), "none");
    }
}
