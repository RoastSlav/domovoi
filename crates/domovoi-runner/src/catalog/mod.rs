//! The skills installed on this host, loaded from `<dir>/<name>/skill.toml`.

mod checks;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use domovoi_core::ports::{ProcessExecutor, SkillOutput};
use domovoi_core::{ManifestHash, SkillManifest, SkillName, SkillSummary, ValidatedArgs};

use crate::error::{CatalogIssue, RunError};
use crate::skill_run;

/// A skill that passed every load check. Only the catalogue creates one.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedSkill {
    manifest: SkillManifest,
    manifest_hash: ManifestHash,
    entry_path: PathBuf,
    dir: PathBuf,
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
        self.skills.values().map(|s| s.manifest.summary()).collect()
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
}

impl LoadedSkill {
    pub(crate) fn new(manifest: SkillManifest, entry_path: PathBuf, dir: PathBuf) -> Self {
        Self {
            manifest_hash: manifest.manifest_hash(),
            manifest,
            entry_path,
            dir,
        }
    }
}
