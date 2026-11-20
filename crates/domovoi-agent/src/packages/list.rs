//! `agent.package.list`: what is installed, who signed it and whether it could run.

use domovoi_core::package::{KeyState, PackageListEntry, PackageStatus};

use super::error::PackageError;
use super::{ListOutcome, PackageService};

/// Why no package is executable yet; a test pins it so the skill-runner work notices.
pub const EXECUTABLE_REASON: &str = "skill_runner_not_integrated";

impl PackageService {
    /// Lists the installed packages with their status.
    ///
    /// # Errors
    /// The fence's code while the service is fenced.
    pub fn list(&self) -> Result<ListOutcome, PackageError> {
        self.ready()?;
        let registry = self.read_registry()?;
        let packages = registry
            .entries()
            .iter()
            .map(|entry| {
                let key_state = self.keys.state(&entry.key_id);
                let present = self
                    .dirs
                    .packages_dir
                    .join(entry.name.as_str())
                    .join(entry.version.as_str())
                    .is_dir();
                let status = if !present {
                    PackageStatus::MissingFiles
                } else if key_state == KeyState::Trusted {
                    PackageStatus::Active
                } else {
                    PackageStatus::DisabledUntrustedKey
                };
                PackageListEntry {
                    name: entry.name.clone(),
                    version: entry.version.clone(),
                    key_id: entry.key_id.clone(),
                    archive_sha256: entry.archive_sha256,
                    installed_at: entry.installed_at,
                    skills: entry.skills.clone(),
                    status,
                    key_state,
                    executable: false,
                    executable_reason: Some(EXECUTABLE_REASON.to_owned()),
                }
            })
            .collect();
        Ok(ListOutcome {
            packages,
            install_allowed: self.allow_install,
            privilege_boundary: self.privilege_boundary(),
        })
    }
}
