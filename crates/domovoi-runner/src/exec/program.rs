//! Which programs may be run at all.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use domovoi_core::ports::ProcessError;

use super::SHELL_NAMES;

/// Checks `program` and returns the canonical path to execute.
///
/// Refuses a relative path ([`ProcessError::NotFound`]), a shell by the given name or by
/// the name of the file it resolves to ([`ProcessError::ShellRefused`]), a missing file
/// ([`ProcessError::NotFound`]) and anything that is not a regular file with an execute
/// bit ([`ProcessError::NotExecutable`]).
pub(super) fn check(program: &str) -> Result<PathBuf, ProcessError> {
    let path = Path::new(program);
    if !path.is_absolute() {
        return Err(ProcessError::NotFound);
    }
    if is_shell(path) {
        return Err(ProcessError::ShellRefused);
    }
    let canonical = path.canonicalize().map_err(|_| ProcessError::NotFound)?;
    if is_shell(&canonical) {
        return Err(ProcessError::ShellRefused);
    }
    let meta = std::fs::metadata(&canonical).map_err(|_| ProcessError::NotFound)?;
    if !meta.is_file() || meta.permissions().mode() & 0o111 == 0 {
        return Err(ProcessError::NotExecutable);
    }
    Ok(canonical)
}

fn is_shell(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| SHELL_NAMES.contains(&name))
}
