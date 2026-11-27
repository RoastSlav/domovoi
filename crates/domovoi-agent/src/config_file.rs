//! Writing `agent.toml` at enrollment, so nobody has to edit it (ADR 0017). An existing
//! file keeps its settings; only its hub address can be pointed at a new hub.

use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::path::Path;

use domovoi_config::{AgentConfig, MapEnv};
use url::Url;

use crate::error::StateError;
use crate::fs_atomic::write_atomic;

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
        options.mode(0o644);
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

/// An existing `agent.toml` with the required keys it lacked filled in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completed {
    /// The file's text with the added lines; the file itself is not touched.
    pub text: String,
    /// The keys that were added, empty when the file had them.
    pub added: Vec<&'static str>,
}

/// Fills `agent.host_name` and `agent.hub_url`, where the file at `path` lacks them, from what
/// the owner gave `agent enroll`. Present values are never changed, and nothing is written:
/// [`save`] does that once the enrollment succeeded.
pub fn complete(
    path: &Path,
    host_name: Option<&str>,
    hub_url: &Url,
) -> Result<Completed, StateError> {
    let text = std::fs::read_to_string(path).map_err(|e| StateError::new(path, e))?;
    let table: toml::Table = text
        .parse()
        .map_err(|e: toml::de::Error| StateError::new(path, format!("invalid TOML: {e}")))?;
    let agent = table.get("agent").and_then(toml::Value::as_table);
    let lacks = |key: &str| agent.is_none_or(|a| !a.contains_key(key));
    let mut added = Vec::new();
    let mut lines = String::new();
    if let Some(name) = host_name.filter(|_| lacks("host_name")) {
        added.push("agent.host_name");
        lines.push_str(&format!("host_name = {}\n", quote(name)));
    }
    if lacks("hub_url") {
        added.push("agent.hub_url");
        lines.push_str(&format!("hub_url = {}\n", quote(hub_url.as_str())));
    }
    if added.is_empty() {
        return Ok(Completed { text, added });
    }
    let mut out = String::new();
    let mut placed = false;
    for line in text.split_inclusive('\n') {
        out.push_str(line);
        if !placed
            && line
                .split('#')
                .next()
                .is_some_and(|l| l.trim() == "[agent]")
        {
            if !line.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(&lines);
            placed = true;
        }
    }
    if !placed {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str("[agent]\n");
        out.push_str(&lines);
    }
    if let Err(err) = out.parse::<toml::Table>() {
        return Err(StateError::new(
            path,
            format!(
                "could not add {}; add it by hand: {err}",
                added.join(" and ")
            ),
        ));
    }
    Ok(Completed { text: out, added })
}

/// Replaces the file at `path` with `text`, as [`complete`] made it.
pub fn save(path: &Path, text: &str) -> Result<(), StateError> {
    write_atomic(path, text.as_bytes(), 0o644)
}

/// Points an existing `agent.toml` at `hub_url`, changing only its `hub_url` line, so the
/// rest of the file is kept. Returns whether the file changed. The result is checked as an
/// `agent.toml` before it is written.
pub fn set_hub_url(path: &Path, hub_url: &Url) -> Result<bool, StateError> {
    let text = std::fs::read_to_string(path).map_err(|e| StateError::new(path, e))?;
    let line = format!("hub_url = {}", quote(hub_url.as_str()));
    let mut replaced = false;
    let updated: Vec<String> = text
        .lines()
        .map(|current| {
            if current.trim_start().starts_with("hub_url") {
                replaced = true;
                line.clone()
            } else {
                current.to_owned()
            }
        })
        .collect();
    if !replaced {
        return Err(StateError::new(path, "has no hub_url line to change"));
    }
    let mut next = updated.join("\n");
    if text.ends_with('\n') {
        next.push('\n');
    }
    if next == text {
        return Ok(false);
    }
    if let Err(issues) = domovoi_config::parse::<AgentConfig>(&next, &MapEnv::new()) {
        let reason = issues
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        return Err(StateError::new(path, reason));
    }
    #[cfg(unix)]
    let mode = 0o644;
    #[cfg(not(unix))]
    let mode = 0o644;
    write_atomic(path, next.as_bytes(), mode)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use domovoi_config::{AgentConfig, MapEnv};
    use url::Url;

    use super::{complete, set_hub_url, write_if_absent};

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
            assert_eq!(mode & 0o777, 0o644);
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

    #[test]
    fn complete_adds_only_the_missing_keys_and_keeps_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agent.toml");
        let hub = Url::parse("wss://hub.lan:7421/").unwrap();
        for before in [
            "# mine\n[agent] # here\n[docker]\nenabled = false\n",
            "[agent]",
            "",
        ] {
            std::fs::write(&path, before).unwrap();
            let done = complete(&path, Some("nas"), &hub).unwrap();
            assert_eq!(done.added, ["agent.host_name", "agent.hub_url"]);
            let (config, _) =
                domovoi_config::parse::<AgentConfig>(&done.text, &MapEnv::new()).unwrap();
            assert_eq!(config.agent.host_name.as_str(), "nas");
            assert_eq!(config.agent.hub_url, hub);
            assert!(before.lines().all(|line| done.text.contains(line)));
            assert_eq!(
                std::fs::read_to_string(&path).unwrap(),
                before,
                "not written"
            );
        }
        std::fs::write(
            &path,
            "[agent]\nhost_name = \"mine\"\nhub_url = \"wss://old:1/\"\n",
        )
        .unwrap();
        assert!(complete(&path, Some("nas"), &hub).unwrap().added.is_empty());
    }

    #[test]
    fn set_hub_url_changes_only_the_hub_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agent.toml");
        let old = Url::parse("wss://chuchi:7421/").unwrap();
        let new = Url::parse("wss://192.168.88.119:7421/").unwrap();
        write_if_absent(&path, "tyron", &old, "AB:CD").unwrap();
        let before = std::fs::read_to_string(&path).unwrap();

        assert!(set_hub_url(&path, &new).unwrap());
        let after = std::fs::read_to_string(&path).unwrap();
        let (config, _) = domovoi_config::parse::<AgentConfig>(&after, &MapEnv::new()).unwrap();
        assert_eq!(config.agent.hub_url, new);
        assert_eq!(config.agent.host_name.as_str(), "tyron");
        assert_eq!(
            after,
            before.replace(
                &format!("\"{}\"", old.as_str()),
                &format!("\"{}\"", new.as_str())
            ),
            "every other line is kept as it was"
        );
        assert!(
            !set_hub_url(&path, &new).unwrap(),
            "the same address changes nothing"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o644);
        }
    }
}
