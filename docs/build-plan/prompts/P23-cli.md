# P23 — CLI, single-box mode, metrics and selftest

| | |
| --- | --- |
| Phase | 6, Ship |
| Depends on | P22 |
| Crates | `domovoi` (binary) |
| Size | M |
| Invariants touched | INV-2 (executor injection), INV-11 (selftest proves delivery) |

## Goal

Ship the `domovoi` command line: run the hub or the agent, enroll agents, manage tokens and passwords, validate configs and skills, dry-run checks, test notifications, run an end-to-end selftest, send the digest, and run both roles in one process for single-box installs. Initialise TLS crypto, logging and metrics once.

## Read first

- `ARCHITECTURE.md` §7.3, R6
- P09 `AgentRuntime`, P13 functions, P22 `HubRuntime`

## In scope

Add `tokio` (full), `tokio-util`, `tracing`, `tracing-subscriber`, `rustls`, `metrics`, `metrics-exporter-prometheus`, `jiff`. Keep `main.rs` small: one module per subcommand under `src/cmd/`.

### Process start-up (`src/init.rs`)

- Install `rustls::crypto::ring::default_provider()` as the process-wide default exactly once; fail clearly if another provider was installed first.
- Tracing: `--log-format human|json` (default human), `--log-level` or `RUST_LOG`; secrets never logged.
- Metrics: install the Prometheus recorder; the hub's `/metrics` route renders it. Record: `domovoi_signals_total{source}`, `domovoi_incidents{status}`, `domovoi_triage_runs_total{outcome}`, `domovoi_llm_tokens_total{provider,direction}`, `domovoi_llm_cost_usd_total`, `domovoi_actions_total{status}`, `domovoi_agents_connected`, `domovoi_delivery_failures_total{gateway}`. Add the missing `metrics::` calls in hub and engine where these events happen.
- Signals: SIGINT/SIGTERM cancel the shared `CancellationToken`.

### Subcommands

| Command | Behaviour |
| --- | --- |
| `domovoi hub run --config hub.toml [--with-local-agent agent.toml]` | build and run `HubRuntime`; with a local agent, run `AgentRuntime` in-process connected through an in-memory transport |
| `domovoi hub token create <host> [--ttl 15m]` | print the enrollment command, fingerprint and expiry |
| `domovoi hub agent list` / `revoke <host>` | presence table / revoke |
| `domovoi hub set-password` | prompt twice without echo, update the admin password |
| `domovoi agent run --config agent.toml` | run `AgentRuntime` with the runner's `SandboxExecutor`, real `HostFs` and Docker source |
| `domovoi agent enroll --hub <url> --token <t> --fingerprint <fp> --config agent.toml` | enrollment (P09) |
| `domovoi check config <file> [--kind hub|agent]` | load and validate; print every issue with its path; exit 1 on errors |
| `domovoi check skills <dir>` | load a skills folder; print valid skills and issues |
| `domovoi check run --config agent.toml [--only <check>] [--json]` | run every enabled check once locally and print observations and facts; no hub needed (parity with a script's `--dry-run`) |
| `domovoi notify test --config hub.toml` | send a test card to each enabled gateway and a test message to each notifier |
| `domovoi selftest --config hub.toml [--offline]` | push a synthetic critical signal through manager → triage → card → gateway; `--offline` uses a built-in scripted provider and a recording gateway; print PASS/FAIL per stage |
| `domovoi digest now --config hub.toml` | build and send today's digest |
| `domovoi openapi` (hidden) | from P18 |
| `domovoi version` | version, git commit, build date, target |

Commands that talk to a running hub's database (`token`, `agent list`, `revoke`, `set-password`, `digest now`) open the store directly with a write lock and refuse if the store is locked by a running hub, telling the user to use the web UI instead (or, simpler, call the local API with an admin session: pick one approach and document it).

### Single-box mode (`src/local_link.rs`)

- `ChannelTransport` pair over `tokio::sync::mpsc` implementing `domovoi_proto::Transport`. The hub side attaches through `AgentServer::attach_local`; the agent side runs the normal handshake with a key generated on first start and enrolled automatically (local trust).

## Out of scope

- Packaging and install scripts (P24).

## Tests required

`assert_cmd` + `predicates`, temp dirs:

1. `check_config_accepts_examples_and_reports_paths_for_errors`.
2. `check_skills_lists_valid_and_invalid`.
3. `check_run_executes_checks_against_a_fixture_root` (agent config with `host_root` pointing to a fixture tree, Docker disabled).
4. `selftest_offline_passes_all_stages`.
5. `version_prints_target_and_commit`.
6. `single_box_hub_and_agent_connect_in_process` (start `hub run --with-local-agent`, wait for `/healthz`, query agents through the API, cancel).
7. `sigterm_shuts_down_cleanly_within_15_seconds`.
8. `crypto_provider_is_installed_once`.

## Architecture gates

- The binary is the only crate that wires `domovoi-runner` into the hub (R6).
- No business logic in `main.rs`; subcommands call library APIs.

## Acceptance criteria

- `cargo xtask ci` passes.
- `docs/book/src/reference/cli.md` documents every command with an example.

## Deliverables

- `crates/domovoi/src/{main,init,local_link,cmd/*}.rs`
- `crates/domovoi/tests/*`, `docs/book/src/reference/cli.md`

## PR

Title: `feat(cli): hub and agent commands, single-box mode, metrics and selftest`
