//! The skills installed on this host, loaded from `<dir>/<name>/skill.toml`.

pub(crate) mod checks;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use domovoi_core::ports::{ProcessExecutor, SkillOutput};
use domovoi_core::{
    ManifestHash, ScriptDigest, SkillManifest, SkillName, SkillSummary, ValidatedArgs,
};
use jiff::SignedDuration;

use crate::error::{CatalogIssue, RunError};
use crate::exec::{RunEnd, SandboxExecutor};
use crate::skill_run;

/// A skill that passed every load check. Only the catalogue creates one.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedSkill {
    manifest: SkillManifest,
    manifest_hash: ManifestHash,
    entry_path: PathBuf,
    dir: PathBuf,
    /// SHA-256 of the entry file's bytes as they were when the catalogue loaded it.
    entry_script_digest: ScriptDigest,
    /// The folder's `README.md`, at most 8 KiB; absent when there is none.
    readme: Option<String>,
}

impl LoadedSkill {
    /// The parsed, validated manifest.
    #[must_use]
    pub fn manifest(&self) -> &SkillManifest {
        &self.manifest
    }

    /// Hash of the manifest, reported to the hub and bound into plan hashes.
    #[must_use]
    pub fn manifest_hash(&self) -> ManifestHash {
        self.manifest_hash
    }

    /// Canonical path of the entry, inside [`LoadedSkill::dir`].
    #[must_use]
    pub fn entry_path(&self) -> &Path {
        &self.entry_path
    }

    /// Canonical path of the skill folder.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// SHA-256 of the entry file's bytes when the catalogue loaded it.
    ///
    /// Reported to the hub so a plan hash can bind the entry bytes. It covers the entry
    /// executable only: not its interpreter, sourced files, other folder contents or the rest
    /// of the runtime environment.
    #[must_use]
    pub fn entry_script_digest(&self) -> ScriptDigest {
        self.entry_script_digest
    }
}

/// Every valid skill under one directory.
///
/// Loading never fails as a whole: each folder that breaks a rule is left out and
/// reported as a [`CatalogIssue`], and the rest are kept. The rules: the folder holds a
/// `skill.toml` that parses and passes [`SkillManifest::validate`]; the manifest's `name`
/// equals the folder name; the folder is not world-writable; the entry resolves (after
/// following symlinks) to a regular, executable file inside the folder that is not
/// writable by group or others.
#[derive(Debug, Clone, PartialEq)]
pub struct SkillCatalog {
    dir: PathBuf,
    skills: BTreeMap<SkillName, LoadedSkill>,
}

impl SkillCatalog {
    /// Loads every skill under `dir`.
    #[must_use]
    pub fn load(dir: &Path) -> (Self, Vec<CatalogIssue>) {
        let mut catalog = Self {
            dir: dir.to_path_buf(),
            skills: BTreeMap::new(),
        };
        let issues = catalog.reload();
        (catalog, issues)
    }

    /// Loads exactly `<root>/<name>` into a catalogue of one.
    ///
    /// The folder is canonicalised and refused when it does not resolve to a direct child of
    /// the canonical `root` (a symlinked folder); every other check of [`SkillCatalog::load`]
    /// applies.
    ///
    /// # Errors
    /// A [`CatalogIssue`] saying why the folder is missing, outside the root or invalid.
    pub fn load_one(root: &Path, name: &SkillName) -> Result<(Self, LoadedSkill), CatalogIssue> {
        let folder = checks::resolve_in_root(root, name.as_str())
            .map_err(|why| checks::missing_issue(root, name.as_str(), why))?;
        let skill = checks::load_folder(&folder)?;
        let catalog = Self {
            dir: root.to_path_buf(),
            skills: BTreeMap::from([(skill.manifest.name.clone(), skill.clone())]),
        };
        Ok((catalog, skill))
    }

    /// Loads the directory again, replacing the current skills.
    pub fn reload(&mut self) -> Vec<CatalogIssue> {
        let (skills, issues) = checks::load_dir(&self.dir);
        self.skills = skills
            .into_iter()
            .map(|skill| (skill.manifest.name.clone(), skill))
            .collect();
        issues
    }

    /// The directory this catalogue reads.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The skill called `name`.
    #[must_use]
    pub fn get(&self, name: &SkillName) -> Option<&LoadedSkill> {
        self.skills.get(name)
    }

    /// What the hub may know about each skill, ordered by name.
    #[must_use]
    pub fn summaries(&self) -> Vec<SkillSummary> {
        self.skills
            .values()
            .map(|s| {
                let mut summary = s.manifest.summary(s.entry_script_digest);
                summary.readme = s.readme.clone();
                summary
            })
            .collect()
    }

    /// Runs `skill` with `args` through `executor`.
    ///
    /// The entry is checked again first, so a file swapped or loosened after loading is
    /// refused. The argv is `[entry, one value per declared param]`, the working
    /// directory is the skill folder, and the manifest's timeout, output cap and
    /// `run_as` apply. A timeout returns [`RunError::TimedOut`] with the partial output.
    pub async fn run_skill(
        &self,
        skill: &LoadedSkill,
        args: &ValidatedArgs,
        executor: &dyn ProcessExecutor,
    ) -> Result<SkillOutput, RunError> {
        if self.get(&skill.manifest.name) != Some(skill) {
            return Err(RunError::UnknownSkill(skill.manifest.name.to_string()));
        }
        checks::recheck_entry(skill).map_err(RunError::EntryChanged)?;
        let spec = skill_run::build_spec(&skill.manifest, &skill.entry_path, &skill.dir, args);
        let argv_hash = skill_run::argv_hash(&spec);
        let output = executor.run(spec).await?;
        let skill_output = SkillOutput {
            exit_code: output.exit_code,
            stdout: output.stdout,
            stderr: output.stderr,
            truncated: output.truncated,
            duration_ms: output.duration_ms,
            argv_hash,
        };
        if output.timed_out {
            return Err(RunError::TimedOut {
                output: skill_output,
            });
        }
        Ok(skill_output)
    }

    /// Runs `skill` like [`SkillCatalog::run_skill`], but for the skill runner: the checks and
    /// the spec are the same, `caps` can only lower the manifest's limits, the spec never
    /// switches user, and `cancel` ends the run early.
    ///
    /// # Errors
    /// [`RunError::UnknownSkill`], [`RunError::EntryChanged`] or [`RunError::Process`]. A
    /// timeout is a normal [`RunEnd::Finished`] with `timed_out` set.
    pub async fn run_skill_cancellable<C>(
        &self,
        skill: &LoadedSkill,
        args: &ValidatedArgs,
        executor: &SandboxExecutor,
        caps: RunCaps,
        cancel: C,
    ) -> Result<CancellableRun, RunError>
    where
        C: std::future::Future<Output = ()> + Send,
    {
        if self.get(&skill.manifest.name) != Some(skill) {
            return Err(RunError::UnknownSkill(skill.manifest.name.to_string()));
        }
        checks::recheck_entry(skill).map_err(RunError::EntryChanged)?;
        let mut spec = skill_run::build_spec(&skill.manifest, &skill.entry_path, &skill.dir, args);
        spec.run_as = None;
        if let Some(timeout) = caps.timeout {
            spec.timeout = spec.timeout.min(timeout);
        }
        if let Some(cap) = caps.max_output_bytes {
            spec.max_output_bytes = spec.max_output_bytes.min(cap);
        }
        let argv_hash = skill_run::argv_hash(&spec);
        let end = executor.run_cancellable(spec, cancel).await?;
        Ok(CancellableRun { end, argv_hash })
    }
}

/// Limits a caller may lower for one run; a cap above the manifest's value changes nothing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunCaps {
    /// Upper bound on the wall-clock limit.
    pub timeout: Option<SignedDuration>,
    /// Upper bound on captured bytes per stream.
    pub max_output_bytes: Option<u32>,
}

/// The result of [`SkillCatalog::run_skill_cancellable`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancellableRun {
    /// How the process ended.
    pub end: RunEnd,
    /// SHA-256 of the argv that was run.
    pub argv_hash: [u8; 32],
}

impl LoadedSkill {
    pub(crate) fn new(
        manifest: SkillManifest,
        entry_path: PathBuf,
        dir: PathBuf,
        entry_script_digest: ScriptDigest,
        readme: Option<String>,
    ) -> Self {
        Self {
            manifest_hash: manifest.manifest_hash(),
            manifest,
            entry_path,
            dir,
            entry_script_digest,
            readme,
        }
    }
}
