# P03 — Ports and testkit

| | |
| --- | --- |
| Phase | 1, Core contracts |
| Depends on | P02 |
| Crates | `domovoi-core`, `domovoi-testkit` |
| Size | L |
| Invariants touched | foundations for INV-2 (`ProcessExecutor`), INV-5 (`Redactor`) |

## Goal

Define every port (trait) the engine and runtimes depend on, and build `domovoi-testkit`: in-memory fakes for each port, a fake clock, a scripted LLM, a recording chat gateway, fixture helpers, and **contract test suites** that later prove each real adapter behaves like its fake.

## Read first

- `ARCHITECTURE.md` §6, §7, §17
- ADR 0002, ADR 0011
- `crates/domovoi-core` as built by P01 and P02

## In scope

### Ports in `crates/domovoi-core/src/ports/`

All traits use `#[async_trait]`, are object-safe and `Send + Sync`. Add `async-trait` to core. Each port module defines its own error enum.

1. **`clock.rs`**: `Clock { fn now(&self) -> Timestamp; async fn sleep_until(&self, t: Timestamp); }`.
2. **`repos.rs`** (split into `repos/*.rs` if large) with `StoreError { NotFound, Conflict(String), Backend(String) }`:
   - `HostRepo`: `upsert_host`, `get_host`, `get_host_by_name`, `list_hosts`, `put_agent(AgentIdentity)`, `get_agent`, `revoke_agent`, `create_enrollment_token { host, token_hash, expires_at }`, `consume_enrollment_token(token_hash, now) -> Result<HostId>` (single use).
   - `SignalRepo`: `insert`, `list_for_incident(id, limit)`, `purge_older_than(ts)`.
   - `IncidentRepo`: `insert`, `update`, `get`, `find_active_by_fingerprint(fp)` (Pending/Open/Acked/Muted), `find_recent_resolved_by_fingerprint(fp, since)`, `list(filter: IncidentFilter, page: Page) -> Paged<Incident>`, `append_event(IncidentTimelineEntry)`, `timeline(id)`, `search(text, limit)`.
   - `ActionRepo`: `insert`, `update`, `get`, `list(filter, page)`, `find_by_chat_token_hash`.
   - `AuditLog`: `append(AuditEvent)`, `list(filter, page)`. **No update or delete methods.** `AuditEvent { at, actor: String, kind, subject, detail: serde_json::Value }`.
   - `NoteRepo`: `upsert`, `get`, `delete`, `list(host)`, `search(query, host, limit) -> Vec<NoteHit { note, snippet, score }>`.
   - `BaselineRepo`, `AckRepo` (rules and maintenance windows), `AdvisoryRepo`, `FactRepo` (digest facts by host and day), `TriageCacheRepo { get(fingerprint, evidence_digest, since), put }`, `UsageRepo { record(provider, tokens_in, tokens_out, cost_micros, at), month_total(month) }`, `SettingsRepo { get(key), set(key, value) }`, `UserRepo` and `SessionRepo` (used from P18).
   - Shared `Page { cursor: Option<String>, limit: u16 }` and `Paged<T> { items, next_cursor }`.
3. **`llm.rs`**: `LlmProvider { fn id(&self) -> &str; fn supports_tools(&self) -> bool; async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError>; }` with `LlmRequest { system: String, messages: Vec<LlmMessage>, tools: Vec<ToolSpec { name, description, input_schema: Value }>, max_output_tokens: u32 }`, `LlmMessage { role: User | Assistant, parts: Vec<Part> }`, `Part { Text(String), ToolCall { id, name, input: Value }, ToolResult { id, content: String, is_error: bool } }`, `LlmResponse { parts, stop: StopReason { EndTurn, ToolUse, MaxTokens, Other(String) }, usage: Usage { input_tokens, output_tokens, cost_micros: Option<u64> } }`, `LlmError { Transport(String), RateLimited { retry_after: Option<SignedDuration> }, Auth, InvalidResponse(String), Unavailable(String) }`.
4. **`chat.rs`**: `ChatGateway { fn platform(&self) -> &str; async fn post_card(&self, card: &Card) -> Result<MessageRef>; async fn update_card(&self, msg: &MessageRef, card: &Card) -> Result<()>; async fn open_thread(&self, msg: &MessageRef, title: &str) -> Result<ThreadRef>; async fn reply(&self, thread: &ThreadRef, text: &str) -> Result<()>; async fn run(&self, sink: Arc<dyn InteractionSink>) -> Result<()>; }`, `InteractionSink { async fn deliver(&self, i: Interaction) -> InteractionReply; }`, `Notifier { fn id(&self) -> &str; async fn send(&self, msg: &OutboundMessage) -> Result<()>; }` with `OutboundMessage { title, markdown, severity: Option }`. `MessageRef`/`ThreadRef` are opaque `{ platform, id: String }`.
5. **`agent_link.rs`**: `AgentLink { async fn run_tool(&self, host: HostId, call: ReadToolCall, deadline: Timestamp) -> Result<ToolOutput, LinkError>; async fn run_skill(&self, host: HostId, req: SkillDispatch, deadline) -> Result<SkillOutput, LinkError>; async fn run_check(&self, host, check: CheckKey, target: Option<TargetKey>, deadline) -> Result<CheckRecheck, LinkError>; fn presence(&self, host: HostId) -> Presence; fn skills(&self, host: HostId) -> Vec<SkillSummary>; }` plus `SignalSink { async fn submit(&self, signals: Vec<Signal>) -> Result<(), IngestError>; }`. Define `ToolOutput { text, truncated }`, `SkillDispatch { action, skill, args, plan_hash }`, `SkillOutput { exit_code: Option<i32>, stdout, stderr, truncated, duration_ms, argv_hash }`, `Presence { Connected { since, version }, Disconnected { last_seen: Option } }`, `LinkError { NotConnected, Timeout, Rejected(String), Transport(String) }`.
6. **`process.rs`**: `ProcessExecutor { async fn run(&self, spec: ProcessSpec) -> Result<ProcessOutput, ProcessError>; }` with `ProcessSpec { program: String (absolute path), args: Vec<String>, env: BTreeMap<String, String>, cwd: Option<String>, stdin: Option<String>, timeout: SignedDuration, max_output_bytes: u32, run_as: Option<String> }`, `ProcessOutput { exit_code: Option<i32>, signal: Option<i32>, stdout, stderr, truncated: bool, timed_out: bool, duration_ms }`, `ProcessError { NotFound, NotExecutable, ShellRefused, UserSwitch(String), Spawn(String) }`.
7. **`hostfs.rs`**: `HostFs { async fn read_to_string(&self, path: &str) -> Result<String, FsError>; async fn read_dir(&self, path: &str) -> Result<Vec<String>, FsError>; async fn exists(&self, path: &str) -> bool; }` (paths are host-absolute, e.g. `/proc/meminfo`).
8. **`redact.rs`**: `Redactor { fn redact(&self, text: &str) -> String; }`.

### `domovoi-testkit`

Add `tokio` (`sync`, `time`, `macros`, `rt`), `async-trait`, `serde_json`, `jiff`, `uuid`.

- `clock::FakeClock`: starts at a fixed instant; `advance(duration)` wakes every `sleep_until` whose target is reached (use `tokio::sync::Notify` or a watch channel). Never touches real time.
- `repos::InMemory*` for every repo port (one `InMemoryStore` struct implementing all of them is fine).
- `llm::ScriptedLlm`: queue of `Result<LlmResponse, LlmError>`; records every `LlmRequest`; returns `LlmError::Unavailable("script exhausted")` when empty.
- `chat::RecordingGateway` and `RecordingNotifier`: record every call; `inject(Interaction)` delivers to the sink passed to `run` and returns the reply.
- `link::FakeAgentLink`: per-host presence and skills; handlers registered as closures for tools, skills and rechecks; records requests.
- `process::FakeProcessExecutor`: expectations keyed by program + args, returning canned `ProcessOutput`; unexpected calls return `ProcessError::NotFound` and are recorded.
- `fs::FixtureHostFs`: maps host paths to files under a fixture root directory (`/proc/meminfo` → `<root>/proc/meminfo`).
- `redact::ListRedactor`: replaces a configured list of secret strings with `[REDACTED]`.
- `approvals::human(user)` and `approvals::web_typed(user)`: helpers building `HumanApproval` for tests (testkit is allowed to call `HumanApproval::new`).
- `build::{signal, incident, manifest, action}`: concise builders with sensible defaults.
- `contracts::*`: async functions such as `incident_repo_contract(repo: &dyn IncidentRepo)`, `audit_log_contract`, `host_repo_contract`, `note_repo_contract`, `action_repo_contract` that exercise the full documented behaviour of a port and panic on violation. P10 runs them against SQLite.

## Out of scope

- Real adapters (store, llm, chat, runner, checks): later prompts.
- Engine logic.

## Tests required

1. `fake_clock_wakes_sleepers_only_when_advanced`.
2. `scripted_llm_returns_in_order_and_records_requests`; `scripted_llm_errors_when_exhausted`.
3. `recording_gateway_round_trips_injected_interaction`.
4. Every `contracts::*` suite passes against the in-memory implementation (`crates/domovoi-testkit/tests/contracts.rs`).
5. `enrollment_token_is_single_use` and `expired_token_is_rejected` (through the contract).
6. `audit_log_has_no_mutation_api`: a compile-time check documented in the test that `AuditLog` exposes only `append` and `list`.

## Architecture gates

- Core stays free of `tokio` (ports use `async-trait` only).
- `domovoi-testkit` is used only as a dev-dependency anywhere (R5).

## Acceptance criteria

- `cargo xtask ci` passes.
- Every port has at least one fake in testkit and, where it is a repository, a contract suite.
- Port docs describe semantics precisely enough to implement an adapter without reading the fake (ordering, idempotency, errors).

## Deliverables

- `crates/domovoi-core/src/ports/*`
- `crates/domovoi-testkit/src/*`, `crates/domovoi-testkit/tests/contracts.rs`

## PR

Title: `feat(core): ports and testkit fakes with contract suites`
