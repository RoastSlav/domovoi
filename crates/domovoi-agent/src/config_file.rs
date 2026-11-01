//! Writing `agent.toml` at enrollment, so nobody has to edit it (ADR 0017). An existing
//! file is never touched.

use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::path::Path;

use domovoi_config::{AgentConfig, MapEnv};
use url::Url;

use crate::error::StateError;

/// Renders the `agent.toml` for `host_name` reaching the hub at `hub_url`. Check
/// settings are left to their defaults, and to the overrides the hub manages.
#[must_use]
pub fn render(host_name: &str, hub_url: &Url, fingerprint: &str) -> String {
    let defaults = domovoi_config::agent::AgentSection::default_dirs();
    format!(
        "# agent.toml: written by `domovoi agent enroll`. Check settings are managed from\n\
         # the hub's web UI; anything set here is the base those settings apply to.\n\
         \n\
         [agent]\n\
         # This host's name in Domovoi.\n\
         host_name = {host}\n\
         # The hub's agent server. Its certificate ({fingerprint}) is pinned in\n\
         # {state}/identity.json.\n\
         hub_url = {url}\n\
         # The agent's key and its spool of unsent signals.\n\
         state_dir = {state_q}\n\
         # Where skill manifests are read from.\n\
         skills_dir = {skills}\n\
         # Where the host's filesystem is mounted (\"/host\" in a container).\n\
         host_root = {root}\n",
        host = quote(host_name),
        url = quote(hub_url.as_str()),
        state = defaults.state_dir.display(),
        state_q = quote(&defaults.state_dir.display().to_string()),
        skills = quote(&defaults.skills_dir.display().to_string()),
        root = quote(&defaults.host_root.display().to_string()),
    )
}

fn quote(text: &str) -> String {
    toml::Value::String(text.to_owned()).to_string()
}

/// Writes `render(..)` to `path` unless a file is already there. Returns whether it
/// wrote one. The text is checked as an `agent.toml` first, so a bad host name fails
/// here rather than at the next start.
pub fn write_if_absent(
    path: &Path,
    host_name: &str,
    hub_url: &Url,
    fingerprint: &str,
) -> Result<bool, StateError> {
    if path.exists() {
        return Ok(false);
    }
    let text = render(host_name, hub_url, fingerprint);
    if let Err(issues) = domovoi_config::parse::<AgentConfig>(&text, &MapEnv::new()) {
        let reason = issues
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        return Err(StateError::new(path, reason));
    }
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| StateError::new(path, e))?;
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o640);
    }
    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == ErrorKind::AlreadyExists => return Ok(false),
        Err(err) => return Err(StateError::new(path, err)),
    };
    file.write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|e| StateError::new(path, e))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use domovoi_config::{AgentConfig, MapEnv};
    use url::Url;

    use super::write_if_absent;

    #[test]
    fn enroll_writes_agent_toml_when_absent_and_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("etc/agent.toml");
        let hub = Url::parse("wss://hub.lan:7421/").unwrap();
        assert!(write_if_absent(&path, "nas", &hub, "AB:CD").unwrap());
        let text = std::fs::read_to_string(&path).unwrap();
        let (config, _) = domovoi_config::parse::<AgentConfig>(&text, &MapEnv::new()).unwrap();
        assert_eq!(config.agent.host_name.as_str(), "nas");
        assert_eq!(config.agent.hub_url, hub);
        assert!(text.contains("AB:CD"), "the pinned fingerprint is noted");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o640);
        }

        std::fs::write(&path, "# mine\n").unwrap();
        assert!(!write_if_absent(&path, "other", &hub, "AB:CD").unwrap());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# mine\n",
            "never overwritten"
        );
        assert!(
            write_if_absent(&dir.path().join("bad.toml"), "Not A Name", &hub, "x").is_err(),
            "an invalid host name is refused before writing"
        );
        assert!(!dir.path().join("bad.toml").exists());
    }
}
