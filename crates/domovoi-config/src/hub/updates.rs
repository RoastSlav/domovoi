//! `[updates]`.

use schemars::JsonSchema;
use serde::Deserialize;

/// `[updates]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdatesSection {
    /// Look for a newer Domovoi release on GitHub twice a day. Nothing is sent but the
    /// request itself. Default `true`.
    #[serde(default = "default_check")]
    pub check: bool,
}

impl Default for UpdatesSection {
    fn default() -> Self {
        Self {
            check: default_check(),
        }
    }
}

fn default_check() -> bool {
    true
}
