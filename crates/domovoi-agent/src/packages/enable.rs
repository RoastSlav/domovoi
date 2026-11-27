//! `agent.package.enable` and `disable`: the owner's decision that a stored package's skills
//! may run on this host.
//!
//! Install stores files and records a digest of the tree it placed. Enabling re-reads the
//! disk, refuses when the tree differs from that digest or the signer is no longer trusted,
//! and records the decision in `.enabled.toml` inside the packages directory (0600, owned by
//! the agent, never writable by the skill account). The state lives on the agent, so a hub
//! restore cannot enable anything. Every time the skills are listed or run, the same checks
//! run again, so a changed tree or a revoked key switches the skills off by itself, with the
//! reason. Nothing here enables a package on its own: a package that was just installed is
//! stored and off.

use std::path::Path;

use domovoi_core::package::{KeyState, PackageFailureCode, PackageName, PackageVersion};
use domovoi_core::skill::SkillName;
use domovoi_core::skill_runner::SkillSource;
use domovoi_proto::messages::PackageSkillState;
use domovoi_runner::{LoadedSkill, SkillCatalog};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::PackageService;
use super::error::PackageError;
use super::events::{EventKind, PackageEvent};
use super::layout::PRIVATE_FILE_MODE;
use super::outcome::{EnableOutcome, EnableRequest};
use super::registry::RegistryEntry;
use crate::fs_atomic::{read_optional, write_atomic_durable_with};

/// Name of the file inside `packages_dir` that lists the enabled packages.
pub const ENABLED_FILE: &str = ".enabled.toml";
const ENABLED_FORMAT: u32 = 1;

/// Why a stored package's skills are off before anyone enabled them.
pub const REASON_NOT_ENABLED: &str =
    "the package is stored but not enabled; a person enables it on the host's Packages tab";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EnabledEntry {
    name: PackageName,
    version: PackageVersion,
    tree_sha256: String,
    enabled_at: Timestamp,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EnabledFile {
    format: u32,
    #[serde(default, rename = "enabled")]
    entries: Vec<EnabledEntry>,
}

/// A package skill that may run now.
#[derive(Debug, Clone)]
pub struct RunnableSkill {
    /// Where the runner finds it.
    pub source: SkillSource,
    /// A catalogue of the package's skills, as the route needs it.
    pub catalog: SkillCatalog,
    /// The skill.
    pub skill: LoadedSkill,
}

/// The stored packages' skills: which are on, and why the others are off.
#[derive(Debug, Default)]
pub struct SkillView {
    /// One row per skill of every stored package.
    pub states: Vec<PackageSkillState>,
    /// The ones that may run.
    pub runnable: Vec<RunnableSkill>,
}

/// SHA-256 over every directory and file under `root`: sorted relative paths, the owner
/// executable bit and the content. A symbolic link or any other entry is an error.
///
/// # Errors
/// `storage` when the tree cannot be read, `tree_changed` for a link or special file.
pub fn tree_digest(root: &Path) -> Result<String, PackageError> {
    let mut lines: Vec<(String, String)> = Vec::new();
    walk(root, root, &mut lines)?;
    lines.sort();
    let mut hasher = Sha256::new();
    for (path, detail) in lines {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(detail.as_bytes());
        hasher.update(*b"\n");
    }
    Ok(hex::encode(hasher.finalize()))
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) -> Result<(), PackageError> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| PackageError::io("reading the package tree", &e))?;
    for entry in entries {
        let entry = entry.map_err(|e| PackageError::io("reading the package tree", &e))?;
        let path = entry.path();
        let meta = std::fs::symlink_metadata(&path)
            .map_err(|e| PackageError::io("reading the package tree", &e))?;
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .into_owned();
        if meta.is_dir() {
            out.push((rel, "dir".to_owned()));
            walk(root, &path, out)?;
        } else if meta.is_file() {
            let bytes =
                std::fs::read(&path).map_err(|e| PackageError::io("reading a package file", &e))?;
            out.push((
                rel,
                format!(
                    "file {} {}",
                    u8::from(is_exec(&meta)),
                    hex::encode(Sha256::digest(&bytes))
                ),
            ));
        } else {
            return Err(PackageError::new(
                PackageFailureCode::TreeChanged,
                "the package tree holds a link or a special file",
            ));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn is_exec(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o100 != 0
}

#[cfg(not(unix))]
fn is_exec(_meta: &std::fs::Metadata) -> bool {
    false
}

impl PackageService {
    fn enabled_path(&self) -> std::path::PathBuf {
        self.dirs.packages_dir.join(ENABLED_FILE)
    }

    /// The enabled list; a missing, unreadable or malformed file means nothing is enabled.
    fn read_enabled(&self) -> Vec<EnabledEntry> {
        let Ok(Some(bytes)) = read_optional(&self.enabled_path()) else {
            return Vec::new();
        };
        let parsed = std::str::from_utf8(&bytes)
            .ok()
            .and_then(|text| toml::from_str::<EnabledFile>(text).ok())
            .filter(|file| file.format == ENABLED_FORMAT);
        if parsed.is_none() {
            tracing::warn!("the enabled-packages file cannot be read, so no package is enabled");
        }
        parsed.map(|file| file.entries).unwrap_or_default()
    }

    fn write_enabled(&self, entries: Vec<EnabledEntry>) -> Result<(), PackageError> {
        let file = EnabledFile {
            format: ENABLED_FORMAT,
            entries,
        };
        let text = toml::to_string(&file).map_err(|_| {
            PackageError::new(
                PackageFailureCode::Storage,
                "the enabled list cannot be encoded",
            )
        })?;
        write_atomic_durable_with(
            &self.enabled_path(),
            text.as_bytes(),
            PRIVATE_FILE_MODE,
            self.durable_faults(),
        )
        .map_err(|e| match e {
            crate::fs_atomic::DurableWriteError::BeforeCommit(i)
            | crate::fs_atomic::DurableWriteError::Indeterminate(i) => {
                PackageError::new(PackageFailureCode::Storage, i.to_string())
            }
        })
    }

    /// Why the files of `entry` may not run, or the digest they match.
    fn verify_tree(&self, entry: &RegistryEntry) -> Result<String, PackageError> {
        if self.keys.state(&entry.key_id) != KeyState::Trusted {
            return Err(PackageError::new(
                PackageFailureCode::UntrustedKey,
                "the key that signed this package is not trusted on this host now",
            ));
        }
        let Some(recorded) = &entry.tree_sha256 else {
            return Err(PackageError::new(
                PackageFailureCode::TreeChanged,
                "no tree digest was recorded when this package was installed; install this version again, which records it",
            ));
        };
        let dir = self
            .dirs
            .packages_dir
            .join(entry.name.as_str())
            .join(entry.version.as_str());
        if !dir.is_dir() {
            return Err(PackageError::new(
                PackageFailureCode::NotInstalled,
                "the package files are missing",
            ));
        }
        let now = tree_digest(&dir)?;
        if &now != recorded {
            return Err(PackageError::new(
                PackageFailureCode::TreeChanged,
                "the package files differ from what was installed and signed; install this version again to place them anew",
            ));
        }
        Ok(now)
    }

    /// Turns the skills of a stored package on.
    ///
    /// # Errors
    /// `packages_disabled`, `not_installed`, `version_conflict`, `untrusted_key`,
    /// `tree_changed` or a storage failure.
    pub fn enable(&self, req: EnableRequest) -> Result<EnableOutcome, PackageError> {
        let _guard = self.lock_ops();
        self.ready()?;
        self.need_mutation()?;
        let registry = self.read_registry()?;
        let entry = self.stored(&registry, &req)?;
        let digest = self.verify_tree(entry)?;
        let mut enabled = self.read_enabled();
        let already = enabled
            .iter()
            .any(|e| e.name == req.package && e.version == req.version && e.tree_sha256 == digest);
        if !already {
            enabled.retain(|e| e.name != req.package);
            enabled.push(EnabledEntry {
                name: req.package.clone(),
                version: req.version.clone(),
                tree_sha256: digest.clone(),
                enabled_at: self.deps.clock.now(),
            });
            self.write_enabled(enabled)?;
            self.skills_changed.notify_one();
            self.deps.events.emit(
                PackageEvent::new(EventKind::Enabled)
                    .package(&req.package, &req.version)
                    .key(&entry.key_id),
            );
        }
        Ok(EnableOutcome {
            package: req.package,
            version: req.version,
            enabled: true,
            changed: !already,
            tree_sha256: digest,
        })
    }

    /// Turns the skills of a stored package off. Turning off what is off succeeds.
    ///
    /// # Errors
    /// `packages_disabled`, `version_conflict` or a storage failure.
    pub fn disable(&self, req: EnableRequest) -> Result<EnableOutcome, PackageError> {
        let _guard = self.lock_ops();
        self.ready()?;
        self.need_mutation()?;
        let registry = self.read_registry()?;
        if let Some(entry) = registry.get(&req.package)
            && entry.version != req.version
        {
            return Err(PackageError::new(
                PackageFailureCode::VersionConflict,
                "a different version of this package is stored",
            ));
        }
        let mut enabled = self.read_enabled();
        let before = enabled.len();
        enabled.retain(|e| e.name != req.package);
        let changed = enabled.len() != before;
        if changed {
            self.write_enabled(enabled)?;
            self.skills_changed.notify_one();
            self.deps
                .events
                .emit(PackageEvent::new(EventKind::Disabled).package(&req.package, &req.version));
        }
        Ok(EnableOutcome {
            package: req.package,
            version: req.version,
            enabled: false,
            changed,
            tree_sha256: String::new(),
        })
    }

    fn need_mutation(&self) -> Result<(), PackageError> {
        if self.allow_install {
            Ok(())
        } else {
            Err(PackageError::new(
                PackageFailureCode::PackagesDisabled,
                "package changes are not allowed on this agent",
            ))
        }
    }

    fn stored<'a>(
        &self,
        registry: &'a super::registry::Registry,
        req: &EnableRequest,
    ) -> Result<&'a RegistryEntry, PackageError> {
        let entry = registry.get(&req.package).ok_or_else(|| {
            PackageError::new(
                PackageFailureCode::NotInstalled,
                "this package is not stored here",
            )
        })?;
        if entry.version != req.version {
            return Err(PackageError::new(
                PackageFailureCode::VersionConflict,
                "a different version of this package is stored",
            ));
        }
        Ok(entry)
    }

    /// Drops a package from the enabled list (after an uninstall).
    pub(crate) fn forget_enabled(&self, name: &PackageName) {
        let mut enabled = self.read_enabled();
        let before = enabled.len();
        enabled.retain(|e| &e.name != name);
        if enabled.len() != before
            && let Err(error) = self.write_enabled(enabled)
        {
            tracing::warn!(%error, "could not update the enabled list after an uninstall");
        }
    }

    /// Whether `name` at `version` is enabled and its files still match (for the list).
    pub(crate) fn is_enabled(&self, entry: &RegistryEntry) -> Result<(), String> {
        let enabled = self.read_enabled();
        let Some(record) = enabled
            .iter()
            .find(|e| e.name == entry.name && e.version == entry.version)
        else {
            return Err(REASON_NOT_ENABLED.to_owned());
        };
        match self.verify_tree(entry) {
            Ok(digest) if digest == record.tree_sha256 => Ok(()),
            Ok(_) => Err("the package changed since it was enabled; enable it again".to_owned()),
            Err(error) => Err(format!("switched off: {}", error.detail())),
        }
    }

    /// Every stored package skill, on or off, and the ones that may run.
    #[must_use]
    pub fn skill_view(&self, owner_names: &[String]) -> SkillView {
        let mut view = SkillView::default();
        if self.fence_error().is_some() {
            return view;
        }
        let Ok(registry) = self.read_registry() else {
            return view;
        };
        for entry in registry.entries() {
            let verdict = self.is_enabled(entry);
            let dir = self
                .dirs
                .packages_dir
                .join(entry.name.as_str())
                .join(entry.version.as_str())
                .join("skills");
            for skill in &entry.skills {
                let mut state = PackageSkillState {
                    package: entry.name.to_string(),
                    version: entry.version.to_string(),
                    skill: skill.clone(),
                    enabled: false,
                    reason: None,
                };
                match &verdict {
                    Err(reason) => state.reason = Some(reason.clone()),
                    Ok(()) if owner_names.contains(skill) => {
                        state.reason =
                            Some("an owner skill with the same name takes precedence".to_owned());
                    }
                    Ok(()) => {
                        let found = SkillName::parse(skill)
                            .ok()
                            .and_then(|name| SkillCatalog::load_one(&dir, &name).ok());
                        if let Some((one, loaded_skill)) = found {
                            state.enabled = true;
                            view.runnable.push(RunnableSkill {
                                source: SkillSource::Package {
                                    name: entry.name.clone(),
                                    version: entry.version.clone(),
                                },
                                catalog: one,
                                skill: loaded_skill,
                            });
                        } else {
                            state.reason =
                                Some("the skill cannot be loaded from the package".to_owned());
                        }
                    }
                }
                view.states.push(state);
            }
        }
        view
    }
}
