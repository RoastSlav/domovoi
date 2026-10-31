//! Errors from loading skills and running them.

use std::path::PathBuf;

use domovoi_core::ports::{ProcessError, SkillOutput};

/// A skill folder that was not loaded, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogIssue {
    /// The folder (or the catalogue directory itself for directory-level problems).
    pub skill_dir: PathBuf,
    /// What is wrong and how to fix it.
    pub message: String,
}

impl std::fmt::Display for CatalogIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.skill_dir.display(), self.message)
    }
}

/// Why a skill run did not complete normally.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RunError {
    /// The skill is not (or no longer) in this catalogue.
    #[error("skill {0} is not in the catalogue")]
    UnknownSkill(String),
    /// The entry changed on disk since the catalogue loaded it.
    #[error("skill entry changed since it was loaded: {0}")]
    EntryChanged(String),
    /// The executor refused or failed to start the process.
    #[error(transparent)]
    Process(#[from] ProcessError),
    /// The run hit its timeout and was killed; `output` holds what it printed.
    #[error("skill timed out after {}ms", output.duration_ms)]
    TimedOut {
        /// Output captured before the kill.
        output: SkillOutput,
    },
}
