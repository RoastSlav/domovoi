//! The self-test report, the JSON of `selftest --json`.

use serde::{Deserialize, Serialize};

use super::boundary::BoundaryState;
use super::check::{CheckId, CheckResult};
use super::tier::PlatformId;

/// Every check's result with the facts needed to classify the platform.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelftestReport {
    /// One entry per check that was run or reported `nd`.
    pub results: Vec<CheckResult>,
    /// The operating system and architecture.
    pub platform: PlatformId,
    /// The systemd version text; `None` means the host has no systemd.
    pub systemd_version: Option<String>,
    /// Whether the agent runs as the unprivileged account.
    pub boundary: BoundaryState,
}

impl SelftestReport {
    /// The result for `id`, if the report has one.
    #[must_use]
    pub fn result(&self, id: CheckId) -> Option<&CheckResult> {
        self.results.iter().find(|r| r.id == id)
    }

    /// Whether any check failed. `nd` is not a failure here; it never counts as a pass either.
    #[must_use]
    pub fn any_failed(&self) -> bool {
        self.results.iter().any(|r| r.outcome.is_fail())
    }
}
