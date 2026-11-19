//! `[backup]`.

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::Deserialize;

use super::FALLBACK_KEEP_PRE_MIGRATE;

/// `[backup]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BackupSection {
    /// Where backups are written. Default `<data_dir>/backups`.
    #[serde(default)]
    pub dir: Option<PathBuf>,
    /// Backups kept. Default 7.
    #[serde(default = "default_keep")]
    pub keep: u32,
    /// Pre-migration backups kept. Default 3.
    #[serde(default = "default_keep_pre_migrate")]
    pub keep_pre_migrate: u32,
}

impl Default for BackupSection {
    fn default() -> Self {
        Self {
            dir: None,
            keep: default_keep(),
            keep_pre_migrate: default_keep_pre_migrate(),
        }
    }
}

fn default_keep() -> u32 {
    7
}
fn default_keep_pre_migrate() -> u32 {
    FALLBACK_KEEP_PRE_MIGRATE
}
