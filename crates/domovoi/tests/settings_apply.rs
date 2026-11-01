//! `domovoi hub run` with no config file: settings saved in the web UI are applied by
//! restarting the runtime in the same process, and a change that cannot start is rolled
//! back.
//!
//! Every wait is bounded and the hub is killed when a test ends, so a stuck hub fails the
//! test instead of stalling the run.

#![cfg(unix)]

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::path::Path;
    use std::process::{Child, Stdio};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use assert_cmd::cargo::CommandCargoExt;
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;
    use reqwest::header::{COOKIE, SET_COOKIE};
    use serde_json::{Value, json};

    const PASSWORD: &str = "correct horse battery";
    const HOOK_TOKEN: &str = "hook-token-for-tests";

    struct Hub {
        child: Child,
        lines: mpsc::Receiver<String>,
        base: String,
        client: reqwest::Client,
        cookie: String,
        csrf: String,
    }

    impl Drop for Hub {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    /// A free port below the kernel's ephemeral range (32768 and up on Linux), so an
    /// outgoing connection or a `bind(0)` in another test cannot take it between this
    /// check and the hub binding it.
    fn free_port() -> u16 {
        use std::sync::atomic::{AtomicU32, Ordering};
        static TRIES: AtomicU32 = AtomicU32::new(0);
        loop {
            let n = TRIES.fetch_add(1, Ordering::Relaxed);
            let offset = std::process::id()
                .wrapping_mul(7919)
                .wrapping_add(n.wrapping_mul(104_729))
                % 12_000;
            let port = 20_000 + u16::try_from(offset).unwrap();
            if TcpListener::bind(("127.0.0.1", port)).is_ok() {
                return port;
            }
        }
    }

    /// Starts `hub run` with no config file: the data dir and listeners come from the
    /// environment, everything else from defaults and the web UI.
    fn spawn(dir: &Path, local_agent: bool) -> (Child, mpsc::Receiver<String>, String) {
        assert!(
            !Path::new("/etc/domovoi/hub.toml").exists(),
            "these tests need the default /etc/domovoi/hub.toml to be absent"
        );
        let (web, agents) = (free_port(), free_port());
        let mut command = std::process::Command::cargo_bin("domovoi").unwrap();
        command
            .args(["hub", "run"])
            .env_remove("DOMOVOI_HUB_CONFIG")
            .env("DOMOVOI_HUB__DATA_DIR", dir.join("data"))
            .env("DOMOVOI_WEB__BIND", format!("127.0.0.1:{web}"))
            .env("DOMOVOI_AGENT_SERVER__BIND", format!("127.0.0.1:{agents}"))
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if local_agent {
            std::fs::create_dir_all(dir.join("root/proc")).unwrap();
            let agent = dir.join("agent.toml");
            std::fs::write(
                &agent,
                format!(
                    "[agent]\nhost_name = \"domovoi\"\nhub_url = \"wss://127.0.0.1:{agents}\"\nstate_dir = '{}'\nskills_dir = '{}'\nhost_root = '{}'\n",
                    dir.join("agent-state").display(),
                    dir.join("skills").display(),
                    dir.join("root").display()
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
        (child, lines, format!("http://127.0.0.1:{web}"))
    }

    fn setup_token(lines: &mpsc::Receiver<String>) -> Option<String> {
        let deadline = Instant::now() + Duration::from_secs(30);
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            let Ok(line) = lines.recv_timeout(left) else {
                break;
            };
            if let Some((_, token)) = line.split_once("/setup?token=") {
                return Some(token.trim().to_owned());
            }
        }
        None
    }

    fn client() -> reqwest::Client {
        let _ = rustls::crypto::ring::default_provider().install_default();
        reqwest::Client::builder()
            .timeout(Duration::from_secs(40))
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

    async fn eventually<T, F, Fut>(what: &str, mut check: F) -> T
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Option<T>>,
    {
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            if let Some(found) = check().await {
                return found;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        panic!("{what} did not happen within 30 seconds");
    }

    impl Hub {
        /// Starts a hub and creates the admin account. The hub prints its setup link
        /// before it binds its ports, so when a port was taken after `free_port` checked
        /// it, the hub exits and another hub may answer on that port: start again on
        /// new ports.
        async fn start(dir: &Path, local_agent: bool) -> Self {
            let client = client();
            for attempt in 1..=5 {
                let (mut child, lines, base) = spawn(dir, local_agent);
                let token = setup_token(&lines);
                let exited = |child: &mut Child| child.try_wait().unwrap().is_some();
                let Some(token) = token else {
                    assert!(exited(&mut child), "the hub printed no setup link");
                    let _ = child.wait();
                    continue;
                };
                wait_healthy(&client, &base).await;
                let reply = client
                    .post(format!("{base}/api/v1/auth/setup"))
                    .json(&json!({ "token": token, "username": "admin", "password": PASSWORD }))
                    .send()
                    .await
                    .unwrap();
                if !reply.status().is_success() {
                    // Give an exit that is under way a moment to show.
                    let _ = lines.recv_timeout(Duration::from_millis(200));
                    assert!(
                        exited(&mut child) && attempt < 5,
                        "setup: {}",
                        reply.status()
                    );
                    let _ = child.wait();
                    continue;
                }
                return Self::logged_in(child, lines, base, client, reply).await;
            }
            panic!("the hub did not start in five attempts");
        }

        async fn logged_in(
            child: Child,
            lines: mpsc::Receiver<String>,
            base: String,
            client: reqwest::Client,
            reply: reqwest::Response,
        ) -> Self {
            let cookie = reply.headers()[SET_COOKIE]
                .to_str()
                .unwrap()
                .split(';')
                .next()
                .unwrap()
                .to_owned();
            let body: Value = reply.json().await.unwrap();
            let csrf = body["csrf_token"].as_str().unwrap().to_owned();
            Self {
                child,
                lines,
                base,
                client,
                cookie,
                csrf,
            }
        }

        async fn get(&self, path: &str) -> Value {
            self.client
                .get(format!("{}{path}", self.base))
                .header(COOKIE, &self.cookie)
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap()
        }

        async fn revision(&self) -> u64 {
            self.get("/api/v1/settings").await["editable"]["revision"]
                .as_u64()
                .unwrap()
        }

        async fn save(&self, changes: Value, password: bool) -> (u16, Value) {
            let mut body = json!({ "base_revision": self.revision().await, "changes": changes });
            if password {
                body["password"] = json!(PASSWORD);
            }
            let reply = self
                .client
                .patch(format!("{}/api/v1/settings", self.base))
                .header(COOKIE, &self.cookie)
                .header("x-csrf-token", &self.csrf)
                .json(&body)
                .send()
                .await
                .unwrap();
            let status = reply.status().as_u16();
            (status, reply.json().await.unwrap_or(Value::Null))
        }

        async fn local_agent_connected(&self) {
            eventually("the local agent connecting", || async {
                let hosts = self.get("/api/v1/hosts").await;
                hosts
                    .as_array()?
                    .iter()
                    .any(|h| h["kind"] == "local" && h["connected"] == true)
                    .then_some(())
            })
            .await;
        }

        fn stop(&mut self) {
            let pid = Pid::from_raw(i32::try_from(self.child.id()).unwrap());
            kill(pid, Signal::SIGTERM).unwrap();
            let deadline = Instant::now() + Duration::from_secs(20);
            while Instant::now() < deadline {
                if let Some(status) = self.child.try_wait().unwrap() {
                    assert!(status.success(), "{status}");
                    return;
                }
                let _ = self.lines.recv_timeout(Duration::from_millis(50));
            }
            panic!("the hub ignored SIGTERM");
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn hub_starts_without_config_file_and_prints_setup_url() {
        let dir = tempfile::tempdir().unwrap();
        let mut hub = Hub::start(dir.path(), false).await;
        let settings = hub.get("/api/v1/settings").await;
        assert_eq!(settings["editable"]["revision"], 0);
        assert_eq!(settings["hub"]["name"], "domovoi", "the default");
        assert!(dir.path().join("data/secret.key").exists());
        hub.stop();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn apply_restarts_runtime_with_new_config_on_same_store() {
        let dir = tempfile::tempdir().unwrap();
        let mut hub = Hub::start(dir.path(), true).await;
        hub.local_agent_connected().await;

        let webhook = json!({ "webhooks": { "alerts": {
            "kind": "generic", "auth": "bearer", "secret": { "set": HOOK_TOKEN }
        }}});
        let (status, body) = hub.save(webhook, true).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["applied"], true);
        wait_healthy(&hub.client, &hub.base).await;

        let hook = hub
            .client
            .post(format!("{}/hooks/alerts", hub.base))
            .bearer_auth(HOOK_TOKEN)
            .json(&json!({ "host": "domovoi", "check": "backup.nightly", "severity": "critical", "summary": "backup failed" }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            hook.status().as_u16(),
            202,
            "the restarted hub serves the new webhook"
        );
        let incident = eventually("the incident opening", || async {
            let page = hub.get("/api/v1/incidents").await;
            page["items"].as_array()?.first().cloned()
        })
        .await;

        let (status, body) = hub
            .save(json!({ "incidents": { "confirm_runs": 3 } }), false)
            .await;
        assert_eq!(status, 200, "{body}");
        wait_healthy(&hub.client, &hub.base).await;
        let settings = hub.get("/api/v1/settings").await;
        assert_eq!(
            settings["editable"]["values"]["incidents"]["confirm_runs"],
            3
        );
        let page = hub.get("/api/v1/incidents").await;
        assert_eq!(
            page["items"][0]["id"], incident["id"],
            "the incident survived the restart"
        );
        hub.local_agent_connected().await;
        hub.stop();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn failed_apply_rolls_back_to_previous_and_raises_advisory() {
        let dir = tempfile::tempdir().unwrap();
        let mut hub = Hub::start(dir.path(), false).await;
        let (status, _) = hub
            .save(
                json!({ "notify": { "digest": { "time": "07:00" } } }),
                false,
            )
            .await;
        assert_eq!(status, 200);
        wait_healthy(&hub.client, &hub.base).await;

        // Valid as settings, but the CLI model cannot be built: the preset's default
        // program is not an absolute path.
        let broken = json!({
            "llm": { "primary": { "kind": "cli", "cli": { "preset": "claude" } } },
            "triage": { "enabled": true }
        });
        let (status, body) = hub.save(broken, false).await;
        assert_eq!(status, 400, "{body}");
        assert!(
            body["message"]
                .as_str()
                .unwrap()
                .contains("previous ones are back"),
            "{body}"
        );
        let settings = hub.get("/api/v1/settings").await;
        assert!(
            settings["editable"]["values"].get("discord").is_none(),
            "rolled back"
        );
        assert_eq!(
            settings["editable"]["values"]["notify"]["digest"]["time"],
            "07:00"
        );
        assert_eq!(settings["editable"]["last_apply"]["ok"], false);
        let advisories = hub.get("/api/v1/advisories").await;
        assert!(
            advisories
                .as_array()
                .unwrap()
                .iter()
                .any(|a| a["kind"] == "settings_apply_failed"),
            "{advisories}"
        );
        hub.stop();
    }
}
