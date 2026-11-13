//! `link-status.json` under the state directory: what `domovoi agent status` reads.

use std::path::Path;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::error::StateError;
use crate::fs_atomic::{read_optional, write_atomic};

/// `link-status.json` under the state directory: what `domovoi agent status` reads.
pub const STATUS_FILE: &str = "link-status.json";

/// What the link is doing, for an operator looking at a host that is not reporting.
#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone)]
pub struct LinkStatus {
    /// `connected`, `reconnecting` or `incompatible`.
    pub state: String,
    /// When the file was last written.
    pub updated_at: Timestamp,
    /// Since when the link is connected, while it is.
    pub connected_since: Option<Timestamp>,
    /// For `incompatible`: the protocol the hub said it speaks.
    pub hub_protocol: Option<String>,
    /// For `incompatible`: the protocol this agent speaks.
    pub agent_protocol: Option<String>,
    /// When the idle agent last said it is still waiting.
    pub last_notice_at: Option<Timestamp>,
    /// One line explaining the state.
    pub detail: Option<String>,
}

/// Writes `status` to [`STATUS_FILE`] under `dir`.
pub fn write(dir: &Path, status: &LinkStatus) -> Result<(), StateError> {
    let path = dir.join(STATUS_FILE);
    let bytes = serde_json::to_vec_pretty(status).map_err(|err| StateError::new(&path, err))?;
    write_atomic(&path, &bytes, 0o644)
}

/// Reads [`STATUS_FILE`] under `dir`; `None` when the agent has not run yet.
pub fn read(dir: &Path) -> Result<Option<LinkStatus>, StateError> {
    let path = dir.join(STATUS_FILE);
    let Some(bytes) = read_optional(&path)? else {
        return Ok(None);
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|err| StateError::new(&path, err))
}
