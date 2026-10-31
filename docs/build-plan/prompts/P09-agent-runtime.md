# P09 — Agent runtime

| | |
| --- | --- |
| Phase | 2, Agent side |
| Depends on | P04, P05, P08 |
| Crates | `domovoi-agent` |
| Size | L |
| Invariants touched | INV-3 (re-validate every request), INV-4 (plan hash), INV-10 (mutual auth, pinning) |

## Goal

Build the agent: enrollment, the pinned-TLS WebSocket link with challenge-response authentication and reconnects, the check scheduler that turns observations into signals, a disk spool for offline periods, and the request handler that serves read tools and skills only after re-validating them locally.

## Read first

- `ARCHITECTURE.md` §7.2, §8, INV-3, INV-4, INV-10
- ADR 0003
- P05 messages, P06 catalogue and executor, P07–P08 checks, P04 `AgentConfig`

## In scope

Add `tokio` (full set needed: `rt-multi-thread`, `net`, `time`, `sync`, `macros`, `fs`, `signal`), `tokio-rustls`, `rustls`, `tokio-tungstenite`, `ed25519-dalek`, `rand`, `sha2`, `base64`, `serde`, `serde_json`, `tracing`, `thiserror`, `futures`, `jiff`, `uuid`. Dev: `domovoi-testkit`, `rcgen`, `tempfile`.

### Identity and enrollment (`identity.rs`, `enroll.rs`)

- `Identity { agent_id, host_id, signing_key (ed25519), hub_fingerprint: [u8; 32] }` stored in `state_dir/identity.json` (mode 0600, written atomically via temp file + rename). The signing key is never logged.
- `enroll(hub_url, token, expected_fingerprint: Option<[u8;32]>, config, state_dir) -> Result<Identity>`: connect with a TLS verifier that records the server certificate's SHA-256; if `expected_fingerprint` is given and differs, abort before sending anything. Send `enroll`, receive `enrolled`, persist identity with the observed fingerprint.

### Link (`link/`)

- TLS client using `tokio-rustls` with a custom `ServerCertVerifier` that accepts exactly the pinned SHA-256 fingerprint of the end-entity certificate and ignores names and CAs. Signature verification still uses the rustls crypto provider.
- WebSocket via `tokio_tungstenite::client_async` over the TLS stream. Implement `domovoi_proto::Transport` for it.
- Handshake: send `hello` (with skill summaries from the catalogue and host facts) → receive `challenge` → sign `challenge_message(nonce, agent_id, session_hint)` → receive `welcome` or `reject`. On `reject(Revoked | UnknownAgent)` stop retrying and surface a clear error; on `Incompatible` stop with an upgrade message.
- Reconnect with exponential backoff (1 s doubling to 60 s, ±20 % jitter), reset after 60 s of stable connection.
- Heartbeat: `ping` every `heartbeat_seconds` (from `welcome`), connection considered dead after 3 missed `pong`s.
- `LinkHandle` exposing an outbound queue (`send(Message)`) and an inbound request stream. The runtime is generic over `Transport` so the binary can supply an in-memory transport for single-box mode (P23).

### Scheduler (`scheduler.rs`)

- Runs each enabled check at its cadence (config override or default), at most 4 concurrently, using the `Clock` port.
- Converts `Observation`s into `Signal`s: keeps the last firing state per `(check, target)` in `CheckState`; emits `Firing` signals each run while firing (the hub folds them) and one `Resolved` signal when a target stops firing or disappears.
- Publishes `digest_facts` once per hour (latest per key).
- Persists `CheckState` to `state_dir/checks.json` atomically after each run.

### Spool (`spool.rs`)

- Outgoing signal batches get a monotonically increasing `batch_seq`. Every batch is appended to `state_dir/spool.jsonl` before sending; `signals_ack { up_to_seq }` truncates acknowledged batches. On reconnect, unacknowledged batches are replayed in order.
- Caps: 10 000 batches or 8 MiB; when full, drop the oldest **info** signals first and record a `spool.dropped` count fact.

### Request handler (`requests.rs`)

- `run_tool`: map each `ReadToolCall` to its implementation: Docker tools via the checks crate's `ContainerSource` (ps, inspect, logs with line cap, events from the 24 h ring buffer); `HostOverview` from `HostFs` (uptime, load, memory, disks); `Journal` and `SmartReport` via the checks crate's command templates executed by the runner's `SandboxExecutor`; `ListeningPorts` from `/proc/net`; `RunReadSkill` via the catalogue **only if the skill's risk is `Read`**. Enforce the deadline; cap output at 64 KiB; redact.
- `run_skill`: in order: (1) skill exists in the local catalogue else `NotFound`; (2) risk allowed by `[policy] allow_risk` else `RiskNotAllowed`; (3) validate args with the local manifest else `InvalidArgs`; (4) recompute `PlanHash` from local manifest hash, host id and validated args and compare with the request else `PlanMismatch`; (5) execute through the runner; (6) return `skill_result` with `argv_hash`. Log each step without arguments' values.
- `run_check`: run the named check once and return its observations for the target.
- `IncomingRequest` handling never blocks the link task: requests run on a bounded task set (max 4 concurrent).

### Runtime (`runtime.rs`)

- `AgentRuntime::new(config, deps: AgentDeps { clock, executor, hostfs, container_source }) -> Result<Self>` and `run(self, shutdown: CancellationToken) -> Result<()>`.

## Out of scope

- Local webhook relay listener (P14). CLI subcommands (P23). Hub side (P13).

## Tests required

In `crates/domovoi-agent/tests/`, with a minimal test hub built in the test from `tokio-tungstenite` + `tokio-rustls` + an `rcgen` self-signed certificate, and the fake clock:

1. `enrollment_pins_fingerprint_and_persists_identity_with_0600`.
2. `enrollment_aborts_on_unexpected_fingerprint`.
3. `link_refuses_hub_with_different_certificate`.
4. `handshake_signs_challenge_with_enrolled_key` (test hub verifies with the public key).
5. `revoked_reject_stops_retrying`.
6. `reconnects_with_backoff_after_drop` (advance fake clock; count connection attempts).
7. `signals_are_spooled_and_replayed_in_order_after_reconnect`; `ack_truncates_spool`; `full_spool_drops_info_first`.
8. `scheduler_emits_resolved_when_target_recovers` and `firing_signals_repeat_each_run`.
9. `run_skill_rejects_unknown_skill_disallowed_risk_invalid_args_and_plan_mismatch` (four cases, each asserting the runner was never called via `FakeProcessExecutor`).
10. `run_read_skill_tool_refuses_write_skills`.
11. `tool_output_is_capped_and_redacted`.
12. `crates/domovoi-agent/tests/gates.rs`: `hub_cannot_run_skill_not_in_local_catalog` and `plan_hash_mismatch_never_executes` (INV-3, INV-4).

## Architecture gates

- Internal deps exactly: core, config, proto, checks, runner (R1).
- No `std::thread::sleep`; all waiting through `Clock`.

## Acceptance criteria

- `cargo xtask ci` passes.
- Identity file permissions verified in a test.
- The runtime shuts down cleanly on cancellation within 2 s (test).

## Deliverables

- `crates/domovoi-agent/src/{identity,enroll,link/*,scheduler,spool,requests,runtime,error}.rs`
- `crates/domovoi-agent/tests/*`

## PR

Title: `feat(agent): enrollment, pinned TLS link, scheduler, spool and request handling`
