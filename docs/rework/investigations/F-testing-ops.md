# F. Testing, reliability and operations (read-only investigation)

Scope: TESTING.md, CI/release workflows, xtask, testkit, every `tests/gates.rs`, Docker/deploy/installer, observability, web test setup, and design-pack 04/05. Nothing was run. Counts are grep counts of `#[test]`/`#[tokio::test]` (src + tests), so approximate.
Labels: FACT = read in code/doc, INFERENCE = my reading, RECOMMENDATION = proposal.

## 1. Test inventory by kind

Approximate test-attribute counts per crate (FACT, grep): engine 202, hub 248, core 102, checks 79, config 58, agent 51, domovoi (bin) 36, runner 32, chat 26, llm 24, proto 22, testkit 16, store 15, web ~69 test files.

| Kind | Where | Notes |
| --- | --- | --- |
| Unit (in `src/`) | all crates, e.g. `crates/domovoi-hub/src/runtime/tests.rs`, `xtask/src/arch/scan.rs:185+` | xtask tests its own rules (`process_spawn_outside_runner_is_rejected` etc.) |
| Integration with fakes | `crates/domovoi-engine/tests/{engine,services,notify}/`, `crates/domovoi-hub/tests/hub/`, `crates/domovoi-agent/tests/agent/`, `crates/domovoi-checks/tests/*` | Real engine/hub/agent code against testkit fakes; biggest single file `engine/tests/services/limits.rs` (1382 lines, admission/retry/fallback) |
| Property | proptest in `domovoi-core/tests/fingerprint_properties.rs`, `domovoi-engine/tests/engine/properties.rs`, `domovoi-runner/tests/gates.rs:14` (manifest param strategy) | |
| Protocol / snapshot | `crates/domovoi-proto/tests/protocol/{roundtrip,samples,snapshots,version,auth}.rs` + 20 insta `.snap` files; `domovoi-core/tests/serde_snapshots.rs`; chat/discord cards | `version.rs:8` has a pure `compatible(agent,hub)` matrix and pins `PROTOCOL_VERSION == 1.1`. No test runs an old agent binary/fixture against a new hub |
| DB / contract | `domovoi-testkit/src/contracts/*` (18 suites) run twice: against `InMemoryStore` (`testkit/tests/contracts.rs`) and SQLite (`store/tests/sqlite/contracts.rs`); plus `store/tests/sqlite/{migrations,concurrency,purge,search,paging}.rs` | `migrations.rs` covers idempotent migrate, pre-migrate backup, backup pruning, private dir |
| Gate / invariant | 5 `tests/gates.rs` files + `xtask arch-check` (see below) | |
| Whole-process | `crates/domovoi/tests/{single_box,agent_hub,settings_apply,cli}.rs`; `hub/runtime/tests.rs:62 end_to_end_signal_to_card_to_triage_update`; `selftest --offline` | `single_box.rs:241 sigterm_shuts_down_cleanly_within_15_seconds` is the only real-process lifecycle test; `#![cfg(unix)]` |
| Triage drills | `engine/tests/drills/` (6 YAML cases: `log_injection`, `disk_full_only_destructive_fits`, ...) scored 100 pts (detection 30, discrimination 20, action 30, restraint 20), pass at 80 plus gate probes | Scripted model in CI; `drills_live` is `#[ignore]`, manual workflow `drills-live.yml` |
| Web unit/component | vitest + jsdom + Testing Library, 69 test files (`web/package.json`, `web/vite.config.ts`) | |
| A11y | `web/src/test/axe.ts` runs axe-core after every page test via `setup.ts`; colour contrast excluded in jsdom and proven separately by `styles/contrast.test.ts`; `skipAxe()` opt-out | `web/src/guardrails.test.ts` and `voice.test.ts` enforce DESIGN.md rules |
| E2E / browser | None in CI. `playwright` is a devDependency only for `web/scripts/screens.mjs` (screenshots at 4 widths x 2 themes, manual, "not part of CI" TESTING.md:133) | FACT: no browser-driven test of UI against a real hub |
| Installer | `deploy/test/installer.sh` (205 lines) on ubuntu:24.04 and debian:12 via CI; shellcheck; `systemd-analyze verify` | |
| Container | `.github/workflows/docker.yml` builds both images, runs `version`, `selftest --offline`, `/healthz` (path-filtered to `Dockerfile`, `docker/**`, `deploy/**`) | |

### Gate / invariant tests and what they protect (FACT)

- `crates/domovoi-core/tests/gates.rs` (183 lines): `read_tool_catalogue_is_read_only` (:14, closed ReadTool enum, INV-1/ADR 0007), `read_tool_call_rejects_unknown_tool_names` (:48), `no_approval_without_human` (:67), `destructive_actions_cannot_be_approved_from_chat` (:78), `audit_log_has_no_mutation_api` (:120), `approvals_and_actions_cannot_be_deserialized` (:153, ADR 0014).
- `crates/domovoi-engine/tests/gates.rs` (636): `model_sees_only_read_tools` (:32), `model_calling_approve_or_unknown_tool_changes_nothing` (:56), `shell_command_in_report_is_never_executed` (:86), `secrets_never_reach_the_model` (:112), `prompt_injection_in_logs_does_not_create_actions` (:163), `triage_failure_still_leaves_a_card` (:201).
- `crates/domovoi-hub/tests/gates.rs` (641): `agent_link_requires_completed_handshake` (:33), `webhook_without_auth_never_reaches_signal_sink` (:70), `destructive_approve_requires_exact_confirm_text` (:284), `settings_never_return_secret_values` (:314), `settings_writes_require_admin_session_and_csrf` (:395), `webhook_secrets_never_reach_the_incident_api` (:453), `no_tool_or_chat_command_reaches_settings` (:535), `destructive_action_cannot_be_approved_from_discord` (:575), `discord_message_text_can_never_approve` (:605).
- `crates/domovoi-agent/tests/gates.rs` (107): `hub_cannot_run_skill_not_in_local_catalog` (:33), `plan_hash_mismatch_never_executes` (:71) (INV-3/INV-4, agent-side validation).
- `crates/domovoi-runner/tests/gates.rs` (79): proptest that manifest parameter values cannot become shell/argv injection.
- `xtask arch-check` (`xtask/src/arch/rules.rs`, `scan.rs`): R1/R2 dependency direction and pure crates (`PURE_CRATE_DIRS` core/proto/engine, :178), R3/R4 process spawning only in `domovoi-runner` (:166), INV-1 `HumanApproval::new` only in `hub/src/auth/` (:169) and `restore` only in store (:174), R5 testkit dev-only, R7/R9 anyhow, R8 500-line files, R10 doc comments, R11 banned openssl (:163), R13 append-only migrations and ADR required to change rules (`--base`). Source scans exempt test paths, which is why rejection tests live in `tests/`.
- Also CI-enforced: generated OpenAPI / config docs freshness, docs-link check, cargo-deny, MSRV, musl check, CodeQL (web + workflows), Scorecard, actionlint, shellcheck.

INFERENCE: the gates are strong on authorization/secrecy/injection (the INV-* family). They do not cover liveness, durability, or crash-consistency invariants.

## 2. Testkit fakes (`crates/domovoi-testkit/src`, 1455 lines)

FACT:
- `FakeClock` (`clock.rs`): `advance`/`set` wake pending `sleep_until`; `watch`-based. Tests never sleep.
- `FakeProcessExecutor` (`process.rs`): expectations keyed by (program,args), records calls; unknown -> `NotFound`. No stdout streaming, no delay/timeout simulation.
- `ScriptedLlm` (`llm.rs`): FIFO script of `Result<LlmResponse, LlmError>`, records requests, `hold()`/`wait_for_request()` gate for concurrency tests. Empty script -> `Unavailable("script exhausted")`.
- `FakeAgentLink` (`link.rs:71`): presence, skills, facts, checks, per-kind handlers (`on_tool`/`on_skill`/`on_check`), records `LinkRequest`s. Doc says "Deadlines are recorded but never enforced" (`link.rs:~68`). `RecordingSignalSink` with `fail_next`.
- `InMemoryStore` (`repos/mod.rs`): implements every repo port; `fail_next(StoreError)` injects one failure (`repos/mod.rs:75`). Validated by the same contract suites as SQLite (good fidelity guard).
- `RecordingGateway`/`RecordingNotifier` (`chat.rs`), `FixtureHostFs` (`fs.rs`), `CannedLogSource`, `ListRedactor`, `approvals::{human,web_typed,chat}`, builders (`build.rs`: `T0`, `host`, `signal`, `open_incident`, `action`, `manifest`).

Gaps (INFERENCE from reading the above):
1. No fake that models a real agent: a stateful remote with its own spool, reconnects, drops, duplicate delivery, and late replies. `FakeAgentLink` is request/response with canned handlers and no deadline/timeout semantics, so "outcome unknown" (reply lost) cannot be produced by it. The agent crate has its own transport harness (`agent/tests/agent/harness.rs`, `link.rs`) that is not reusable from hub-side tests.
2. `fail_next` injects exactly one failure on the next call, not "fail the Nth write" or "crash after write N". No crash-point injection, no partial-write simulation for `InMemoryStore`; the SQLite store has no fault injection (disk full, busy beyond timeout, power loss).
3. `FakeProcessExecutor` cannot simulate a hang/timeout, partial output, or exit-signal; runner timeout is tested only against real processes (`runner/tests/runner/exec.rs`).
4. `ScriptedLlm` has no simulated latency / rate limit / streaming truncation (those are tested in `domovoi-llm` with HTTP mocks).
5. No restartable harness: building a "second hub session" over the same store is done ad hoc in `engine/tests/services/recovery.rs:58 restarted(...)` (InMemoryStore only), not a general `Restartable<Hub>` helper.
6. No fake for the update/release source other than the settings row used in `hub/tests/hub/api_updates.rs`.

## 3. CI, release, deploy artifacts, upgrade path

CI (`.github/workflows/ci.yml`, 572 lines) FACT:
- A classifier job (`ci.yml:21-267`) maps changed paths to jobs; fails open (`fail_open`, :57) to run everything. Jobs: pr-title (conventional commits regex :278), web (format, lint, `npm test`, gen:api drift, build :283-311), web-build, docs-links, rust (fmt, clippy `-D warnings`, `nextest run --workspace`, doctests, feature-gated `domovoi-checks`, arch-check `--base origin/<base>` on PRs, openapi/config-docs drift, rustdoc, embed-ui build :359-440), deny, msrv, musl, actionlint, shellcheck, installer matrix, units verify, coverage (llvm-cov, on push to main only, artifact only, no threshold; :~530).
- `.config/nextest.toml`: 30s slow-timeout, terminate after 4 periods (2 min), `fail-fast=false`.
- `cargo xtask ci` (`xtask/src/ci.rs:9-156`) mirrors this locally; each step killed at 30 min (`:171`). Not run by agents by default (AGENTS.md).
- Extra workflows: `codeql.yml` (web + workflows, weekly), `scorecard.yml`, `docs.yml`, `docker.yml` (path-filtered smoke), `drills-live.yml` (manual, scores reported not enforced).
- Dependabot weekly for cargo and npm (`.github/dependabot.yml`).

Not in CI (FACT): no Linux-agent/hub multi-process E2E beyond `domovoi/tests/*` (unix-only), no browser E2E, no performance/load test, no fuzzing, no mutation testing, no protocol compatibility test against a released previous version, no upgrade-from-previous-DB test (only "pending migrations get backed up" on fixtures built in the test), coverage not gated.

Release (`release.yml`, 247 lines, ADR 0008) FACT: release-plz opens a release PR on every push to main; merging it tags `vX.Y.Z` and creates the GitHub release (`:63-83`), uploads `install.sh`; then builds web, static musl binaries for amd64/arm64 with embedded UI (`:121-146`), tarball + sha256 + SLSA provenance attestation with one retry, uploads, then pushes GHCR images `domovoi` (distroless nonroot, `docker/Dockerfile.release`) and `domovoi-agent` (alpine with smartmontools/util-linux, `docker/Dockerfile.agent`), `latest` + semver tags. One shared version for all crates (`release-plz.toml`). Tag-before-build means a failed build leaves a release with no binaries; recovery is documented (`workflow_dispatch` with `tag`). The release workflow itself is not exercised before merge except via `docker.yml` for Dockerfiles.

Deploy artifacts FACT: `Dockerfile` (source build, rust alpine -> distroless), `deploy/compose/{hub,agent}.yml`, `deploy/systemd/{domovoi-hub,domovoi-agent}.service` + `single-box.conf`, `deploy/unraid/domovoi-agent.xml`, `install.sh` (366 lines; checks SHA-256, creates user/dirs/unit, never writes config or starts the service, idempotent upgrade, "keeps edits", adds new starter skills; `install.sh:350-360` prints `systemctl restart` for upgrades). Hub unit stops within 15 s (`deploy/systemd`, matches runtime drain).

Upgrade path FACT/INFERENCE:
- DB: migrations run on every `Store::open` (`domovoi-store/src/db.rs:127-165`, `sqlx::migrate!` at :17). If the DB existed and migrations are pending, a `VACUUM INTO` backup to `backups/pre-migrate-<time>.db` is taken first (`db.rs:130-141`, `backup.rs:21`), pruned to newest N (test `backups_keep_the_newest_copies`). 5 migrations so far (`0001_init` .. `0005_ack_rule_creator`), append-only enforced by arch-check R13. FACT: forward-only; there is no down migration and no automated restore command (INFERENCE: restore is manual copy of the backup file). No check that an older binary refuses a newer-schema DB (INFERENCE: sqlx would fail with a version-mismatch error; no test).
- Hub binary: `domovoi upgrade [--version] [--dry-run]` (`crates/domovoi/src/cmd/upgrade.rs`) downloads, verifies SHA-256, swaps the exe; never restarts a service (prints commands). Or rerun `install.sh`. Hub has update-check (`hub/src/runtime/updates.rs`, `api/updates` with copy-paste commands, test `hub/tests/hub/api_updates.rs`).
- Agent: FACT no auto-update anywhere (`grep auto_update` returns nothing). Agents are upgraded manually or by pulling the image. Skew handling is protocol-level: `PROTOCOL_VERSION` 1.1 (`proto/src/version.rs:17`), `MIN_AGENT_VERSION`/`MIN_HUB_VERSION` both 0.0.5 (:49,:56), `AgentTooOld`/incompatible rejects are final and stop retrying (`agent/tests/agent/link.rs:212,236,253`), advisory `AgentOutdated`.
- Config: on-disk TOML + `DOMOVOI_*` env; `domovoi check config` validates; web-saved settings stored encrypted in DB (`secret.key` in data dir); `StoredSecretUnreadable` advisory covers lost key.

RECOMMENDATION: add (a) an "upgrade from N-1" test that opens a database file produced by the previous release's schema plus data and runs migrations + a smoke; (b) a test that a binary refuses a DB with unknown newer migrations with a clear error; (c) `domovoi restore-backup` or documented runbook; (d) decide whether agent self-update is in scope (otherwise document that protocol minor bumps must remain additive for N-2 agents).

## 4. Observability

FACT:
- Logging: `tracing` + `tracing_subscriber` `EnvFilter` (`--log-level` > `RUST_LOG` > info), human or JSON format, stderr; also an in-memory `LogRing` layer feeding the web logs page, scrubbed by `PatternRedactor` (`crates/domovoi/src/init.rs:58-85`, `log_ring.rs`). Secrets are wrapped types, never logged.
- Metrics: Prometheus via `metrics-exporter-prometheus`, recorder installed in `init.rs:89-95`, `/metrics` served unauthenticated? (`hub/src/api/system.rs:139`; security-headers test at `api_routes.rs:236-243` hits it without auth, so INFERENCE: unauthenticated). Series: `domovoi_signals_total{source}`, `domovoi_incidents{status}`, `domovoi_triage_runs_total{outcome}`, `domovoi_llm_tokens_total`, `domovoi_llm_cost_usd_total`, `domovoi_actions_total{status}`, `domovoi_agents_connected`, `domovoi_event_bus_lagged_total{consumer}`, `domovoi_delivery_failures_total{gateway}`, `domovoi_incident_flaps_total`, `domovoi_session*` (`engine/src/telemetry.rs:5-82`, `agent_server/registry.rs:95`, `runtime/delivery.rs:388`).
- Health: `/healthz` returns the constant string `ok` (`api/system.rs:133`). Used by Docker smoke test and single-box tests. FACT: it checks nothing (no DB ping, no writer-queue depth, no scheduler liveness).
- Tracing spans/distributed tracing: none found (only events and fields). No per-incident correlation id on log lines beyond ad-hoc fields (`action = %run.action_id` at `agent/src/requests/skills.rs:110`).
- Self-monitoring: advisories (`core/src/advisory.rs:10-33`): `HeartbeatMissing`, `TriageDegraded` (3 failures, cleared on success, test `limits.rs:581`), `AgentOutdated`, `DeliveryFailed`, `SettingsApplyFailed`, `StoredSecretUnreadable`, etc. Hub heartbeat ping to an external URL on a scheduler with 10 s timeout (`hub/src/runtime/schedulers.rs:30`; test `runtime/tests.rs:252 heartbeat_failure_raises_advisory_and_success_clears_it`) - i.e. a dead-man's-switch for the hub's own liveness. Metric `domovoi_event_bus_lagged_total` plus test `bus_lag_triggers_reload_not_crash`.
- Metrics tests: `hub/tests/hub/api_routes.rs:380`, `domovoi/tests/single_box.rs:221` assert the endpoint renders; the recorder in `runtime/tests.rs:330-360` is a counting fake. FACT: no test asserts that a specific metric increments for a specific event (INFERENCE from grep).

Gaps: no metrics for writer-queue depth, action latency / time-in-state, approval wait time, store write latency, spool/ack lag (hub-side), agent reconnect count; no readiness vs liveness split; no agent-side `/metrics` (agent is outbound-only so metrics would need to relay as signals/facts).

## 5. Existing reliability safeguards (FACT with INFERENCE on adequacy)

- Crash/restart recovery for actions (ADR 0014, `engine/src/actions/recover.rs`): on startup, `Approved` but undispatched actions are started; `Dispatched` (in flight) actions are failed with "the hub restarted while this ran; check the host before running it again" (`recover.rs:28`, test `engine/tests/services/recovery.rs:193 a_dispatched_action_fails_after_a_restart`); the loop guard is rebuilt from the audit log (`recovery.rs:222`, `:272`). This is a deliberate "outcome unknown -> fail closed and tell a human" policy.
- Single-writer actors: ADR 0011 (incident manager) and ADR 0019 (actions writer, `actions/writer.rs`) serialize state changes; executor runs outside the writer and asks it to complete (`executor.rs:272-296`).
- Re-validation at dispatch (INV-3/INV-4): manifest hash, args, plan hash re-checked hub-side (`executor.rs revalidate`) and again agent-side (`agent/src/requests/skills.rs:97-108`); tests `execute.rs:113 manifest_change_between_approval_and_dispatch_rejects`, `agent/tests/gates.rs:71`.
- Loop guard: `loop_guard_per_hour` per (host, skill), claimed under one lock (`executor.rs reserve_dispatch`), test `execute.rs:183`.
- Verification: `verify()` after a successful skill (`execute.rs:73 failed_verification_reaches_a_human`); recheck in `actions/recheck.rs`.
- Timeouts: dispatch deadline = skill timeout + `dispatch_grace` (`executor.rs:260-266`); runner kills whole process group on timeout (`runner/src/lib.rs:5`, `exec.rs`); agent request deadline test `agent/tests/agent/requests.rs:78 request_deadline_times_out`; offline agent -> `wait_for_agent` then "agent offline" failure (`execute.rs:247`); heartbeat drops link after 3 missed pongs (`agent/tests/agent/link.rs:346`); nextest 2 min kill.
- Reconnect/backoff: agent backoff and reset after stable connection (`link.rs:292,323`); revoked/incompatible rejects are final (`link.rs:191,236`).
- Durable at-least-once signal delivery: agent spool on disk before send, removed on ack, `batch_seq` survives restart, overflow drops info first then oldest batches, resolved signals protected (`agent/src/spool.rs:1-9`; tests `agent/tests/agent/spool.rs:53,101,120,156`). Hub dedupes via incident fingerprint correlation (INFERENCE; no explicit batch_seq idempotency test hub-side was found).
- Admission gate / backpressure: single gate for triage, questions and chat (`engine/src/triage/admission.rs:1`): hourly cap, monthly budget, reserved runs for triage, "busy" for concurrent questions, retry when a slot frees; ~35 tests in `services/limits.rs`. Provider fallback chain and `TriageDegraded` advisory; a failed triage still leaves a raw card (`engine/tests/gates.rs:201`). Triage cache with re-validation (`571421b`), error redaction (`64117f0`).
- Bounded buffers: agent_server per-session outbox `MAX_PENDING=256` (`registry.rs:19`), WS `MAX_FRAMES_PER_SECOND=100` (`ws.rs:16`), body limits (`middleware.rs:16` 1 MiB), broadcast lag handled by resync (`events.rs`, `bus_lag_triggers_reload_not_crash`), scheduler max 4 concurrent checks (`scheduler.rs:186`).
- Retries: delivery retries then records failure without blocking others (`runtime/tests.rs:99`), failed triage retried with delay and capped (`limits.rs:123,652`), SQLite busy timeout retry (`store concurrency.rs:43`).
- Process lifecycle: graceful shutdown drains within 15 s (`runtime/serve.rs:108`, `single_box.rs:241`), `hub.lock` single-instance (`runtime/lock.rs`), private data dir permissions (`migrations.rs:116`), restart-to-apply-settings with rollback advisory (`settings_apply.rs`).

Weaknesses (INFERENCE):
- Idempotency at the agent: `run_skill` carries `action_id` but the agent does not remember executed action ids (`agent/src/requests/skills.rs:110` only logs it; no dedupe store found). A duplicate dispatch (e.g. hub retry after lost reply) would execute twice. Today the hub avoids this by never retrying a `Dispatched` action (fails it), so the gap is latent but becomes real if retry/resume/offline-permit features arrive.
- The agent does not persist a result for a run whose reply could not be delivered, so after "hub restarted while this ran" the hub cannot learn the real outcome (it only advises checking the host). Design-pack 04 wants "failed or uncertain operation is recorded and not blindly repeated" and a verification phase; today an `Unknown` outcome is collapsed into `Failed` with prose.
- No incident-level cycle/budget limits (design: "per-incident cycle and rate limits"); admission is global per hour/month and loop guard is per (host, skill, hour).
- `/healthz` is static; a wedged writer actor would still report healthy.

## 6. Gap analysis against the target design (docs/design-pack/04 and 05)

Target guarantees extracted: explicit phase states persisted (`complete/needs_evidence/waiting/failed/not_applicable/escalate`, 04), durable incident timeline with operation identity, no blind retries and revised plans must differ (04), policy re-evaluated before every operation, per-incident cycle/rate limits (04), explicit-deny-wins deterministic policy with explainable decision records (05), typed policy permits distinct from `HumanApproval` (05), offline emergency permits that are signed, expiry-bound, revocable, "safe under restart, clock uncertainty and duplicate execution", fail closed when stale/ambiguous (05), agent-side validation even after hub authorization (05), unknown/stale context means no autonomous execution (05).

| Needed guarantee | Automated test today | Gap |
| --- | --- | --- |
| Phase machine: persisted state survives restart; every phase returns an explicit status; missing output is not success | Incident status transitions (`core/tests/incident_transitions.rs`, 551 lines) and action recovery (`recovery.rs`) | No orchestration/phase model exists to test; no resume-at-phase test |
| Resume after crash at any point (incl. between approval and dispatch, between agent reply and store write) | Three states covered in-memory (`recovery.rs`) | No crash-at-every-step sweep; SQLite not used; agent side not part of it |
| Outcome-unknown handling (reply lost, agent ran it) | Collapsed to Failed with prose | No `Unknown` outcome type, no agent-held receipt, no reconcile-on-reconnect test |
| Idempotent execution by operation id | None (agent does not dedupe) | Agent-side dedupe + test needed before any retry/resume/offline path |
| No blind retry; revised plan must differ; cycle limits per incident | `failed_retries_stop_at_the_limit...` (triage retries only), loop guard | No per-incident cycle cap test, no "same plan hash twice after failed verification" guard test |
| Verification and regression watch (delayed verification windows) | `execute.rs:36,73` single-shot verify | No delayed/observation-window verification, no regression-after-resolve test |
| Policy evaluator determinism, deny-wins, precedence, decision record | Not existing | Needs table-driven + property tests over (global, host, category, capability, deny) lattices |
| Policy permit cannot be forged or satisfied by model/chat | Closest: INV-1 gates and arch-check | New gate: permit constructor call-site scan + "model output cannot yield a permit" test; keep `HumanApproval` gates unchanged |
| Agent-side enforcement independent of hub | `agent/tests/gates.rs` (plan hash, catalog) | Extend: expired/revoked permit, clock skew, replayed permit, scope widening |
| Offline emergency permit: expiry, revoke-on-reconnect, duplicate execution, audit reconcile | None | Needs fake agent + fake clock with skew, spool/reconcile tests |
| Protocol evolution without breaking old agents | `compatible()` matrix + snapshots | No wire-compat test with stored frames from a previous release |
| Upgrade of persisted data | Migration backup test | No N-1 fixture DB upgrade test; no behavior-preserving test for new tables |
| Self-monitoring of the orchestrator (stuck incident, wedged actor) | Heartbeat ping advisory only | No stuck-phase watchdog, health not tied to progress |
| Load/backpressure at scale (storm of signals, flapping, many agents) | `bus_lag_triggers_reload_not_crash`, `scheduler_runs_at_most_four_checks_at_once` | No signal-storm test through hub with SQLite, no soak |
| End-to-end UI against real hub | None | Playwright exists as devDependency; no spec |

### Proposed harnesses (RECOMMENDATION, in priority order)

1. **Deterministic orchestration simulator** (new module in `domovoi-testkit`, depends only on core/proto per arch rules): single-threaded driver that owns `FakeClock`, `InMemoryStore` (or SQLite temp), `ScriptedLlm`, a `FakeAgent`, and a seeded scheduler that chooses the order of ready events (signal arrival, LLM reply, agent reply, timer). Run N seeds; assert invariants after every step: (a) no action executes without an approval/permit record; (b) every `Dispatched` has an audit entry; (c) no two executions share an operation id; (d) incident never `Resolved` without a verification success; (e) cycle count <= cap. Failing seed printed for replay. Fits the existing "tests never sleep" rule and the pure engine design.
2. **Crash-injection wrapper**: `CrashingStore<S: Repos>` that panics/returns `Crash` after the k-th write (and variant: after write, before returning), plus a `restart()` helper that drops the engine, keeps the store, and rebuilds (generalizing `recovery.rs:58 restarted`). Sweep k over 1..=N for a canonical scenario (signal -> triage -> propose -> approve -> dispatch -> verify -> resolve) and run the invariants from (1) on the recovered state. Run against both InMemoryStore and SQLite (the contract-suite pattern already proves they behave alike).
3. **`FakeAgent` (stateful)**: reusable remote that implements the proto handshake semantics, has a real spool (reuse `Spool` or a model of it), an executed-operation ledger with idempotent replay, and fault knobs: drop request, drop reply after executing, duplicate delivery, delay beyond deadline, reconnect with older protocol version, clock skew. This is the piece that makes outcome-unknown testable. `FakeAgentLink` should enforce deadlines using `FakeClock`.
4. **Protocol compatibility matrix**: commit golden frames per released minor version (`proto/tests/protocol/fixtures/v1.0/*.json`, `v1.1/...`); tests decode every historical frame with current code (forward compat) and assert current frames encode to a shape older decoders (kept as a frozen copy of the message enums in test code) accept when version-compatible. Add `cargo xtask proto-freeze <version>` to snapshot on release. Extend the existing `compatible()` table to include the `MIN_*_VERSION` constants and actual rejects.
5. **Upgrade harness**: store `crates/domovoi-store/tests/fixtures/db-<release>.sqlite` (or a script that builds it from the old migrations at that tag), test open -> migrate -> contract + invariants; plus "unknown future migration -> clear error". Add CI job that downloads the previous release binary and runs `hub run` over a seeded data dir then the new one (needs Linux; fits `domovoi/tests` cfg(unix)).
6. **Policy evaluator tests**: table-driven decision matrix plus proptest "adding a deny never turns a deny into an allow", "result independent of rule order", "unknown context -> not auto-allowed"; snapshot the `DecisionRecord` text. Add an INV-style gate: permit type has no `Deserialize`, constructor scanned like `HumanApproval::new`.
7. **Observability assertions**: a test recorder (exists in `runtime/tests.rs:330`, promote to testkit) so tests can assert `domovoi_*` counters; make `/healthz` check writer liveness and add `/readyz`; test that a wedged writer flips it.
8. **E2E smoke**: one Playwright spec in CI using `cargo run -p domovoi-hub --example dev_hub` (already provides seeded incident + actions): setup admin, approve an action, assert audit entry. Keep it tiny.
9. **Soak/load (non-gating nightly)**: signal storm of 10k signals across 50 fake agents through the real hub on SQLite; assert bounded queue depth, no lag-driven loss of resolved signals, and p99 write latency.

Also RECOMMENDATION: make coverage a soft gate for `domovoi-engine` and `domovoi-core` (llvm-cov already runs on main), and add `cargo-mutants` on the pure engine/policy crates as a scheduled job, since decisions there are the safety surface.

## 7. Open questions

1. Should an in-flight action whose reply was lost become a distinct `OutcomeUnknown` state (with agent-held receipt and reconcile-on-reconnect), or stay folded into `Failed` per ADR 0014? The design pack (04) implies the former; it needs an ADR and a migration.
2. Agent idempotency: is `action_id` the operation identity the target design wants, or should a per-attempt operation id be introduced so a legitimately revised plan for the same action gets a new id?
3. Is agent self-update or at least a staged "upgrade agents from hub" in scope? Today a protocol minor bump relies on operators updating manually; what N-x agent skew must the hub support (currently `MIN_AGENT_VERSION` 0.0.5 is pinned by hand)?
4. Does `/metrics` need auth? Evidence suggests it is unauthenticated (security-headers test calls it with no session); confirm intended exposure vs the 7420 bind default.
5. Is backing up only before migrations enough, or is a scheduled online backup (and a documented restore) expected for the knowledge/experience store in design 06/07?
6. Where should crash-consistency be asserted for SQLite: is WAL + `synchronous` setting documented/tested (not examined here), and does the writer actor commit audit + state transition in one transaction? (Not verified: `actions/writer.rs` and `store` transaction boundaries should be read before designing the crash sweep.)
7. Should live-model drills gate releases (currently manual and "scores reported, not enforced"), especially as orchestration becomes multi-phase and less scripted?
8. Windows development note: AGENTS.md says WSL has no Rust/Node, and `single_box.rs` is `cfg(unix)`; so local process-level tests cannot run on the maintainer's machine. Is a Linux dev container or CI-only policy acceptable for the new crash/upgrade harnesses?
9. Is a `.claude/worktrees/*` copy of the repo (found containing duplicate `tests/gates.rs`) intentionally ignored by CI and arch-check scans? (git-excluded locally, but the arch scanner walks the tree; verify `collect_rs` skips `.claude/`.)
