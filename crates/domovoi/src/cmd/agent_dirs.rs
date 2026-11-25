//! `domovoi agent dirs`: the two directories the skill runner unit is rendered with.
//!
//! The installer reads this output to write the runner unit's arguments and bind sources, so the
//! unit and `agent.toml` can never disagree. A path that cannot be written into a unit safely is
//! refused with a plain sentence.

use std::path::Path;

use domovoi_config::agent::AgentSection;

/// Prefixes under which the runner's unit view cannot reach a directory, or must not.
const FORBIDDEN_ROOTS: [&str; 6] = ["/run", "/tmp", "/home", "/root", "/proc", "/sys"];

/// The two directories, from the validated `agent.toml` or the defaults when there is none.
pub struct RunnerDirs {
    /// Where skill folders live.
    pub skills_dir: String,
    /// Where installed packages live.
    pub packages_dir: String,
}

impl RunnerDirs {
    /// The directories of `config`, or the defaults, after checking both are usable in a unit.
    pub fn resolve(config: Option<(&Path, &Path)>) -> Result<Self, String> {
        let (skills, packages) = match config {
            Some((skills, packages)) => (skills.to_path_buf(), packages.to_path_buf()),
            None => {
                let defaults = AgentSection::default_dirs();
                (defaults.skills_dir, defaults.packages_dir)
            }
        };
        Ok(Self {
            skills_dir: check("skills_dir", &skills)?,
            packages_dir: check("packages_dir", &packages)?,
        })
    }

    /// The two lines the installer reads.
    #[must_use]
    pub fn render(&self) -> String {
        format!(
            "skills_dir={}\npackages_dir={}\n",
            self.skills_dir, self.packages_dir
        )
    }
}

/// `path` as unit-safe text, or the reason it is not usable.
pub fn check(field: &str, path: &Path) -> Result<String, String> {
    let text = path
        .to_str()
        .ok_or_else(|| format!("{field} is not valid UTF-8"))?;
    if !text.starts_with('/') {
        return Err(format!("{field} must be an absolute path"));
    }
    let normalised = text
        .split('/')
        .skip(1)
        .all(|s| !s.is_empty() && s != "." && s != "..");
    if !normalised {
        return Err(format!("{field} must be a normalised path"));
    }
    if text
        .chars()
        .any(|c| c.is_control() || matches!(c, ' ' | '%' | '|' | '&' | '\\' | '"' | '\''))
    {
        return Err(format!(
            "{field} contains a character that cannot be written into a unit file"
        ));
    }
    for root in FORBIDDEN_ROOTS {
        if is_under(text, root) {
            return Err(format!("{field} must not be under {root}"));
        }
    }
    if is_under(text, "/dev") {
        return Err(format!("{field} must not be under /dev"));
    }
    if text == "/etc/domovoi" || "/etc/domovoi".starts_with(&format!("{text}/")) || text == "/" {
        return Err(format!("{field} must not be or contain /etc/domovoi"));
    }
    let dirs = domovoi_core::identity::HostDirs::default();
    for state in [dirs.state_dir, dirs.helper_dir] {
        let state = state.to_string_lossy().into_owned();
        let state = state.as_str();
        if is_under(text, state) || state.starts_with(&format!("{text}/")) {
            return Err(format!("{field} must not overlap {state}"));
        }
    }
    Ok(text.to_owned())
}

fn is_under(text: &str, root: &str) -> bool {
    text == root || text.starts_with(&format!("{root}/"))
}
