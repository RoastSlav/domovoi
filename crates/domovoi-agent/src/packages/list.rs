//! `agent.package.list`: what is installed, who signed it and whether it could run.

use domovoi_core::package::{KeyState, PackageListEntry, PackageStatus};

use super::error::PackageError;
use super::{ListOutcome, PackageService};

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
                let enabled = self.is_enabled(entry);
                PackageListEntry {
                    name: entry.name.clone(),
                    version: entry.version.clone(),
                    key_id: entry.key_id.clone(),
                    archive_sha256: entry.archive_sha256,
                    installed_at: entry.installed_at,
                    skills: entry.skills.clone(),
                    status,
                    key_state,
                    executable: enabled.is_ok() && status == PackageStatus::Active,
                    executable_reason: enabled.err(),
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
