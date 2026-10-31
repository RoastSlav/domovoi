# P05 — Wire protocol

| | |
| --- | --- |
| Phase | 1, Core contracts |
| Depends on | P02 |
| Crates | `domovoi-proto` |
| Size | M |
| Invariants touched | INV-3, INV-10 (message shapes that make them possible); R14 |

## Goal

Define every message exchanged between hub and agent, the envelope, versioning, size limits, the challenge signing payload and a transport trait, as pure data with exhaustive snapshot tests. This is a public contract.

## Read first

- `ARCHITECTURE.md` §8, R14
- ADR 0003
- P02 types (`ReadToolCall`, `SkillSummary`, `PlanHash`)

## In scope

Add `serde`, `serde_json`, `thiserror`, `jiff`, `uuid`, `base64`, `async-trait`.

1. **`version.rs`**: `ProtocolVersion { major: u16, minor: u16 }`, `PROTOCOL_VERSION = 1.0`, `fn compatible(agent, hub) -> bool` (same major, `agent.minor + 1 >= hub.minor` and `agent.minor <= hub.minor`).
2. **`envelope.rs`**: `Envelope { v: u16, id: RequestId, kind: String, reply_to: Option<RequestId>, payload: serde_json::Value }`; typed `Message` enum; `encode(&Message, id, reply_to) -> String`; `decode(&str) -> Result<Decoded, ProtoError>` where `Decoded { id, reply_to, message: Message }` and an unknown `kind` decodes to `Message::Unknown { kind }` (not an error). `MAX_FRAME_BYTES = 1 MiB`; larger input → `ProtoError::FrameTooLarge`. Unknown payload fields are ignored.
3. **`messages.rs`**: one struct per kind:

| Kind | Direction | Payload |
| --- | --- | --- |
| `enroll` | agent → hub | `token`, `public_key` (base64, 32 bytes), `host_name`, `facts: HostFacts`, `agent_version` |
| `enrolled` | hub → agent | `agent_id`, `host_id` |
| `hello` | agent → hub | `agent_id`, `protocol`, `agent_version`, `facts`, `capabilities: Vec<String>`, `skills: Vec<SkillSummary>` |
| `challenge` | hub → agent | `nonce` (base64, 32 bytes) |
| `challenge_response` | agent → hub | `signature` (base64, 64 bytes) |
| `welcome` | hub → agent | `session_id`, `hub_version`, `config: AgentSettings` |
| `reject` | hub → agent | `reason: RejectReason { UnknownAgent, Revoked, Incompatible { hub }, BadSignature, BadToken }` |
| `ping` / `pong` | both | `at` |
| `signals` | agent → hub | `batch_seq: u64`, `signals: Vec<Signal>` |
| `signals_ack` | hub → agent | `up_to_seq: u64` |
| `digest_facts` | agent → hub | `facts: Vec<DigestFact>` |
| `run_tool` | hub → agent | `call: ReadToolCall`, `deadline` |
| `tool_result` | agent → hub | `outcome: Ok(ToolOutput) / Err(ToolFailure { code, message })` |
| `run_skill` | hub → agent | `action_id`, `skill`, `args`, `plan_hash`, `deadline` |
| `skill_result` | agent → hub | `outcome: Ok(SkillOutput) / Err(SkillFailure { code: NotFound / RiskNotAllowed / PlanMismatch / InvalidArgs / Timeout / Exec, message })` |
| `run_check` | hub → agent | `check`, `target`, `deadline` |
| `check_result` | agent → hub | `observations` (status per target) |
| `config_update` | hub → agent | `config: AgentSettings` |
| `error` / `unsupported` | both | `code`, `message` / `kind` |

   `HostFacts { hostname, os, kernel, arch, cpu_count, mem_total_bytes, boot_time, boot_id }`; `AgentSettings { heartbeat_seconds, check_overrides: BTreeMap<CheckKey, CheckOverride> }`.
4. **`auth.rs`**: `fn challenge_message(nonce: &[u8; 32], agent_id: AgentId, session_hint: &str) -> Vec<u8>` returning `b"domovoi-agent-auth-v1\0" || nonce || agent_id bytes || session_hint bytes`. Signing and verification happen in the runtimes; this is only the byte layout.
5. **`transport.rs`**: `#[async_trait] trait Transport: Send { async fn send(&mut self, frame: String) -> Result<(), TransportError>; async fn recv(&mut self) -> Result<Option<String>, TransportError>; }`. WebSocket and in-memory implementations live in the runtimes and the binary.

Document every message (purpose, who sends it, when) in rustdoc; this doc becomes the protocol reference.

## Out of scope

- TLS, WebSocket, enrollment logic, signature crypto (P09, P13).

## Tests required

1. `every_message_kind_round_trips`: encode then decode equals the original, for a sample of every kind.
2. Insta snapshots of the JSON of every message kind with fixed IDs and times (`tests/snapshots/`). These are the wire contract.
3. `unknown_kind_decodes_as_unknown`; `unknown_fields_are_ignored`.
4. `oversized_frame_is_rejected`.
5. `compatibility_matrix`: same version, agent one minor behind, agent ahead, different major.
6. `challenge_message_layout_is_stable`: fixed bytes for fixed inputs.
7. `deadline_and_timestamps_serialize_as_rfc3339`.

## Architecture gates

- Proto is pure: no `tokio`, no I/O (R2).
- Snapshot changes in later PRs require a protocol version bump if breaking (R14).

## Acceptance criteria

- `cargo xtask ci` passes.
- `docs/book/src/reference/protocol.md` generated or written from the rustdoc: message table with directions and examples.

## Deliverables

- `crates/domovoi-proto/src/{version,envelope,messages,auth,transport}.rs`
- Snapshot tests; `docs/book/src/reference/protocol.md`

## PR

Title: `feat(proto): hub-agent wire protocol v1`
