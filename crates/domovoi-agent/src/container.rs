//! Detects whether the agent itself runs inside a container.
//!
//! The decision is a pure function over [`ContainerSignals`]; [`probe`] reads the real
//! signals. When nothing points to a container, the answer is `false`.

use std::path::Path;

/// The environment variable that overrides every other signal (`true` or `false`).
pub const OVERRIDE_ENV: &str = "DOMOVOI_AGENT__IN_CONTAINER";

/// Everything the decision looks at, so tests can supply it without a real host.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContainerSignals {
    /// `/.dockerenv` exists.
    pub dockerenv: bool,
    /// `/run/.containerenv` exists.
    pub containerenv: bool,
    /// The contents of `/proc/1/cgroup`, when readable.
    pub cgroup: Option<String>,
    /// The `container` environment variable, when set.
    pub container_env: Option<String>,
    /// The [`OVERRIDE_ENV`] value, when set.
    pub override_env: Option<String>,
    /// `agent.host_root` is not `/`: the agent reads a mounted host.
    pub host_root_mounted: bool,
}

/// Whether the signals say the agent runs in a container. A valid override wins over
/// everything; otherwise any positive signal counts.
#[must_use]
pub fn decide(signals: &ContainerSignals) -> bool {
    match signals
        .override_env
        .as_deref()
        .map(|v| v.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("true") => return true,
        Some("false") => return false,
        _ => {}
    }
    signals.dockerenv
        || signals.containerenv
        || signals.host_root_mounted
        || signals
            .container_env
            .as_deref()
            .is_some_and(|v| !v.trim().is_empty())
        || signals
            .cgroup
            .as_deref()
            .is_some_and(cgroup_names_container)
}

fn cgroup_names_container(cgroup: &str) -> bool {
    ["docker", "kubepods", "containerd", "libpod", "lxc"]
        .iter()
        .any(|marker| cgroup.contains(marker))
}

/// Reads the real signals from this process's environment and filesystem.
#[must_use]
pub fn probe(host_root: &Path) -> ContainerSignals {
    ContainerSignals {
        dockerenv: Path::new("/.dockerenv").exists(),
        containerenv: Path::new("/run/.containerenv").exists(),
        cgroup: std::fs::read_to_string("/proc/1/cgroup").ok(),
        container_env: std::env::var("container").ok(),
        override_env: std::env::var(OVERRIDE_ENV).ok(),
        host_root_mounted: host_root != Path::new("/"),
    }
}

/// Whether the agent runs in a container, from the real host.
#[must_use]
pub fn in_container(host_root: &Path) -> bool {
    decide(&probe(host_root))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(f: impl FnOnce(&mut ContainerSignals)) -> bool {
        let mut signals = ContainerSignals::default();
        f(&mut signals);
        decide(&signals)
    }

    #[test]
    fn nothing_found_is_not_a_container() {
        assert!(!decide(&ContainerSignals::default()));
        assert!(!with(|s| s.cgroup = Some("0::/init.scope\n".into())));
        assert!(!with(|s| s.container_env = Some("  ".into())));
    }

    #[test]
    fn dockerenv_file_means_container() {
        assert!(with(|s| s.dockerenv = true));
    }

    #[test]
    fn containerenv_file_means_container() {
        assert!(with(|s| s.containerenv = true));
    }

    #[test]
    fn cgroup_naming_a_runtime_means_container() {
        for line in [
            "0::/docker/abc123\n",
            "0::/kubepods/burstable/pod1\n",
            "1:name=systemd:/system.slice/containerd.service/x\n",
        ] {
            assert!(with(|s| s.cgroup = Some(line.into())), "{line}");
        }
    }

    #[test]
    fn container_env_var_means_container() {
        assert!(with(|s| s.container_env = Some("podman".into())));
    }

    #[test]
    fn mounted_host_root_means_container() {
        assert!(with(|s| s.host_root_mounted = true));
    }

    #[test]
    fn override_true_wins_without_signals() {
        assert!(with(|s| s.override_env = Some("true".into())));
    }

    #[test]
    fn override_false_beats_every_signal() {
        assert!(!with(|s| {
            s.dockerenv = true;
            s.containerenv = true;
            s.host_root_mounted = true;
            s.cgroup = Some("0::/docker/x".into());
            s.container_env = Some("docker".into());
            s.override_env = Some("false".into());
        }));
    }

    #[test]
    fn unparsable_override_is_ignored() {
        assert!(with(|s| {
            s.dockerenv = true;
            s.override_env = Some("maybe".into());
        }));
        assert!(!with(|s| s.override_env = Some("maybe".into())));
    }

    #[test]
    fn real_probe_reports_a_custom_host_root_as_mounted() {
        assert!(probe(Path::new("/host")).host_root_mounted);
        assert!(!probe(Path::new("/")).host_root_mounted);
    }
}
