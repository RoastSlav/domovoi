//! `domovoi hub run` as a real process: single-box mode and shutdown on SIGTERM.
//!
//! Every wait here is bounded and the hub is killed when a test ends, however it ends,
//! so a stuck hub fails the test instead of stalling the run.

#![cfg(unix)]

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::path::Path;
    use std::process::{Child, ExitStatus, Stdio};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use assert_cmd::cargo::CommandCargoExt;
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    /// The hub process; killed on drop.
    struct Hub {
        child: Child,
        lines: mpsc::Receiver<String>,
    }

    impl Drop for Hub {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    impl Hub {
        fn start(dir: &Path, local_agent: bool) -> (Self, String) {
            let web = free_port();
            let agents = free_port();
            let config = dir.join("hub.toml");
            std::fs::write(
                &config,
                format!(
                    "[hub]\nname = \"boxhub\"\ndata_dir = '{}'\n[web]\nbind = \"127.0.0.1:{web}\"\n[agent_server]\nbind = \"127.0.0.1:{agents}\"\n",
                    dir.join("data").display()
                ),
            )
            .unwrap();
            let mut command = std::process::Command::cargo_bin("domovoi").unwrap();
            command
                .args(["hub", "run", "--config"])
                .arg(&config)
                .stdout(Stdio::piped())
                .stderr(Stdio::null());
            if local_agent {
                let root = dir.join("root");
                std::fs::create_dir_all(root.join("proc")).unwrap();
                let agent = dir.join("agent.toml");
                std::fs::write(
                    &agent,
                    format!(
                        "[agent]\nhost_name = \"boxhub\"\nhub_url = \"wss://127.0.0.1:{agents}\"\nstate_dir = '{}'\nskills_dir = '{}'\nhost_root = '{}'\n",
                        dir.join("agent-state").display(),
                        dir.join("skills").display(),
                        root.display()
                    ),
                )
                .unwrap();
                command.arg("--with-local-agent").arg(&agent);
            }
            let mut child = command.spawn().unwrap();
            let stdout = child.stdout.take().unwrap();
            let (tx, lines) = mpsc::channel();
            std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    let _ = tx.send(line);
                }
            });
            (Self { child, lines }, format!("http://127.0.0.1:{web}"))
        }

        fn setup_token(&self) -> String {
            let deadline = Instant::now() + Duration::from_secs(30);
            while let Some(left) = deadline.checked_duration_since(Instant::now()) {
                let Ok(line) = self.lines.recv_timeout(left) else {
                    break;
                };
                if let Some((_, token)) = line.split_once("/setup?token=") {
                    return token.trim().to_owned();
                }
            }
            panic!("the hub printed no setup link");
        }

        fn terminate(&mut self) -> (ExitStatus, Duration) {
            let pid = Pid::from_raw(i32::try_from(self.child.id()).unwrap());
            let sent = Instant::now();
            kill(pid, Signal::SIGTERM).unwrap();
            loop {
                if let Some(status) = self.child.try_wait().unwrap() {
                    return (status, sent.elapsed());
                }
                assert!(
                    sent.elapsed() < Duration::from_secs(20),
                    "the hub ignored SIGTERM for 20 seconds"
                );
                let _ = self.lines.recv_timeout(Duration::from_millis(50));
            }
        }
    }

    fn free_port() -> u16 {
        TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    fn client() -> reqwest::Client {
        let _ = rustls::crypto::ring::default_provider().install_default();
        reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap()
    }

    async fn wait_healthy(client: &reqwest::Client, base: &str) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            if let Ok(reply) = client.get(format!("{base}/healthz")).send().await
                && reply.status().is_success()
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("the hub did not become healthy within 30 seconds");
    }

    /// Polls until `check` returns `Some`, for at most 20 seconds.
    async fn eventually<T, F, Fut>(what: &str, mut check: F) -> T
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Option<T>>,
    {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if let Some(found) = check().await {
                return found;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        panic!("{what} did not happen within 20 seconds");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn single_box_hub_and_agent_connect_in_process() {
        let dir = tempfile::tempdir().unwrap();
        let (mut hub, base) = Hub::start(dir.path(), true);
        let token = hub.setup_token();
        let client = client();
        wait_healthy(&client, &base).await;

        let reply = client
            .post(format!("{base}/api/v1/auth/setup"))
            .json(&serde_json::json!({
                "token": token,
                "username": "admin",
                "password": "correct horse battery"
            }))
            .send()
            .await
            .unwrap();
        assert!(reply.status().is_success(), "setup: {}", reply.status());
        let cookie = reply
            .headers()
            .get(reqwest::header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();

        let host = eventually("the local agent connecting", || {
            let client = client.clone();
            let cookie = cookie.clone();
            let base = base.clone();
            async move {
                let hosts: serde_json::Value = client
                    .get(format!("{base}/api/v1/hosts"))
                    .header(reqwest::header::COOKIE, cookie)
                    .send()
                    .await
                    .ok()?
                    .json()
                    .await
                    .ok()?;
                hosts
                    .as_array()?
                    .iter()
                    .find(|h| h["kind"] == "local" && h["connected"] == true)
                    .cloned()
            }
        })
        .await;
        assert_eq!(host["name"], "boxhub");
        assert_eq!(host["version"], env!("CARGO_PKG_VERSION"));

        let metrics = client
            .get(format!("{base}/metrics"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert!(metrics.contains("domovoi_agents_connected 1"), "{metrics}");
        assert!(
            dir.path().join("agent-state/identity.json").exists(),
            "the local agent saved its key"
        );

        let (status, took) = hub.terminate();
        assert!(status.success(), "{status}");
        assert!(took < Duration::from_secs(15), "{took:?}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn sigterm_shuts_down_cleanly_within_15_seconds() {
        let dir = tempfile::tempdir().unwrap();
        let (mut hub, base) = Hub::start(dir.path(), false);
        wait_healthy(&client(), &base).await;
        let (status, took) = hub.terminate();
        assert!(status.success(), "{status}");
        assert!(took < Duration::from_secs(15), "{took:?}");
        // The data directory is free again: an admin command can open it.
        let mut list = assert_cmd::Command::cargo_bin("domovoi").unwrap();
        list.args(["hub", "agent", "list", "--config"])
            .arg(dir.path().join("hub.toml"))
            .timeout(Duration::from_secs(30))
            .assert()
            .success();
    }
}
