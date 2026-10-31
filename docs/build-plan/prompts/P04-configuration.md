# P04 — Configuration

| | |
| --- | --- |
| Phase | 1, Core contracts |
| Depends on | P01 |
| Crates | `domovoi-config`; `examples/` |
| Size | M |
| Invariants touched | INV-5 (secrets are `SecretString`, never inline) |

## Goal

Typed, validated configuration for the hub and the agent, with secret indirection, environment overrides and precise error messages. Ship fully commented example files that are tested.

## Read first

- `ARCHITECTURE.md` §14, §15 (INV-5)
- P01 types (`HostName`, `CheckKey`, `Severity`)

## In scope

Add to `domovoi-config`: `serde`, `toml`, `thiserror`, `secrecy`, `jiff`, `schemars`, `url`. All structs `#[serde(deny_unknown_fields)]` and derive `JsonSchema`.

### Shared

- `Duration` newtype over `jiff::SignedDuration` parsed from friendly strings (`"30s"`, `"5m"`, `"24h"`, `"1h30m"`). Reject negatives and zero where not meaningful.
- `Secret`: TOML forms `{ env = "NAME" }` or `{ file = "/path" }` only; an inline string is a validation error with the message "secrets must use { env = .. } or { file = .. }". Resolved at load into `secrecy::SecretString`. A file readable by group or others produces a warning (returned, not printed).
- `LocalTime` (`"HH:MM"`), `TimeZone` (IANA name validated with jiff).
- `ConfigIssue { path: String (e.g. "llm.primary.model"), message, severity: Error | Warning }`. Loading returns `Result<(Config, Vec<ConfigIssue /* warnings */>), Vec<ConfigIssue>>`.
- Environment overrides: `DOMOVOI_<SECTION>__<KEY>` (double underscore = nesting) for scalar values only; applied after file parsing, before validation. Example: `DOMOVOI_WEB__BIND=0.0.0.0:7420`.

### `HubConfig` (`hub.toml`)

```toml
[hub]          # name, data_dir, timezone
[web]          # bind (default 127.0.0.1:7420), public_url, trusted_proxies = []
[agent_server] # bind (default 0.0.0.0:7421)
[llm]          # primary, secondary (optional): kind = "anthropic" | "openai_compatible" | "cli"
               # model, base_url, api_key (Secret), cli = { preset = "claude" | "codex", command, args }
               # price = { input_per_mtok_usd, output_per_mtok_usd } (optional)
[triage]       # enabled, concurrency (2), per_run = { tool_calls = 15, seconds = 90, output_tokens = 2000 },
               # per_hour_runs = 6, monthly_budget_usd (optional), cache_days = 7
[incidents]    # confirm_runs = 2, reopen_window = "6h", grouping_window = "5m", default_ttl (optional)
[notify]       # quiet_hours = { start = "00:00", end = "09:00" }, renotify = { warning = "24h", critical = "4h" },
               # digest = { time = "09:00", enabled = true }
[discord]      # enabled, bot_token (Secret), channel_id, allowed_user_ids = [], webhook_url (Secret, optional)
[webhooks.<name>] # kind = "generic" | "alertmanager" | "uptime_kuma", auth = "hmac" | "bearer", secret (Secret), default_host
[[probes]]     # host, kind = "tcp" | "http", target, interval, expect_status (http)
[heartbeat]    # url (Secret), interval = "1m"
[backup]       # dir, keep = 7
[secrets]      # exposed = [] (names of secrets the user marked as exposed; feeds an advisory)
```

### `AgentConfig` (`agent.toml`)

```toml
[agent]   # host_name, hub_url (wss://...), state_dir, skills_dir, host_root = "/" (for container mode)
[policy]  # allow_risk = ["read", "write"]  (destructive opt-in)
[docker]  # enabled, endpoint = "unix:///var/run/docker.sock" or "tcp://socket-proxy:2375"
[checks.<check_key>] # enabled, interval, thresholds (check-specific table), criticality overrides
[containers] # critical = [], ignore = [], baseline_expiry = "7d"
[local_webhooks] # bind = "127.0.0.1:7422", token (Secret, optional)
```

Validation rules include: URLs parse; `hub_url` scheme is `wss`; bind addresses parse; quiet hours start ≠ end; `allowed_user_ids` non-empty when Discord is enabled; at least one LLM provider when triage is enabled; `cli` kind requires `cli` block; host names valid.

### Examples

- `examples/hub.toml` and `examples/agent.toml`: every option present, commented with what it does and its default. Secrets use `{ env = ... }`.
- `crates/domovoi-config/tests/examples.rs` loads both (with the referenced env vars set in the test via a scoped helper that does not use `std::env::set_var` — pass an env lookup function into the loader instead).

### API

- `pub fn load_hub(path, env: &dyn EnvSource) -> Result<(HubConfig, Vec<ConfigIssue>), Vec<ConfigIssue>>` and `load_agent(...)`.
- `EnvSource` trait with `SystemEnv` (reads `std::env::var`) and `MapEnv` (tests).
- `HubConfig::json_schema()` / `AgentConfig::json_schema()` for docs.

## Out of scope

- Using the config anywhere (runtimes wire it later).
- Hot reload.

## Tests required

1. `examples_parse_and_validate`.
2. `inline_secret_is_rejected_with_path`.
3. `secret_from_env_and_file_resolves`; `missing_env_secret_reports_path`; `world_readable_secret_file_warns`.
4. `unknown_key_is_rejected_with_path`.
5. `env_override_sets_nested_scalar`.
6. `durations_parse_friendly_strings_and_reject_negative`.
7. One test per validation rule listed above.
8. `secrets_do_not_appear_in_debug_output`: `format!("{:?}", config)` contains no secret value.

## Architecture gates

- Only allowlisted external crates in `domovoi-config`.
- No `std::env::set_var` anywhere (clippy).

## Acceptance criteria

- `cargo xtask ci` passes.
- Error messages name the TOML path and say how to fix the problem.

## Deliverables

- `crates/domovoi-config/src/{lib,hub,agent,secret,duration,env,issue}.rs`
- `examples/hub.toml`, `examples/agent.toml`
- `crates/domovoi-config/tests/*`

## PR

Title: `feat(config): typed hub and agent configuration with secret indirection`
