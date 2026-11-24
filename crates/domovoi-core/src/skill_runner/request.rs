//! What the agent asks of the runner.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::action::ImplHash;
use crate::id::ExecutionId;
use crate::package::{PackageName, PackageVersion};
use crate::skill::SkillName;

/// One request per connection. Unknown fields are refused on every struct.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunnerRequest {
    /// Ask for the runner's version and the two roots it was started with.
    Health(Header),
    /// Run one skill.
    Run(RunRequest),
    /// Ask the runner what its own filesystem view contains.
    Probe(ProbeRequest),
}

impl RunnerRequest {
    /// The protocol number the request carries.
    #[must_use]
    pub const fn protocol(&self) -> u32 {
        match self {
            Self::Health(h) => h.protocol,
            Self::Run(r) => r.protocol,
            Self::Probe(p) => p.protocol,
        }
    }
}

/// The part every request has.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Header {
    /// The protocol number the sender speaks.
    pub protocol: u32,
}

/// A request to run one skill. It carries no path, environment or user name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunRequest {
    /// The protocol number the sender speaks.
    pub protocol: u32,
    /// The idempotency key of this execution; the runner only logs it.
    pub execution_id: ExecutionId,
    /// Which tree the skill folder lives in.
    pub source: SkillSource,
    /// The skill's folder name under the source root.
    pub skill: SkillName,
    /// The agent's hash of manifest and entry script; the runner recomputes it and refuses a difference.
    pub impl_hash: ImplHash,
    /// Arguments as the agent validated them; the runner validates again.
    pub args: BTreeMap<String, String>,
    /// A cap: the effective limit is the smaller of this and the manifest's timeout.
    pub timeout_secs: u32,
    /// A cap: the effective limit is the smaller of this and the manifest's output cap.
    pub max_output_bytes: u32,
}

/// Where a skill folder is found; the grammar of every name excludes separators, so no source can traverse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillSource {
    /// The configured skills directory.
    SkillsDir,
    /// `<packages>/<name>/<version>/skills`.
    Package {
        /// The package name.
        name: PackageName,
        /// The installed version.
        version: PackageVersion,
    },
}

/// A request for the runner's filesystem observations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeRequest {
    /// The protocol number the sender speaks.
    pub protocol: u32,
    /// The helper's pid, supplied by a root caller for the helper-visibility point.
    pub helper_pid: Option<u32>,
}
