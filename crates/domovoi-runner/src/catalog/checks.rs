//! The rules a skill folder must pass to be loaded.

use std::path::{Path, PathBuf};

use domovoi_core::SkillManifest;

use super::LoadedSkill;
use crate::error::CatalogIssue;

/// The manifest file name inside each skill folder.
pub(crate) const MANIFEST_FILE: &str = "skill.toml";

/// Loads every folder under `dir`, in name order.
pub(super) fn load_dir(dir: &Path) -> (Vec<LoadedSkill>, Vec<CatalogIssue>) {
    let mut skills = Vec::new();
    let mut issues = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) => {
            issues.push(issue(
                dir,
                format!("cannot read the skills directory: {err}"),
            ));
            return (skills, issues);
        }
    };
    let mut folders: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    folders.sort();
    for folder in folders {
        match load_skill(&folder) {
            Ok(skill) => skills.push(skill),
            Err(message) => issues.push(issue(&folder, message)),
        }
    }
    (skills, issues)
}

fn issue(dir: &Path, message: String) -> CatalogIssue {
    CatalogIssue {
        skill_dir: dir.to_path_buf(),
        message,
    }
}

/// Applies every rule to one folder.
fn load_skill(folder: &Path) -> Result<LoadedSkill, String> {
    let manifest_path = folder.join(MANIFEST_FILE);
    let text = std::fs::read_to_string(&manifest_path)
        .map_err(|err| format!("cannot read {MANIFEST_FILE}: {err}"))?;
    let manifest: SkillManifest = toml::from_str(&text)
        .map_err(|err| format!("invalid {MANIFEST_FILE}: {}", err.message()))?;
    let folder_name = folder
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    if folder_name != manifest.name.as_str() {
        return Err(format!(
            "folder is named `{folder_name}` but the manifest name is `{}`; rename one to match",
            manifest.name
        ));
    }
    if let Err(errors) = manifest.validate() {
        let list: Vec<String> = errors.iter().map(ToString::to_string).collect();
        return Err(format!("invalid manifest: {}", list.join("; ")));
    }
    let dir = folder
        .canonicalize()
        .map_err(|err| format!("cannot resolve the skill folder: {err}"))?;
    check_folder(&dir)?;
    let entry_path = resolve_entry(&dir, &manifest.entry)?;
    Ok(LoadedSkill::new(manifest, entry_path, dir))
}

/// Checks the entry of a loaded skill again, just before running it.
pub(super) fn recheck_entry(skill: &LoadedSkill) -> Result<(), String> {
    check_folder(&skill.dir)?;
    let entry = resolve_entry(&skill.dir, &skill.manifest.entry)?;
    if entry != skill.entry_path {
        return Err(format!(
            "entry now resolves to {} instead of {}",
            entry.display(),
            skill.entry_path.display()
        ));
    }
    Ok(())
}

/// Resolves `entry` inside `dir` (already canonical) and checks the file.
fn resolve_entry(dir: &Path, entry: &str) -> Result<PathBuf, String> {
    let path = Path::new(entry);
    if path.is_absolute() {
        return Err(format!(
            "entry `{entry}` must be relative to the skill folder"
        ));
    }
    let resolved = dir
        .join(path)
        .canonicalize()
        .map_err(|err| format!("entry `{entry}` cannot be resolved: {err}"))?;
    if !resolved.starts_with(dir) || resolved == dir {
        return Err(format!(
            "entry `{entry}` resolves to {}, outside the skill folder",
            resolved.display()
        ));
    }
    if resolved.to_str().is_none() {
        return Err(format!("entry `{entry}` path is not valid UTF-8"));
    }
    check_entry_file(&resolved).map(|()| resolved)
}

#[cfg(unix)]
fn check_entry_file(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::metadata(path).map_err(|err| format!("cannot read the entry: {err}"))?;
    let mode = meta.permissions().mode();
    if !meta.is_file() {
        return Err("entry is not a regular file".to_owned());
    }
    if mode & 0o111 == 0 {
        return Err("entry is not executable; run chmod 755 on it".to_owned());
    }
    if mode & 0o022 != 0 {
        return Err(format!(
            "entry is writable by group or others (mode {:o}); run chmod go-w on it",
            mode & 0o777
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn check_folder(dir: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(dir)
        .map_err(|err| format!("cannot read the skill folder: {err}"))?
        .permissions()
        .mode();
    if mode & 0o002 != 0 {
        return Err(format!(
            "skill folder is world-writable (mode {:o}); run chmod o-w on it",
            mode & 0o777
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_entry_file(_path: &Path) -> Result<(), String> {
    Err("skills can only be loaded on Unix".to_owned())
}

#[cfg(not(unix))]
fn check_folder(_dir: &Path) -> Result<(), String> {
    Err("skills can only be loaded on Unix".to_owned())
}
