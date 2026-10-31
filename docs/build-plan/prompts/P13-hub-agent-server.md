# P13 — Hub agent server, links and presence

| | |
| --- | --- |
| Phase | 3, Hub core |
| Depends on | P04, P05, P09, P10 |
| Crates | `domovoi-hub` (agent server); `domovoi` (cross-role integration tests only) |
| Size | L |
| Invariants touched | INV-10 (mutual authentication, revocation) |

## Goal

Build the hub side of the agent protocol: the TLS WebSocket listener with a persisted self-signed certificate, enrollment tokens, challenge-response authentication, per-agent sessions, the `AgentLink` implementation used by the engine, presence tracking, and host-down detection that first checks the hub's own network ("self-doubt check") before blaming a host.

## Read first

- `ARCHITECTURE.md` §7.1, §7.3, §8, INV-10
- ADR 0003
- P05 messages and `challenge_message`; P09 agent link (the counterpart)

## In scope

Add to `domovoi-hub`: `tokio` (full), `tokio-rustls`, `rustls`, `rcgen`, `tokio-tungstenite`, `ed25519-dalek`, `rand`, `sha2`, `base64`, `subtle`, `tracing`, `thiserror`, `serde_json`, `futures`, `async-trait`, `jiff`, `uuid`.

### TLS identity (`agent_server/tls.rs`)

- On first start, generate a self-signed certificate (`rcgen`, ECDSA P-256, SAN = hub name and `domovoi-hub`, 10-year validity) and key into `data_dir/tls/` (key mode 0600). Load on later starts.
- `fingerprint()` = SHA-256 of the DER certificate, displayed as colon-separated hex.

### Enrollment tokens (`agent_server/tokens.rs`)

- `create_token(host_name, ttl = 15 min) -> EnrollmentInvite { token, expires_at, fingerprint, command }` where token = 32 random bytes base64url; only its SHA-256 is stored. `command` is the exact `domovoi agent enroll --hub wss://<public host>:<port> --token <token> --fingerprint <fp>` line.
- Creating a token for a host name that does not exist creates the host (kind `Remote`).

### Connections (`agent_server/conn.rs`)

- Accept TLS, then WebSocket. First frame must be `enroll` or `hello` within 10 s, else close.
- `enroll`: consume the token atomically (P10), store `AgentIdentity` (public key, version), reply `enrolled`, close. Wrong or reused token → `reject(BadToken)` with a constant-time comparison and a 1 s delay.
- `hello`: look up the agent; unknown → `reject(UnknownAgent)`; revoked → `reject(Revoked)`; incompatible version → `reject(Incompatible)`; else send `challenge` (32 random bytes), verify the signature over `challenge_message(...)` with the stored key; failure → `reject(BadSignature)`; success → `welcome`, register the session (replacing an older session for the same agent), record skill summaries and facts.
- Message loop: `signals` → `SignalSink::submit` then `signals_ack`; `digest_facts` → `FactRepo`; `ping` → `pong`; results matched to pending requests by `reply_to` through a `HashMap<RequestId, oneshot::Sender>`; unknown kinds → `unsupported`.
- Limits: frame ≤ 1 MiB, 100 frames/s per connection (excess closes the connection), at most 256 pending requests per agent.

### `AgentLink` (`agent_server/link.rs`)

- `WsAgentLink` implements the P03 `AgentLink`: routes `run_tool`, `run_skill` and `run_check` to the host's current session; `NotConnected` if none; enforces the deadline with `Timeout`; `presence(host)` and `skills(host)` from the session registry.
- `AgentSessionTransport`: the hub accepts any `domovoi_proto::Transport`, so the binary can attach an in-memory agent for single-box mode (P23) through `AgentServer::attach_local(transport)`, which runs the same handshake minus TLS.

### Presence and host-down detection (`agent_server/presence.rs`)

- Track `last_seen` per agent (any frame). An agent silent for 3 × heartbeat interval triggers an evaluation:
  1. **Self-doubt:** is the hub's own network healthy? Use `domovoi_checks::net::gateway_reachable` on the hub machine and a TCP connect to one configured external target (`[probes] self_check = "1.1.1.1:53"` default). If the hub's own network is down, emit an advisory-like signal `hub.network_degraded` for the hub host and **do not** raise host-down.
  2. Otherwise probe the agent's last remote address (TCP to the agent server port is not meaningful; use configured `[[probes]]` for that host if any, else ICMP-free TCP connect to port 22 and 443 of the last remote IP): reachable → signal `agent.down` (warning) for the host; unreachable → signal `host.down` (critical, Immediate).
- These signals go through the normal `SignalSink`, so they become incidents with cards like any other.

### Revocation

- `revoke_agent(host)` marks the agent revoked and closes its live session.

## Out of scope

- HTTP API endpoints for tokens and revocation (P18) and CLI commands (P23); expose plain async functions they will call.
- Webhooks (P14).

## Tests required

Hub-only tests in `crates/domovoi-hub/tests/` with a scripted fake agent written with `tokio-tungstenite`. Cross-role tests in `crates/domovoi/tests/agent_hub.rs` using the real `AgentRuntime` from P09 and the hub agent server, over real TLS on `127.0.0.1:0`, with the fake clock where timing matters.

1. `certificate_is_generated_once_and_reused`.
2. `enrollment_succeeds_once_and_token_cannot_be_reused`.
3. `expired_token_is_rejected`.
4. `unknown_and_revoked_agents_are_rejected`.
5. `bad_signature_is_rejected` (fake agent signs with a different key).
6. `incompatible_major_version_is_rejected`.
7. `real_agent_enrolls_connects_and_sends_signals` (cross-role).
8. `run_tool_round_trips_and_times_out` (cross-role and fake).
9. `run_skill_reaches_agent_and_plan_mismatch_is_reported` (cross-role).
10. `new_session_replaces_old_session_for_same_agent`.
11. `silent_agent_with_reachable_host_raises_agent_down`; `silent_agent_with_unreachable_host_raises_host_down`; `hub_network_down_raises_hub_network_degraded_and_no_host_down` (probe functions injected so tests control reachability).
12. `revocation_closes_live_session`.
13. `oversized_frame_and_flooding_close_connection`.
14. `crates/domovoi-hub/tests/gates.rs`: `agent_link_requires_completed_handshake` (INV-10).

## Architecture gates

- Hub internal deps remain within the table (R1); no process spawning (R3).
- The `domovoi` crate gains only tests in this prompt.

## Acceptance criteria

- `cargo xtask ci` passes.
- `docs/book/src/guide/agents.md` explains enrollment, pinning, revocation and host-down detection.

## Deliverables

- `crates/domovoi-hub/src/agent_server/*`
- `crates/domovoi-hub/tests/*`, `crates/domovoi/tests/agent_hub.rs`, `docs/book/src/guide/agents.md`

## PR

Title: `feat(hub): agent server with enrollment, pinned TLS, presence and host-down detection`
