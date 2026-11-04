# 16 · Feature: hub and agent logs in the web UI

Two prompts. 16a shows the hub's own logs; 16b fetches each agent's logs over the
link. Run in order.

## Background (verified)

- Logging is set up once per process in `crates/domovoi/src/init.rs`, `logging()`:
  `tracing_subscriber::fmt()` to stderr, human or JSON. Nothing keeps logs in memory.
- Secrets are wrapped types that never print, so log lines carry no secrets; still run
  the redactor on anything sent out.
- The binary is the composition root: it builds `HubDeps`
  (`crates/domovoi/src/cmd/hub_run.rs`, `cmd/selftest.rs`) and `AgentDeps`
  (`crates/domovoi-agent/src/production.rs`). Construction sites of `HubDeps { .. }` and
  `AgentDeps { .. }` also exist in tests: search for them and update all.
- Single-box mode runs hub and agent in one process, so they share one log buffer.

---

## 16a · Keep recent logs in memory and show the hub's

### Steps

1. **Core port.** New file `crates/domovoi-core/src/ports/logs.rs` (export it from
   `ports/mod.rs`):
   ```rust
   /// One captured log line.
   pub struct LogLine { pub at: Timestamp, pub level: String, pub target: String, pub message: String }
   /// Recent log lines of this process.
   pub trait LogSource: Send + Sync {
       /// The newest `max` lines at `min_level` or more severe ("error", "warn", "info", "debug"), oldest first.
       fn tail(&self, max: usize, min_level: &str) -> Vec<LogLine>;
   }
   ```
   Derive `Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize` on `LogLine`.
2. **Ring buffer.** New file `crates/domovoi/src/log_ring.rs`: `pub struct LogRing`
   (a `Mutex<VecDeque<LogLine>>`, capacity 2000, each message cut to 2 KiB) that
   implements `LogSource` and `tracing_subscriber::Layer<S>`; `on_event` formats the
   event's fields (use a small `tracing::field::Visit` that collects `message` and
   `key=value` pairs) and the time from `jiff::Timestamp::now()`.
3. **Install it.** In `init.rs` `logging()`, build the subscriber as
   `tracing_subscriber::registry().with(filter).with(fmt_layer).with(ring.clone())` and
   return the `Arc<LogRing>` (change the signature; update the caller in `main.rs`).
   Keep human/JSON behaviour unchanged.
4. **Hub.** Add `pub logs: Option<Arc<dyn LogSource>>` to `HubDeps`
   (`crates/domovoi-hub/src/runtime/mod.rs`) and to `ApiDeps`; pass the ring from
   `hub_run.rs`; `None` everywhere else (tests, selftest, dev hub).
5. **API.** `GET /api/v1/system/logs?lines=200&level=info` (lines 1–2000, level one of
   error/warn/info/debug, default 200/info) → `Vec<LogLineDto { at, level, target, message }>`,
   redacted with the hub's redactor if one is reachable from `ApiDeps`, else as is. 503
   "logs are not captured" when `logs` is `None`. Regenerate OpenAPI.
6. **Web.** New page `web/src/features/logs/LogsPage.tsx` at `/logs` (nav "Logs"): a
   source select ("Hub" for now), level select, line count select (100/200/1000), a
   Refresh button and an "Auto-refresh every 5 s" checkbox (`refetchInterval`), and a
   monospace, horizontally scrollable `<pre>` with one line per entry:
   `HH:MM:SS LEVEL target: message`, levels coloured (error red, warn amber).

### Tests

- `LogRing`: keeps the newest 2000, filters by level, cuts long messages.
- API: with a fake `LogSource` returns lines; without one, 503.
- Web: renders lines and refetches when Refresh is pressed.

---

## 16b · Fetch an agent's logs over the link

### Steps

1. **Proto** (`messages/work.rs`): `GetLogs { lines: u16, min_level: String, deadline: Timestamp }`
   = `"get_logs"` (hub → agent) and `Logs { lines: Vec<domovoi_core::ports::LogLine> }`
   = `"logs"` (agent → hub, the reply). Add both to the `messages!` list, samples,
   snapshots, protocol minor (rule 10).
2. **Agent.** Add `pub logs: Option<Arc<dyn LogSource>>` to `AgentDeps` (fill it in
   `production.rs` from the ring the binary passes in; `None` in tests). Add
   `RequestKind::Logs(GetLogs)` in `crates/domovoi-agent/src/link/mod.rs` next to the
   `RunTool` mapping (around line 290). In `requests/mod.rs` answer it with
   `Message::Logs(Logs { lines })` from `deps.logs` (empty when `None`), each message
   passed through `ctx.redactor.redact`, at most 2000 lines. Add the busy reply case in
   `busy_reply`.
3. **Hub.** Add an inherent `pub async fn logs(&self, host, lines, min_level) -> Result<Vec<LogLine>, LinkError>`
   on `WsAgentLink` (`crates/domovoi-hub/src/agent_server/link.rs`) using `request()`
   with a 10 s deadline; `Message::Logs` → `Ok(lines)`, `Message::Unsupported(_)` →
   `Err(LinkError::Rejected("the agent is too old to send logs; update it"))`. Add
   `async fn logs(..)` to the `AgentAdmin` trait in `api/mod.rs` and implement it for
   `AgentServer` (fakes in tests: return an empty list).
4. **API.** `GET /api/v1/hosts/{name}/logs?lines=&level=` → `Vec<LogLineDto>`; 409 with
   the link error text when the agent is not connected or too old. Regenerate OpenAPI.
5. **Web.** The Logs page source select lists "Hub" plus every host; picking a host uses
   the new endpoint. On the host detail page add a "Logs" tab showing the same viewer
   (extract `LogViewer.tsx` from the page and reuse it).

### Tests

- Agent: a `get_logs` request returns redacted lines from a fake `LogSource`.
- Hub: the link maps `unsupported` to the "too old" error (use the in-memory transport
  test helpers in `crates/domovoi-hub/tests/hub/agent.rs`).
- Web: switching the source to a host calls `/api/v1/hosts/{name}/logs`.

### Done when

- The Logs page shows the hub's and each connected agent's recent logs, filtered by
  level, with auto-refresh; old agents show "update the agent".
- All tests and snapshots pass.
