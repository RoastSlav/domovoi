# Domovoi repository inventory

Factual inventory of the repository at branch `overhaul-investigation` (HEAD `64117f0`). Read-only snapshot. Paths are relative to `C:\Programming\domovoi`. Counts are grep or wc based and approximate where noted.

Method notes:
- LOC counts every `*.rs` under each `src/` tree, excluding `tests/` directories and files named `tests.rs`. Inline `#[cfg(test)]` modules inside `src/` files are included, so the numbers slightly overstate production code.
- Test counts are `grep` counts of `#[test` and `#[tokio::test` attributes under each crate directory (`src/` and `tests/`). They match attribute text, not executed tests.
- Dependency lists are taken from each `Cargo.toml`, split into `[dependencies]` and `[dev-dependencies]`.

---

## 1. Crates

Workspace members: 13 crates under `crates/` plus `xtask/`.

### crates/domovoi (binary)
- Purpose (`crates/domovoi/src/main.rs`): "The `domovoi` binary: hub, agent and CLI tools in one executable." Outer composition root; no business logic.
- Normal deps: domovoi-agent, domovoi-config, domovoi-core, domovoi-hub, domovoi-runner
- Dev deps: domovoi-checks, domovoi-proto, domovoi-testkit
- Top-level modules: `cmd/` (dir), `init.rs`, `local_link.rs`, `log_ring.rs`, `main.rs`
- Non-test LOC: ~2104

### crates/domovoi-agent
- Purpose (`crates/domovoi-agent/src/lib.rs`): runs on every guarded host; keeps the outbound link to the hub, schedules built-in checks, re-validates and serves tool and skill requests through the runner, spools signals while the hub is unreachable.
- Normal deps: domovoi-checks (features host, net, sources), domovoi-config, domovoi-core, domovoi-proto, domovoi-runner
- Dev deps: domovoi-testkit
- Top-level modules: `checks.rs`, `config_file.rs`, `docker.rs`, `dry_run.rs`, `enroll.rs`, `error.rs`, `facts.rs`, `fs_atomic.rs`, `identity.rs`, `lib.rs`, `link/` (dir), `local.rs`, `overrides.rs`, `production.rs`, `relay.rs`, `requests/` (dir), `runtime.rs`, `scheduler.rs`, `spool.rs`, `store.rs`, `time.rs`
- Non-test LOC: ~4569

### crates/domovoi-chat
- Purpose (`crates/domovoi-chat/src/lib.rs`): chat gateway and notifier adapters; translates `Card`s into platform messages and platform events into `Interaction`s. Authorization happens in the hub.
- Normal deps: domovoi-core
- Dev deps: domovoi-testkit
- Top-level modules: `discord/` (dir), `lib.rs`
- Non-test LOC: ~1485

### crates/domovoi-checks
- Purpose (`crates/domovoi-checks/src/lib.rs`): built-in checks and network probes. Host checks (feature `host`) read files through `HostFs` and run commands only through `ProcessExecutor`. Network probes (feature `net`). Pure parsers for webhook payloads (feature `sources`).
- Normal deps: domovoi-core
- Dev deps: domovoi-testkit
- Top-level modules: `commands.rs`, `containers/`, `framework/`, `hostfs.rs`, `learn.rs`, `lib.rs`, `net/`, `security/`, `smart/`, `sources/`, `system/`, `units/`
- Non-test LOC: ~6525

### crates/domovoi-config
- Purpose (`crates/domovoi-config/src/lib.rs`): typed configuration for hub and agent. Loads `hub.toml` and `agent.toml`, applies `DOMOVOI_*` env overrides, resolves secret indirection (`{ env = ".." }` or `{ file = ".." }`), reports validation problems with TOML paths.
- Normal deps: domovoi-core
- Dev deps: none
- Top-level modules: `agent.rs`, `duration.rs`, `env.rs`, `hub/` (dir), `issue.rs`, `layered.rs`, `lib.rs`, `provenance.rs`, `schema.rs`, `secret.rs`, `snowflake.rs`, `time.rs`, `validate.rs`
- Non-test LOC: ~2845

### crates/domovoi-core
- Purpose (`crates/domovoi-core/src/lib.rs`): domain types, state machines and ports (traits). Pure: no I/O, no async runtime, no adapters.
- Normal deps: none (workspace-internal)
- Dev deps: none
- Top-level modules: `ack.rs`, `action/` (dir), `addr.rs`, `advisory.rs`, `baseline.rs`, `card.rs`, `digest.rs`, `error.rs`, `fingerprint.rs`, `history.rs`, `host.rs`, `id.rs`, `incident/` (dir), `interaction.rs`, `lib.rs`, `note.rs`, `notify.rs`, `ports/` (dir), `severity.rs`, `sha256.rs`, `signal/` (dir), `skill/` (dir), `triage/` (dir), `user.rs`, `verdict.rs`
- Non-test LOC: ~6691

### crates/domovoi-engine
- Purpose (`crates/domovoi-engine/src/lib.rs`): application services: correlation, single-writer incident manager, notification policy, triage orchestration, approvals, digests. Depends only on `domovoi-core` ports.
- Normal deps: domovoi-core
- Dev deps: domovoi-llm, domovoi-testkit
- Top-level modules: `actions/` (dir), `advisories.rs`, `cards.rs`, `correlate.rs`, `digest/` (dir), `events.rs`, `lib.rs`, `manager/` (dir), `notify/` (dir), `policy.rs`, `redact.rs`, `render.rs`, `suppress.rs`, `telemetry.rs`, `triage/` (dir)
- Non-test LOC: ~8457
- Note: `crates/domovoi-engine/prompts/` also exists (not Rust).

### crates/domovoi-hub
- Purpose (`crates/domovoi-hub/src/lib.rs`): serves the HTTP API, SSE and the embedded web UI, authenticates users and chat interactions, ingests webhooks, runs the agent server, wires the engine to store, LLM and chat adapters. Never spawns processes.
- Normal deps: domovoi-chat, domovoi-checks (features net, sources), domovoi-config, domovoi-core, domovoi-engine, domovoi-llm, domovoi-proto, domovoi-store
- Dev deps: domovoi-testkit
- Top-level modules: `admin/` (dir), `agent_server/` (dir), `api/` (dir), `auth/` (dir), `csp.rs`, `digest.rs`, `ingest/` (dir), `lib.rs`, `middleware.rs`, `profile/` (dir), `router.rs`, `runtime/` (dir), `selftest/` (dir), `settings/` (dir), `spa.rs`
- Also present: `crates/domovoi-hub/examples/` (dir, `dev_hub` example per AGENTS.md)
- Non-test LOC: ~17137 (largest crate)

### crates/domovoi-llm
- Purpose (`crates/domovoi-llm/src/lib.rs`): LLM provider adapters: Anthropic Messages API, OpenAI-compatible Chat Completions, and a CLI provider through an injected `ProcessExecutor`.
- Normal deps: domovoi-core
- Dev deps: domovoi-testkit
- Top-level modules: `anthropic.rs`, `anthropic_stream.rs`, `cli.rs`, `error.rs`, `factory.rs`, `http.rs`, `lib.rs`, `models.rs`, `openai.rs`, `openai_stream.rs`, `settings.rs`, `sse.rs`
- Non-test LOC: ~1741

### crates/domovoi-proto
- Purpose (`crates/domovoi-proto/src/lib.rs`): hub and agent wire protocol: JSON envelope, every message kind over the agent link, protocol version rules. Pure data.
- Normal deps: domovoi-core
- Dev deps: none
- Top-level modules: `auth.rs`, `bytes.rs`, `envelope.rs`, `error.rs`, `lib.rs`, `messages/` (dir), `transport.rs`, `version.rs`
- Non-test LOC: ~1324

### crates/domovoi-runner
- Purpose (`crates/domovoi-runner/src/lib.rs`): skill manifests and sandboxed process execution. The only crate allowed to spawn processes (R3, ADR 0004). Argv vectors, dedicated user, cleared env, timeout, capped output.
- Normal deps: domovoi-core
- Dev deps: domovoi-testkit
- Top-level modules: `catalog/` (dir), `error.rs`, `exec/` (dir), `lib.rs`, `skill_run.rs`
- Non-test LOC: ~936

### crates/domovoi-store
- Purpose (`crates/domovoi-store/src/lib.rs`): implements repository and audit ports from `domovoi-core` on SQLite (WAL) via sqlx, with embedded append-only migrations.
- Normal deps: domovoi-core
- Dev deps: domovoi-testkit
- Top-level modules: `backup.rs`, `codec.rs`, `db.rs`, `error.rs`, `fts.rs`, `lib.rs`, `purge.rs`, `repos/` (dir)
- Non-test LOC: ~2533

### crates/domovoi-testkit
- Purpose (`crates/domovoi-testkit/src/lib.rs`): test support. In-memory implementations of every core port, fake clock, scripted LLM responses, recording chat gateway, fixture loaders. Dev-dependency only.
- Normal deps: domovoi-core, domovoi-proto
- Dev deps: none
- Top-level modules: `approvals.rs`, `build.rs`, `chat.rs`, `clock.rs`, `contracts/` (dir), `fs.rs`, `lib.rs`, `link.rs`, `llm.rs`, `logs.rs`, `process.rs`, `redact.rs`, `repos/` (dir). Also `fixtures/` (dir).
- Non-test LOC: ~4244

### xtask
- Purpose (`xtask/src/main.rs`): developer tasks: `arch-check [--base <ref>]`, `ci [--fast] [--web]`, `openapi [--check]`, `docs-config [--check]`, `docs-links`. Not part of the product; may spawn processes.
- Internal domovoi deps: none (`xtask/Cargo.toml`)
- Top-level modules: `arch/` (dir), `ci.rs`, `docs_config.rs`, `docs_links.rs`, `main.rs`, `openapi.rs`
- Non-test LOC: ~2093

### Internal dependency graph (normal deps only)
- domovoi -> agent, config, core, hub, runner
- hub -> chat, checks, config, core, engine, llm, proto, store
- agent -> checks, config, core, proto, runner
- engine -> core
- chat, checks, config, llm, proto, runner, store -> core
- proto -> core
- testkit -> core, proto
- core -> (none)

---

## 2. HTTP API (crates/domovoi-hub)

Routes are built from `utoipa` `#[utoipa::path]` attributes registered through `routes!()` in `crates/domovoi-hub/src/api/mod.rs` (fn `routes()`, starts at line ~305). Router composition: `crates/domovoi-hub/src/router.rs` (`router()`). Ingest is registered separately in `crates/domovoi-hub/src/ingest/mod.rs:69`. The SPA is the fallback (`spa::handler`).

Line numbers are the `#[utoipa::path(` attribute line in the handler's file. The handler column is the Rust fn name.

Total: 58 API routes (utoipa-documented) + 1 webhook route + SPA fallback.

| Method | Path | Handler | file:line |
|---|---|---|---|
| GET | /api/v1/ack-rules | list | crates/domovoi-hub/src/api/rules.rs:269 |
| POST | /api/v1/ack-rules | create | crates/domovoi-hub/src/api/rules.rs:316 |
| DELETE | /api/v1/ack-rules/{id} | delete | crates/domovoi-hub/src/api/rules.rs:391 |
| GET | /api/v1/actions | list | crates/domovoi-hub/src/api/actions.rs:39 |
| POST | /api/v1/actions/{id}/approve | approve | crates/domovoi-hub/src/api/actions.rs:91 |
| POST | /api/v1/actions/{id}/deny | deny | crates/domovoi-hub/src/api/actions.rs:127 |
| GET | /api/v1/advisories | advisories | crates/domovoi-hub/src/api/system.rs:14 |
| POST | /api/v1/auth/login | login | crates/domovoi-hub/src/api/auth.rs:48 |
| POST | /api/v1/auth/logout | logout | crates/domovoi-hub/src/api/auth.rs:69 |
| POST | /api/v1/auth/setup | setup | crates/domovoi-hub/src/api/auth.rs:89 |
| POST | /api/v1/chat | chat | crates/domovoi-hub/src/api/chat.rs:63 |
| GET | /api/v1/digest/preview | digest_preview | crates/domovoi-hub/src/api/system.rs:82 |
| POST | /api/v1/digest/send | digest_send | crates/domovoi-hub/src/api/system.rs:97 |
| GET | /api/v1/events | stream (SSE) | crates/domovoi-hub/src/api/events.rs:88 |
| GET | /api/v1/hosts | list | crates/domovoi-hub/src/api/hosts.rs:93 |
| GET | /api/v1/hosts/{name} | detail | crates/domovoi-hub/src/api/hosts.rs:139 |
| GET | /api/v1/hosts/{name}/checks | host_checks | crates/domovoi-hub/src/api/settings_write.rs:324 |
| PUT | /api/v1/hosts/{name}/checks | set_host_checks | crates/domovoi-hub/src/api/settings_write.rs:374 |
| GET | /api/v1/hosts/{name}/docker | host_docker | crates/domovoi-hub/src/api/host_docker.rs:75 |
| PUT | /api/v1/hosts/{name}/docker | set_host_docker | crates/domovoi-hub/src/api/host_docker.rs:96 |
| POST | /api/v1/hosts/{name}/enrollment | enrollment | crates/domovoi-hub/src/api/hosts.rs:167 |
| GET | /api/v1/hosts/{name}/logs | logs | crates/domovoi-hub/src/api/hosts.rs:289 |
| GET | /api/v1/hosts/{name}/profile | get | crates/domovoi-hub/src/api/profile.rs:72 |
| POST | /api/v1/hosts/{name}/profile | refresh | crates/domovoi-hub/src/api/profile.rs:97 |
| POST | /api/v1/hosts/{name}/revoke | revoke | crates/domovoi-hub/src/api/hosts.rs:199 |
| GET | /api/v1/hosts/{name}/skills | skills | crates/domovoi-hub/src/api/hosts.rs:227 |
| PUT | /api/v1/hosts/{name}/skills/{skill} | set_skill_enabled | crates/domovoi-hub/src/api/settings_write.rs:433 |
| GET | /api/v1/incidents | list | crates/domovoi-hub/src/api/incidents.rs:53 |
| GET | /api/v1/incidents/{id} | detail | crates/domovoi-hub/src/api/incidents.rs:148 |
| POST | /api/v1/incidents/{id}/ack | ack | crates/domovoi-hub/src/api/incidents.rs:215 |
| POST | /api/v1/incidents/{id}/ask | ask | crates/domovoi-hub/src/api/incidents.rs:410 |
| POST | /api/v1/incidents/{id}/dismiss | dismiss | crates/domovoi-hub/src/api/incidents.rs:308 |
| GET | /api/v1/incidents/{id}/followups | history | crates/domovoi-hub/src/api/followups.rs:44 |
| POST | /api/v1/incidents/{id}/followups/clear | clear | crates/domovoi-hub/src/api/followups.rs:80 |
| POST | /api/v1/incidents/{id}/mute | mute | crates/domovoi-hub/src/api/incidents.rs:240 |
| POST | /api/v1/incidents/{id}/triage | retry_triage | crates/domovoi-hub/src/api/incidents.rs:457 |
| POST | /api/v1/incidents/{id}/unmute | unmute | crates/domovoi-hub/src/api/incidents.rs:276 |
| GET | /api/v1/notes | list | crates/domovoi-hub/src/api/notes.rs:67 |
| POST | /api/v1/notes | create | crates/domovoi-hub/src/api/notes.rs:87 |
| POST | /api/v1/notes/import | import | crates/domovoi-hub/src/api/notes_import.rs:115 |
| GET | /api/v1/notes/search | search | crates/domovoi-hub/src/api/notes.rs:107 |
| GET | /api/v1/notes/{id} | get | crates/domovoi-hub/src/api/notes.rs:134 |
| PUT | /api/v1/notes/{id} | replace | crates/domovoi-hub/src/api/notes.rs:158 |
| DELETE | /api/v1/notes/{id} | delete | crates/domovoi-hub/src/api/notes.rs:185 |
| GET | /api/v1/overview | show | crates/domovoi-hub/src/api/overview.rs:84 |
| GET | /api/v1/session | session | crates/domovoi-hub/src/api/auth.rs:35 |
| GET | /api/v1/settings | show | crates/domovoi-hub/src/api/settings.rs:226 |
| PATCH | /api/v1/settings | update | crates/domovoi-hub/src/api/settings_write.rs:157 |
| GET | /api/v1/settings/schema | schema | crates/domovoi-hub/src/api/settings_write.rs:194 |
| POST | /api/v1/settings/models | list_models | crates/domovoi-hub/src/api/settings_write.rs:268 |
| POST | /api/v1/settings/test/discord | test_discord | crates/domovoi-hub/src/api/settings_write.rs:227 |
| POST | /api/v1/settings/test/heartbeat | test_heartbeat | crates/domovoi-hub/src/api/settings_write.rs:297 |
| POST | /api/v1/settings/test/llm | test_llm | crates/domovoi-hub/src/api/settings_write.rs:246 |
| GET | /api/v1/system/logs | list | crates/domovoi-hub/src/api/logs.rs:22 |
| GET | /api/v1/system/update | show | crates/domovoi-hub/src/api/updates.rs:110 |
| POST | /api/v1/system/update/check | check | crates/domovoi-hub/src/api/updates.rs:123 |
| GET | /healthz | healthz | crates/domovoi-hub/src/api/system.rs:129 |
| GET | /metrics | metrics | crates/domovoi-hub/src/api/system.rs:138 |
| POST | /hooks/{name} | receive (ingest, not utoipa) | crates/domovoi-hub/src/ingest/mod.rs:69 |

Other HTTP surface:
- Fallback: `crates/domovoi-hub/src/spa.rs` `handler` serves the embedded web UI for every other path.
- CSRF guard covers the API routes (`crates/domovoi-hub/src/router.rs`, `auth::csrf::guard`). Webhooks authenticate themselves.
- Body limit: `middleware::MAX_BODY_BYTES` (router default); `notes/import` has its own larger limit (`notes_import::MAX_REQUEST_BYTES`, `api/mod.rs` ~line 378).
- Agent-facing protocol is not HTTP: it runs over WebSocket via `crates/domovoi-hub/src/agent_server/` (see section 4).

---

## 3. Database (crates/domovoi-store/migrations)

Migrations are plain SQL, applied in filename order. Five files, 331 lines total. No migration after 0005 exists.

### 0001_init.sql (282 lines)
Creates (STRICT tables unless noted):
- `hosts`: id, name, kind, labels, created_at
- `agents`: agent_id, host_id, public_key, version, enrolled_at, revoked_at; unique partial index `agents_one_live_per_host` on host_id where revoked_at is null
- `enrollment_tokens`: token_hash, host_id, expires_at, used_at
- `incidents`: id, fingerprint, host_id, check_key, target, title, summary_text, status, severity, opened_at, resolved_at, body; unique partial index `incidents_one_active_per_fingerprint`; indexes on fingerprint/resolved_at, opened_at, host/opened_at, status
- `incidents_fts` (FTS5 virtual table) with triggers `incidents_fts_insert`, `incidents_fts_delete`, `incidents_fts_update`
- `signals`: id, host_id, check_key, target, status, severity, observed_at, body; index `signals_observed`
- `incident_signals`: incident_id, signal_id; index `incident_signals_signal`
- `incident_events`: seq, incident_id, at, kind, summary, detail; indexes `incident_events_incident`, `incident_events_at`
- `actions`: id, incident_id, host_id, skill, risk, args, plan_hash, status, proposed_by_kind, proposed_by, approved_by, approved_channel, approved_at, expires_at, created_at, updated_at, chat_token_hash; indexes on created_at, incident, host
- `audit_log`: seq (autoincrement PK), at, actor, kind, subject, detail; indexes on actor, kind, subject, at; triggers `audit_log_no_update`, `audit_log_no_delete`, `audit_log_no_replace` (append-only, INV-7)
- `ack_rules`: id, host_id, check_key, target_pattern, predicates, note, expires_at
- `maintenance_windows`: host_id, starts_at, ends_at, note; index `maintenance_windows_ends`
- `baselines`: host_id, kind, key, criticality, learned_at, last_seen_at, managed
- `notes`: id, host_id, title, body, tags, updated_at; index `notes_host`
- `notes_fts` (FTS5 virtual table) with triggers `notes_fts_insert`, `notes_fts_delete`, `notes_fts_update`
- `triage_cache`: fingerprint, evidence_digest (32-byte blob), report, stored_at; index `triage_cache_stored`
- `llm_usage`: id, provider, tokens_in, tokens_out, cost_micros, at; index `llm_usage_at`
- `digest_facts`: id, host_id, key, label, value, unit, observed_at; index `digest_facts_host_observed`
- `advisories`: id, kind (CHECK list), host_id, title, detail, created_at, resolved_at; index `advisories_open`
- `users`: id, username (unique), password_hash, created_at
- `sessions`: token_hash (32-byte blob PK), user_id, csrf_hash, created_at, expires_at; indexes `sessions_user`, `sessions_expires`
- `settings`: key (PK), value

### 0002_notify_state.sql (9 lines)
- Creates `notify_state`: incident_id (PK), held, state, updated_at; index `notify_state_held`

### 0003_delivery_failed_advisory.sql (17 lines)
- Rebuilds `advisories` via `advisories_new` (copy, drop, rename) to extend the `kind` CHECK with `delivery_failed`; recreates index `advisories_open`

### 0004_settings_advisories.sql (20 lines)
- Rebuilds `advisories` again via `advisories_new` to extend the `kind` CHECK with `stored_secret_unreadable` and `settings_apply_failed`; recreates index `advisories_open`

### 0005_ack_rule_creator.sql (3 lines)
- `ALTER TABLE ack_rules ADD COLUMN created_at INTEGER`
- `ALTER TABLE ack_rules ADD COLUMN created_by TEXT`

Migration runner: `crates/domovoi-store/src/db.rs`. `Store::open` backs up a database with pending migrations before applying them (`crates/domovoi-store/src/lib.rs` doc comment).

---

## 4. Protocol (crates/domovoi-proto)

### Version constants (`crates/domovoi-proto/src/version.rs`)
- `pub struct ProtocolVersion` at line 9
- `pub const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion { major: 1, minor: 1 }` at line 17
- `pub struct Release` at line 39
- `pub const MIN_AGENT_VERSION: Release` at line 49
- `pub const MIN_HUB_VERSION: Release` at line 56
- `pub const AGENT_TOO_OLD_REASON_SINCE: Release` at line 66

Other constants:
- `MAX_FRAME_BYTES = 1 << 20` at `crates/domovoi-proto/src/envelope.rs:11`
- `CHALLENGE_CONTEXT = b"domovoi-agent-auth-v1\0"` at `crates/domovoi-proto/src/auth.rs:8`
- Envelope check: `crates/domovoi-proto/src/envelope.rs:82` rejects `envelope.v != PROTOCOL_VERSION.major`

### Envelope
- `pub struct Envelope` at `crates/domovoi-proto/src/envelope.rs:18` (field `kind` selects the `Message` variant)
- `pub struct Decoded` at `envelope.rs:35`
- `ProtoError` at `crates/domovoi-proto/src/error.rs:5`
- `TransportError` at `crates/domovoi-proto/src/transport.rs:7`

### Message enum (`crates/domovoi-proto/src/messages/mod.rs`)
Declared by the `messages!` macro; `pub enum Message` at line 31. Variant = kind literal (`$variant($ty) = $kind`), with an `Unknown { kind }` variant for unrecognised kinds (receiver replies `unsupported`). 23 known kinds:

| Variant | Payload type (file) | kind | Line |
|---|---|---|---|
| Enroll | Enroll (handshake.rs) | enroll | mod.rs:80 |
| Enrolled | Enrolled (handshake.rs) | enrolled | mod.rs:82 |
| Hello | Hello (handshake.rs) | hello | mod.rs:84 |
| Challenge | Challenge (handshake.rs) | challenge | mod.rs:86 |
| ChallengeResponse | ChallengeResponse (handshake.rs) | challenge_response | mod.rs:88 |
| Welcome | Welcome (handshake.rs) | welcome | mod.rs:90 |
| Reject | Reject (handshake.rs) | reject | mod.rs:92 |
| Ping | Ping (stream.rs) | ping | mod.rs:94 |
| Pong | Pong (stream.rs) | pong | mod.rs:96 |
| Signals | Signals (stream.rs) | signals | mod.rs:98 |
| SignalsAck | SignalsAck (stream.rs) | signals_ack | mod.rs:100 |
| DigestFacts | DigestFacts (stream.rs) | digest_facts | mod.rs:102 |
| RunTool | RunTool (work.rs) | run_tool | mod.rs:104 |
| ToolResult | ToolResult (work.rs) | tool_result | mod.rs:106 |
| GetLogs | GetLogs (work.rs) | get_logs | mod.rs:108 |
| Logs | Logs (work.rs) | logs | mod.rs:110 |
| RunSkill | RunSkill (work.rs) | run_skill | mod.rs:112 |
| SkillResult | SkillResult (work.rs) | skill_result | mod.rs:114 |
| RunCheck | RunCheck (work.rs) | run_check | mod.rs:116 |
| CheckResult | CheckResult (work.rs) | check_result | mod.rs:118 |
| ConfigUpdate | ConfigUpdate (stream.rs) | config_update | mod.rs:120 |
| Error | ErrorMessage (stream.rs) | error | mod.rs:122 |
| Unsupported | Unsupported (stream.rs) | unsupported | mod.rs:124 |

Other public message-related types:
- handshake.rs: `SessionId`, `HostFacts`, `AgentSettings`, `DockerReport`, `CheckOverride`, `CheckInfo`, `RejectReason` (enum)
- work.rs: `ToolOutcome`, `ToolSuccess`, `ToolFailure`, `ToolFailureCode` (enum), `SkillOutcome`, `SkillSuccess`, `SkillFailure`, `SkillFailureCode` (enum), `CheckObservation`
- bytes.rs: `EnrollToken`, fixed-size byte newtypes generated by a macro

---

## 5. Config (crates/domovoi-config)

Two files: `hub.toml` and `agent.toml`.

### HubConfig (`crates/domovoi-config/src/hub/mod.rs:28`), top-level sections
- `hub`: HubSection (line 76)
- `web`: WebSection (line 101)
- `agent_server`: AgentServerSection (line 126)
- `llm`: LlmSection (`hub/llm.rs:15`)
- `triage`: TriageSection (`hub/llm.rs:106`)
- `incidents`: IncidentsSection (line 158)
- `notify`: NotifySection (line 187)
- `discord`: Option<DiscordSection> (line 254)
- `webhooks`: BTreeMap<String, WebhookConfig> (line 275)
- `probes`: Vec<Probe> (line 317)
- `heartbeat`: Option<HeartbeatSection> (line 346)
- `backup`: BackupSection (line 357)
- `secrets`: SecretsSection (line 378)
- `updates`: UpdatesSection (`hub/updates.rs:9`)

### AgentConfig (`crates/domovoi-config/src/agent.rs:18`), top-level sections
- `agent`: AgentSection (line 42)
- `policy`: PolicySection (line 63)
- `docker`: DockerSection (line 83)
- `checks`: BTreeMap<CheckKey, CheckConfig> (line 163 for CheckConfig)
- `containers`: ContainersSection (line 183)
- `local_webhooks`: Option<LocalWebhooks> (line 208)

Other config types: `AgentDirs` (agent.rs:239); `LlmProvider`, `LlmKind`, `CliConfig`, `CliPreset`, `Price`, `TriageLimits` (hub/llm.rs); `WebhookKind`, `WebhookAuth`, `ProbeKind`, `QuietHours`, `Renotify`, `Digest` (hub/mod.rs); `Secret`, `SecretSource` (secret.rs); `Layered`, `Provenance`, `Source` (layered.rs, provenance.rs); `ConfigIssue`, `IssueSeverity` (issue.rs); `Duration`, `LocalTime`, `TimeZone` (duration.rs, time.rs).

---

## 6. Background tasks

Spawn and long-running loop sites in non-test code of `crates/domovoi-hub` and `crates/domovoi-agent`. Test-only spawns (`runtime/tests*`, `runtime/every.rs` test fn, `skill_policy.rs` test fn, `selftest/` harness) and `csp.rs:68` (HTML tag parser loop, not a task) are excluded.

### Hub
- `crates/domovoi-hub/src/runtime/serve.rs:53` (in `run`): spawns the agent-server accept loop (`serving.serve`)
- `crates/domovoi-hub/src/runtime/serve.rs:60` (in `run`): spawns the agent presence loop (`run_presence`)
- `crates/domovoi-hub/src/runtime/serve.rs:67` (in `run`): spawns the axum web/API server with graceful shutdown
- `crates/domovoi-hub/src/runtime/serve.rs:76` (in `run`): one task per chat gateway, running `gateway.run(sink)` until shutdown
- `crates/domovoi-hub/src/runtime/serve.rs:88` (in `run`): spawns the notification/card delivery dispatcher (`dispatcher.run`)
- `crates/domovoi-hub/src/runtime/serve.rs:52` (via `schedulers.spawn`): starts the hub schedulers as one group of tasks
- `crates/domovoi-hub/src/runtime/schedulers.rs:303` (`spawn_minute`): runs a job every minute via `every`
- `crates/domovoi-hub/src/runtime/schedulers.rs:316` (`spawn_advisories`): periodic advisory evaluation via `every`
- `crates/domovoi-hub/src/runtime/schedulers.rs:339` (`spawn_daily`): loop that runs once per day at the next local time (`next_local`)
- `crates/domovoi-hub/src/runtime/schedulers.rs:381` (`spawn_heartbeat`): periodic outbound heartbeat via `every`
- `crates/domovoi-hub/src/runtime/every.rs:23` (`every`, loop): generic periodic runner used by the schedulers, profile and updates tasks, honouring the injected clock and shutdown token
- `crates/domovoi-hub/src/runtime/every.rs:71`: test-only helper; not counted
- `crates/domovoi-hub/src/runtime/updates.rs:233` (`spawn`): checks for new releases once a minute after start, then every 12 hours (`updates.rs` doc)
- `crates/domovoi-hub/src/profile/mod.rs:298` (`spawn`): every minute refreshes at most one host profile that needs it
- `crates/domovoi-hub/src/runtime/dispatcher.rs:128` (`run`, loop): consumes domain events, rebuilding open incidents' cards after a lagged bus
- `crates/domovoi-hub/src/runtime/dispatcher.rs:168` (`recover`, loop): queues a card refresh for every open, confirmed incident
- `crates/domovoi-hub/src/runtime/dispatcher.rs:286` (`notification_items`, loop): builds notification work items
- `crates/domovoi-hub/src/runtime/delivery.rs:368` (`retry`, loop): retries delivery of pending card/notification sends
- `crates/domovoi-hub/src/agent_server/mod.rs:164` (`serve`, loop): accepts agent TCP connections until shutdown
- `crates/domovoi-hub/src/agent_server/mod.rs:172` (`serve`): spawns one task per accepted agent connection (`conn::accept`)
- `crates/domovoi-hub/src/agent_server/mod.rs:266` (`run_presence`, loop): periodic presence/heartbeat check of known agents (`watch_known_hosts`)
- `crates/domovoi-hub/src/agent_server/session.rs:46` (`run_session`, loop): serves one authenticated agent session until it ends
- `crates/domovoi-hub/src/agent_server/ws.rs:47` (`recv`, loop): WebSocket receive loop for an agent link
- `crates/domovoi-hub/src/agent_server/link.rs:68` (`logs`, loop): pages through agent logs with `get_logs` until no more lines
- `crates/domovoi-hub/src/api/events.rs:66` (`events`, loop): SSE stream loop pushing domain events to browsers, with keep-alive
- `crates/domovoi-hub/src/api/chat.rs:133` (`chat`): spawns the triage chat answer computation
- `crates/domovoi-hub/src/api/incidents.rs:441` (`ask`): spawns a follow-up question answer for an incident
- `crates/domovoi-hub/src/api/incidents.rs:478` (`retry_triage`): spawns a triage retry for an incident
- `crates/domovoi-hub/src/selftest/mod.rs:176` and `:418`, loop at `:308` (`wait_for_cards`): self-test harness code (`selftest/` module); not checked whether it runs outside the self-test path

### Agent
- `crates/domovoi-agent/src/runtime.rs:219` (`run`): spawns the relay task (`relay.serve`), if configured
- `crates/domovoi-agent/src/runtime.rs:226` (`run`): spawns the scheduler (`scheduler.run`)
- `crates/domovoi-agent/src/runtime.rs:227` (`run`): spawns the request server (`serve`) for tool and skill requests
- `crates/domovoi-agent/src/link/mod.rs:149` (`run`, loop): hub link with reconnect and backoff
- `crates/domovoi-agent/src/link/mod.rs:217` (`session`, loop): one connected session on the link
- `crates/domovoi-agent/src/link/ws.rs:72` (`recv`, loop): WebSocket receive loop
- `crates/domovoi-agent/src/scheduler.rs:216` (`run`, loop): runs built-in checks on their schedule
- `crates/domovoi-agent/src/scheduler.rs:305` (`run_due`, loop): runs every check that is due and turns results into signals
- `crates/domovoi-agent/src/requests/mod.rs:133` (`chunk_logs`, loop): splits log output into frames
- `crates/domovoi-agent/src/requests/mod.rs:297` (`serve`, loop): serves incoming tool and skill requests after re-validation

---

## 7. Web (web/src)

### Router (`web/src/app/routes.tsx`, mounted by `web/src/app/App.tsx` via `createBrowserRouter([{ path: "*", element: <AppRoutes /> }])`)
Public:
- `/setup` (SetupPage), `/login` (LoginPage)

Behind `RequireSession` and `Layout`:
- `/` (index, overview page)
- `/incidents`, `/incidents/:id`
- `/chat`
- `/rules`
- `/approvals`
- `/hosts`, `/hosts/:name`
- `/skills`
- `/notes`, `/notes/:id`
- `/digest`
- `/logs`
- `/settings`, `/settings/:section`
- `/settings/about` redirects to `/settings/updates`
- `/welcome` (firstRun page)
- `*` NotFoundPage

### Feature folders (web/src/features)
approvals, auth, chat, digest, hosts, incidents, logs, notes, overview, rules, settings, setup, skills, updates

### Components barrel (web/src/components/index.ts, 93 lines)
Exports, grouped as in the barrel:
- Status: BadgePill, StatusPill, Tag, Count, toneStripe
- Messages: InlineError, InlineMessage, Banner, ErrorDetail, ToastProvider, useToast, ToastAction, ToastApi, ToastInput, useActionFeedback, ActionFeedback, FeedbackMessages
- Hints: Tooltip, InfoTip, DocsLink, Kbd
- Buttons: Button, LinkButton, ButtonProps, ButtonVariant, IconButton, CopyButton
- Loading and state: Loading, Skeleton, Spinner, Progress, LoadError, EmptyState, QueryBoundary
- Layout: Cluster, Grid, Stack, Gap, PageHeader, Section, Card, Text, StatTile
- Data: DataTable, Column, TableSort, KeyValueList, KeyValue, FilterChips, Chip, Tabs, Tab
- Time: RelativeTime, Timeline, TIMELINE_PREVIEW, TimelineItem, Countdown, formatRemaining, useRemaining
- Content: CopyBlock, Markdown, Highlight
- Forms: Field, fieldIdForPath, Checkbox, NumberInput, Select, Switch, TextInput, Textarea, SegmentedControl, Segment, FormErrorSummary, SubmitBar, DropZone, ThemeSwitch, PasswordInput, Stepper, StepperStep
- Auth and dialogs: AuthCard, BrandMark, Dialog, DialogProps, ConfirmDialog, ConfirmDialogProps, PasswordPrompt
- Menus: Menu, MenuItem, OptionPicker, PICKER_FILTER_FROM, PickerOption, Popover, CommandPalette, preloadCommandPalette, CommandPaletteProps, PaletteGroup, PaletteItem
- Hook: useDebounced

Barrel source modules (from its `from "..."` lines): StatusPill, Tag, Count, ToneStripe, InlineMessage, Banner, ErrorDetail, Toast, useActionFeedback, Tooltip, InfoTip, DocsLink, Kbd, Button, IconButton, CopyButton, and further kit files (ChatThread, Dialog, Field, Markdown, etc.).

### Test files
- 69 files matching `*.test.ts` or `*.test.tsx` under `web/src` (includes `web/src/components/*.test.tsx`, `web/src/styles/contrast.test.ts`, `web/src/guardrails.test.ts`, `web/src/voice.test.ts`).

---

## 8. Tests

Per crate: count of files under `tests/` (top level of the crate's `tests/` dir, recursive `*.rs`), and `#[test]` / `#[tokio::test]` attribute count across the crate directory (`src/` and `tests/`).

| Crate | tests/ .rs files | #[test]/#[tokio::test] attrs | gates.rs |
|---|---|---|---|
| domovoi | 4 | 36 | none |
| domovoi-agent | 13 | 51 | crates/domovoi-agent/tests/gates.rs |
| domovoi-chat | 5 | 26 | none |
| domovoi-checks | 23 | 79 | none |
| domovoi-config | 8 | 58 | none |
| domovoi-core | 7 | 102 | crates/domovoi-core/tests/gates.rs |
| domovoi-engine | 35 | 202 | crates/domovoi-engine/tests/gates.rs |
| domovoi-hub | 24 | 248 | crates/domovoi-hub/tests/gates.rs |
| domovoi-llm | 9 | 24 | none |
| domovoi-proto | 6 | 22 | none |
| domovoi-runner | 6 | 32 | crates/domovoi-runner/tests/gates.rs |
| domovoi-store | 10 | 15 | none |
| domovoi-testkit | 1 | 16 | none |
| xtask | 0 (no tests/ dir) | 30 (in src/) | none |

Totals: 151 `tests/*.rs` files across crates (xtask has none), about 941 test attributes in total (sum of the column above).

Gate test files: 5 (`agent`, `core`, `engine`, `hub`, `runner`).

Web: 69 test files (see section 7).

---

## 9. CI (.github/workflows)

Six workflow files.

### ci.yml
- changes: "Select relevant checks" (path filter)
- pr-title: "PR title follows Conventional Commits"
- web: "Web (lint, typecheck, test, build)"
- web-build: "Build web UI for embedding"
- docs-links: "Book links, anchors and pages"
- metadata: "Locked workspace metadata"
- rust: "Rust (fmt, clippy, tests, docs, arch-check)"
- deny: "Supply chain (cargo-deny)"
- msrv: "MSRV"
- musl: "Static musl build check"
- actionlint: "Workflow lint"
- shellcheck: "Shell scripts lint"
- installer: "Installer on ${{ matrix.image }}"
- units: "systemd units verify"
- coverage: "Coverage"

### codeql.yml
- analyze: "Analyze ${{ matrix.language }}" (push, pull_request, schedule)

### docker.yml
- container-smoke: "Build and run the images (no push)" (pull_request, workflow_dispatch)

### docs.yml
- build (push, pull_request, workflow_dispatch)
- deploy (same triggers)

### drills-live.yml
- drills-live: "Drills against ${{ inputs.provider }} ${{ inputs.model }}" (workflow_dispatch only)

### release.yml
- release: "Update the release PR and publish" (push, workflow_dispatch)
- web: "Build web UI"
- binaries: "Build ${{ matrix.target }}"
- image: "Publish container image"

### scorecard.yml
- analysis (branch_protection_rule, schedule, push)

---

## Notes on items not verified
- Section 6 spawn descriptions for the agent `requests/mod.rs:133` (`chunk_logs`) and hub `dispatcher.rs:286` (`notification_items`) are taken from function names and nearby doc comments, not from reading each body.
- The `ci.yml` job list was read from `^  name:` lines; some matrix jobs may expand further.
