//! Where a request's skill folder lives, under the roots the unit was rendered with.

use std::path::{Path, PathBuf};

use domovoi_core::skill_runner::SkillSource;

/// The directory holding the skill folders for `source`.
///
/// For a package it is `<packages>/<name>/<version>/skills`; the grammar of package names and
/// versions excludes separators, so the join cannot leave the packages root by itself. The
/// result is still checked against the canonical packages root, because a package folder may
/// be a symlink.
///
/// `None` when the root does not exist or resolves outside the packages root.
pub(super) fn root_for(
    source: &SkillSource,
    skills_root: &Path,
    packages_root: &Path,
) -> Option<PathBuf> {
    match source {
        SkillSource::SkillsDir => Some(skills_root.to_path_buf()),
        SkillSource::Package { name, version } => {
            let root = packages_root
                .join(name.as_str())
                .join(version.as_str())
                .join("skills");
            let canonical = root.canonicalize().ok()?;
            let base = packages_root.canonicalize().ok()?;
            canonical.starts_with(&base).then_some(canonical)
        }
    }
}
