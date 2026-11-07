//! Finding a container engine on the host: Docker, rootless Docker, Podman and rootless
//! Podman all answer the Docker Engine API on a unix socket.

use std::time::Duration;

/// The engine an endpoint belongs to, judged from its address alone: Podman's sockets live
/// under `podman`, everything else is called Docker.
#[must_use]
pub fn engine_name(endpoint: &str) -> &'static str {
    if endpoint.contains("podman") {
        "Podman"
    } else {
        "Docker"
    }
}

/// The sockets tried in order when no endpoint is configured. `runtime_dir` is
/// `$XDG_RUNTIME_DIR`, where rootless engines put theirs.
#[must_use]
pub fn candidates(runtime_dir: Option<&str>) -> Vec<String> {
    let mut found = vec!["unix:///var/run/docker.sock".to_owned()];
    if let Some(dir) = runtime_dir.filter(|d| d.starts_with('/')) {
        let dir = dir.trim_end_matches('/');
        found.push(format!("unix://{dir}/docker.sock"));
        found.push("unix:///run/podman/podman.sock".to_owned());
        found.push(format!("unix://{dir}/podman/podman.sock"));
    } else {
        found.push("unix:///run/podman/podman.sock".to_owned());
    }
    found
}

/// What detection found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detection {
    /// An engine accepts connections on `endpoint`.
    Found {
        /// The address to use.
        endpoint: String,
        /// `Docker` or `Podman`.
        engine: &'static str,
    },
    /// Nothing answered; `problem` says why in a sentence.
    NotFound {
        /// The address that explains the problem, empty when there was none.
        endpoint: String,
        /// Why no engine could be used.
        problem: String,
    },
}

/// Uses `configured` when given, otherwise the first of [`candidates`] that accepts a
/// connection. This opens and closes a connection and sends nothing.
#[must_use]
pub fn detect(configured: Option<&str>, runtime_dir: Option<&str>) -> Detection {
    if let Some(endpoint) = configured {
        return match reachable(endpoint) {
            Ok(()) => Detection::Found {
                endpoint: endpoint.to_owned(),
                engine: engine_name(endpoint),
            },
            Err(problem) => Detection::NotFound {
                endpoint: endpoint.to_owned(),
                problem,
            },
        };
    }
    let mut refused: Option<(String, String)> = None;
    for endpoint in candidates(runtime_dir) {
        match reachable(&endpoint) {
            Ok(()) => {
                let engine = engine_name(&endpoint);
                return Detection::Found { endpoint, engine };
            }
            // A socket that exists but refuses this user is worth naming over one that is
            // simply not there.
            Err(problem) if refused.is_none() && problem.contains("may not use") => {
                refused = Some((endpoint, problem));
            }
            Err(_) => {}
        }
    }
    let (endpoint, problem) = refused.unwrap_or_else(|| {
        (
            String::new(),
            "no Docker or Podman socket was found".to_owned(),
        )
    });
    Detection::NotFound { endpoint, problem }
}

/// Whether something accepts connections at `endpoint`, or why not.
fn reachable(endpoint: &str) -> Result<(), String> {
    if let Some(path) = endpoint.strip_prefix("unix://") {
        return reachable_unix(path);
    }
    if let Some(address) = endpoint
        .strip_prefix("tcp://")
        .or_else(|| endpoint.strip_prefix("http://"))
    {
        return reachable_tcp(address.trim_end_matches('/'));
    }
    Err(format!("{endpoint:?} is not a unix:// or tcp:// address"))
}

#[cfg(unix)]
fn reachable_unix(path: &str) -> Result<(), String> {
    use std::io::ErrorKind;
    match std::os::unix::net::UnixStream::connect(path) {
        Ok(_) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => {
            Err(format!("there is no socket at {path}"))
        }
        Err(err) if err.kind() == ErrorKind::PermissionDenied => Err(format!(
            "the agent may not use {path}; add its user to the group that owns the socket"
        )),
        Err(err) => Err(format!("{path} does not accept connections: {err}")),
    }
}

#[cfg(not(unix))]
fn reachable_unix(_path: &str) -> Result<(), String> {
    Err("unix sockets need a unix host".to_owned())
}

fn reachable_tcp(address: &str) -> Result<(), String> {
    use std::net::ToSocketAddrs;
    let mut addresses = address
        .to_socket_addrs()
        .map_err(|err| format!("cannot resolve {address}: {err}"))?;
    let Some(first) = addresses.next() else {
        return Err(format!("{address} has no address"));
    };
    std::net::TcpStream::connect_timeout(&first, Duration::from_secs(2))
        .map(drop)
        .map_err(|err| format!("{address} does not accept connections: {err}"))
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::net::UnixListener;

    use super::{Detection, candidates, detect, engine_name};

    #[test]
    fn podman_sockets_are_named_podman() {
        assert_eq!(engine_name("unix:///run/podman/podman.sock"), "Podman");
        assert_eq!(
            engine_name("unix:///run/user/1000/podman/podman.sock"),
            "Podman"
        );
        assert_eq!(engine_name("unix:///var/run/docker.sock"), "Docker");
    }

    #[test]
    fn rootless_sockets_come_from_the_runtime_dir() {
        let found = candidates(Some("/run/user/1000/"));
        assert!(found.contains(&"unix:///run/user/1000/docker.sock".to_owned()));
        assert!(found.contains(&"unix:///run/user/1000/podman/podman.sock".to_owned()));
        assert!(
            candidates(Some("relative"))
                .iter()
                .all(|c| !c.contains("relative")),
            "a relative runtime dir is ignored"
        );
    }

    #[test]
    fn a_configured_endpoint_is_found_when_it_accepts_connections() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("engine.sock");
        let _listener = UnixListener::bind(&socket).unwrap();
        let endpoint = format!("unix://{}", socket.display());
        assert_eq!(
            detect(Some(&endpoint), None),
            Detection::Found {
                endpoint: endpoint.clone(),
                engine: "Docker"
            }
        );
    }

    #[test]
    fn a_missing_socket_is_reported_with_its_path() {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = format!("unix://{}/none.sock", dir.path().display());
        let Detection::NotFound { problem, .. } = detect(Some(&endpoint), None) else {
            panic!("nothing listens there");
        };
        assert!(problem.contains("no socket"), "{problem}");
    }

    #[test]
    fn an_endpoint_that_is_not_an_address_is_refused() {
        let Detection::NotFound { problem, .. } = detect(Some("/var/run/docker.sock"), None) else {
            panic!("a bare path is not an endpoint");
        };
        assert!(problem.contains("unix:// or tcp://"), "{problem}");
    }
}
