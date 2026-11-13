# 17 — LLM reliability

| | |
|---|---|
| Branch | `llm-reliability` |
| Milestone | F4 |
| Depends on (must be merged) | 08 (unit of work, migrations numbering, `synchronous=FULL`) |
| Can run in parallel with | 18, 19, 57 |
| Produces (shared contracts) | `AdmissionRepo` port and `llm_admission` table; `UsageRepo::record_call` with model, outcome and latency; `LlmError::Timeout`; `domovoi_llm::with_deadline`; the `prompt_bytes` setting |
| Consumes | Migration numbering and `schema_compat` convention from prompt 08 (verify names in code) |
| Launch gate | none to start. **R-5 stays open**: this prompt measures prompt sizes and ships a configurable cap with a stated default; the owner accepts or changes the default after reading the measurement. Do not write "R-5 resolved" anywhere. |
| Suggested model | medium |

## Objective

Make the model path safe to rely on, without changing what triage, follow-up questions and chat do for the operator:

1. **Persisted admission.** The hourly run cap survives a hub restart (and every settings save, which restarts the runtime). A failure to read the budget or the admission state **fails closed**: no model call is made.
2. **Cache hits use no slot.** A repeat incident answered from the triage cache does not count against the hourly cap and does not need the budget check.
3. **Per-call hard deadline** enforced in `crates/domovoi-llm`, on top of the existing idle timeout.
4. **Total prompt cap** per model call, configurable, with an **R-5 measurement step**.
5. **Attribution on every call**, success and failure: provider, model, outcome, latency, tokens, cost. Failed calls record a usage row.
6. **Redaction** of replayed history and of provider error text, at one choke point.
7. **Host-scoped `similar()`**: similar-incident titles in the triage context come only from the same host.

## Architectural context

Plain-terms background (decisions RW-06 and `04` §16):

- **Admission** is the single gate every model-backed task passes before a call starts: triage runs, incident follow-up questions and general chat. It enforces the rolling-hour run cap (with `reserved_for_incidents` runs kept free for triage) and the monthly budget. Today the hour window is an in-memory deque, so a restart resets the cap, the budget check fails open on a read error, and a cache hit still consumes a slot because admission runs before the cache lookup. Target: counters persisted in a `llm_admission` table, budget or admission read error fails closed, cache lookup before admission.
- **Fail closed, by caller.** For chat and questions a read error is a refusal (HTTP 503 with a plain message). For triage the incident alert has already been delivered (diagnosis failure never suppresses the alert), so the run ends as `TriageOutcome::Skipped` with a reason, the card stays as it is, and a retry is deferred a few minutes. The operation-level `Paused{Error}` status from `04` §16 does not exist yet (operations arrive in a later milestone): do not invent it. Use `Skipped`.
- **Model-call retry is not execution retry** (RW-06). Do not add retries in this prompt. The existing provider-chain fallback stays as it is. Rate-limit `retry_after` handling and a shared provider loop are later work.
- **Hard deadline.** The HTTP adapters only have an idle read timeout that restarts on every streamed byte, and `TriageBudget.max_seconds` is checked between turns, so one call can run forever if the server keeps trickling tokens. A wrapper in `domovoi-llm` races the call against a deadline using the `Clock` port (tests never sleep).
- **Prompt cap.** Context parts are capped (4 KiB each, tool results 8 KiB), but nothing caps the sum: about 170 KiB is reachable (15 tool results x 8 KiB, plus context, plus history). A total byte cap per call, applied centrally before the call, trims old tool results first.
- **Attribution.** The `llm_usage` row stores only `provider.id()`. It gains `model`, `outcome` and `latency_ms`, and failed calls get a row (zero tokens, zero cost, because the cost of a failed call is unknown).
- **Redaction.** Provider error text is a leak channel (a response body excerpt can echo prompt content). Replayed follow-up turns are redacted when stored, but patterns can change later, so they are redacted again on replay (chat already does this).
- **Host scoping.** Similar-incident titles from other hosts enter the prompt today (`context.rs`), a cross-host injection channel (finding D23). Scope the search to the incident's host in the repository query, so the 20-hit limit is not spent on other hosts.
- This prompt touches **no approval, execution or policy path**. The LLM tool catalogue (`ReadTool`) stays closed and unchanged.

## Repository context

Verify each item by opening it. Line numbers are from the state of the repository when this brief was written; they may have moved.

**Admission and triage (engine, no I/O)**

- `crates/domovoi-engine/src/triage/admission.rs:70-102` `admit(shared, Purpose)`: checks `budget_spent` (`:50-62`, **returns `false` on a read error**), then trims `limits(shared).starts` (an in-memory `VecDeque<Timestamp>`) and either pushes `now` or returns `Refusal::Hourly { retry_at }`. `free_slots` (`:105`) and `next_slot` (`:114`) read the same deque synchronously. `Refusal` has `BudgetSpent` and `Hourly{retry_at}`.
- `crates/domovoi-engine/src/triage/run.rs:23-37` `Limits { starts, failures, degraded }` and `limits()`. `outcome()` at `:54` calls `admit(shared, Purpose::Triage)` **first** (`:59`) and only afterwards reads the latest signal and the cache (`:80-94`). That ordering is why a cache hit consumes a slot.
- `crates/domovoi-engine/src/triage/run.rs:168-193` `providers()` builds `why` from provider errors and logs it with `tracing::info!(reason = why.as_str())` (`:184`) **before** the final redaction at `:113`.
- `crates/domovoi-engine/src/triage/run.rs:269-287` the repair turn: it formats the parse `error` into a user message and calls `agent::complete`.
- `crates/domovoi-engine/src/triage/agent.rs:95-123` `complete()`: calls `provider.complete(req).await?` (the `?` returns before any usage is recorded), then `telemetry::record_llm` and `usage.record(provider.id(), in, out, cost, now)`. `converse()` at `:130-212` loops turns; `messages` grows with every tool result. This is the choke point for the prompt cap, attribution and error redaction. All of these call `complete`: `converse`, the repair turn in `run.rs`, and the tool-less path in `chat.rs:~214`.
- `crates/domovoi-engine/src/triage/mod.rs:66-102` `TriageSettings` (Copy; `Default`), `:116-151` `TriageDeps` (ports as `Arc<dyn ...>`), `:153-176` `TriageError { UnknownIncident, Provider, BudgetSpent, Store, Busy{retry_at} }`, `:187-198` `Shared`, `:247-260` `TriageService::spawn` (sync fn).
- `crates/domovoi-engine/src/triage/queue.rs:106-133` `retry_deferred` uses `free_slots` and `next_slot`; `:162-` `retry` is unchanged by this prompt.
- `crates/domovoi-engine/src/triage/followup.rs:81-108` `remembered(turns)` replays stored question and answer turns **without redaction**; `:135-` `answer()` admits at `:154`. `crates/domovoi-engine/src/triage/chat.rs:60-82` `remembered(redactor, history)` already redacts; `:151` admits.
- `crates/domovoi-engine/src/triage/context.rs:168-194` `similar()`:

```rust
let Ok(found) = shared.deps.incidents.search(query, 20).await else { return String::new(); };
found.iter().filter(|i| {
        i.id() != incident.id()
            && i.status() == IncidentStatus::Resolved
            && i.check() == incident.check()
    })
```

  There is **no host filter**. Callers of `incidents.search`: only `context.rs:172` in production code.
- `crates/domovoi-core/src/ports/repos/incidents.rs:118` `async fn search(&self, text: &str, limit: u16)`. SQLite impl `crates/domovoi-store/src/repos/incidents.rs:~340-354` (FTS over the title; the `incidents` table has a `host_id` column and an index `incidents_host_opened`). In-memory impl `crates/domovoi-testkit/src/repos/incidents.rs:205`. Contract `crates/domovoi-testkit/src/contracts/incidents.rs:327-345`.

**LLM port and adapters**

- `crates/domovoi-core/src/ports/llm.rs:122-143` `LlmError { Transport, RateLimited{retry_after}, Auth, InvalidResponse, Unavailable }`. No match on `LlmError` is exhaustive outside `domovoi-llm` (verify with `grep -rn "LlmError::"`).
- `crates/domovoi-llm/src/http.rs:97-141`: `send_error` and `status_error` build error text from the response body via `excerpt` (`error.rs:37`), which only removes the API key.
- `crates/domovoi-llm/src/factory.rs:15-48` `build(settings, executor)`; `crates/domovoi-llm/src/settings.rs:11` `DEFAULT_TIMEOUT` (120 s idle). Re-exports in `crates/domovoi-llm/src/lib.rs`. Tests in `crates/domovoi-llm/tests/llm/` (binary `llm`, `main.rs`).
- Hub wiring: `crates/domovoi-hub/src/runtime/settings.rs:97-138` `chain_for` and `fallback_models_for`; callers `runtime/mod.rs:431,435,452,457`, `runtime/start.rs:63,65` (`preflight`), test `runtime/tests.rs:744-760` (builds an `LlmProvider` config literal). `settings::triage(config)` at `:36`. `TriageDeps` is built in `runtime/mod.rs:~462`.
- Config: `crates/domovoi-config/src/hub/llm.rs` `LlmProvider` (`:27-52`, `timeout: Option<Duration>`), `TriageLimits` (`:160-170`: `tool_calls`, `seconds`, `output_tokens`), validation `crates/domovoi-config/src/validate.rs:~25-70`.

**Store and usage**

- `crates/domovoi-core/src/ports/repos/triage.rs:48-65` `UsageRepo { record(provider, tokens_in, tokens_out, cost_micros, at), month_total(month) }`. SQLite impl `crates/domovoi-store/src/repos/triage.rs:~75-110`. Table `llm_usage(id, provider, tokens_in, tokens_out, cost_micros, at)` in `crates/domovoi-store/migrations/0001_init.sql:227-235`. Existing migrations are `0001`..`0005`; prompt 08 adds more: **use the next free number** (see Database).
- In-memory: `crates/domovoi-testkit/src/repos/mod.rs:28` `type UsageRow = (String, u64, u64, u64, Timestamp)` and `usage: Vec<UsageRow>`; `repos/triage.rs:41-70`. `InMemoryStore::fail_next(StoreError)` (`repos/mod.rs:75`) makes the next repo call fail.
- Contract suite: `crates/domovoi-testkit/src/contracts/triage.rs:67` `usage_repo_contract`.
- Telemetry: `crates/domovoi-engine/src/telemetry.rs:13-15,68-82` `record_llm`.

**Hub error mapping (must stay exhaustive)**

- `TriageError` is matched in `crates/domovoi-hub/src/api/chat.rs:137-141`, `api/followups.rs:64-68`, `api/incidents.rs:445-449`, each mapping to `ApiError` (`Unavailable(String)` exists).

**Existing tests to keep green**

`crates/domovoi-engine/tests/services/limits.rs` (cap, budget, retry, cache: `per_hour_cap_and_monthly_budget_skip_without_model_call`, `repeat_incident_uses_cache_without_model_call`, `questions_count_against_the_hourly_limit`, ...), `chat.rs`, `turns.rs`; `crates/domovoi-engine/tests/gates.rs` (`triage_failure_still_leaves_a_card`, redaction module); harness `crates/domovoi-engine/tests/services/harness.rs`; drills `tests/drills/world.rs`; hub `crates/domovoi-hub/tests/hub/api_harness.rs`.

## Exact scope

Build:

1. `AdmissionRepo` port (core), SQLite impl (store), in-memory impl (testkit), contract suite, migration.
2. Rewrite `admission.rs` on top of it; fail closed; cache lookup before admission in `run.rs`; `free_slots`/`next_slot` become async and fallible; remove `Limits.starts`.
3. `LlmError::Timeout`; `domovoi_llm::with_deadline`; hub wiring and the `call_deadline` setting.
4. Prompt cap: `TriageSettings.max_prompt_bytes`, config `triage.per_run.prompt_bytes`, central enforcement in `agent::complete`, metric, R-5 measurement step.
5. Attribution: `UsageEntry`, `UsageRepo::record_call`, migration columns, `complete()` records failures, metrics.
6. Redaction: choke point in `complete()`, replay redaction in follow-up, redaction of `why` in `providers()`, repair message.
7. Host-scoped `similar()`: `IncidentRepo::search` gains a `host` argument.
8. `TriageError::AdmissionUnavailable` and its three hub mappings.

Files expected to change or be added:

- core: `src/ports/repos/triage.rs` (or a new `src/ports/repos/admission.rs` re-exported from `repos/mod.rs`, whichever keeps files under 500 lines), `src/ports/repos/incidents.rs`, `src/ports/repos/mod.rs`, `src/ports/llm.rs`.
- store: `migrations/NNNN_llm_admission.sql` **(new)**, `src/repos/triage.rs`, `src/repos/incidents.rs`, possibly `src/repos/admission.rs` **(new)** and `repos/mod.rs`; `tests/sqlite/contracts.rs` (register the new contract), `tests/sqlite/migrations.rs` (upgrade test).
- testkit: `src/repos/mod.rs`, `src/repos/triage.rs`, `src/repos/incidents.rs`, `src/contracts/triage.rs`, `src/contracts/incidents.rs`.
- llm: `Cargo.toml` (`futures`), `src/deadline.rs` **(new)**, `src/lib.rs`, `tests/llm/deadline.rs` **(new)** and `tests/llm/main.rs` (module line).
- engine: `src/triage/{admission,run,agent,mod,context,followup,chat,queue}.rs`, `src/telemetry.rs`; tests under `tests/services/` (see Testing).
- config: `src/hub/llm.rs`, `src/validate.rs`; regenerate `docs/book/src/reference/configuration.md`.
- hub: `src/runtime/settings.rs`, `src/runtime/mod.rs`, `src/runtime/start.rs`, `src/runtime/tests.rs`, `src/api/{chat,followups,incidents}.rs`, `tests/hub/api_harness.rs`.

## Explicit non-goals

- No model-call retry loop, no `retry_after` handling, no merging of the three provider loops, no change to the fallback order (RW-06 retry rules arrive with the operation model).
- No `Paused{Error}` operation status and no new advisory for admission failure (deferred; after prompt 08 it would be a Rust-only `AdvisoryKind` addition). Log a warning and count a metric instead.
- No new field on `TriageReport` (it has `deny_unknown_fields`; a model-writable attribution field would be wrong). Attribution lives on usage rows, metrics and logs.
- No change to the `PROPOSALS:` text protocol, `ReadTool`, tool schemas, approvals, the action flow, or the Operator conversation model.
- No change to `Store::purge` (retention is prompt 57). `llm_admission` is self-pruning inside `admit`.
- No `/metrics` auth (prompt 19).
- Do not touch the hub's "test connection" path (`settings/connection.rs`); it builds its own provider.
- Do not wrap override providers used by tests (`AdapterOverrides`); only providers built from configuration get the deadline.

## Required interfaces and contracts

### Core: `AdmissionRepo` (**fixed** semantics, **convention** naming style)

```rust
/// Who is asking for a model slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionPurpose { Triage, Question }   // stored as 'triage' | 'question'

/// What [`AdmissionRepo::admit`] decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdmissionOutcome {
    /// A start was recorded at `now`.
    Admitted,
    /// The window already holds `room` starts; `starts` are the recorded starts inside
    /// the window, oldest first, so the caller can say when a slot frees.
    Full { starts: Vec<Timestamp> },
}

#[async_trait]
pub trait AdmissionRepo: Send + Sync {
    /// Atomically: delete starts older than twice `window`; read the starts newer than
    /// `now - window`; when fewer than `room` are there, insert one for `purpose` at `now`.
    /// `room == 0` always returns `Full`. Check and insert are one transaction.
    async fn admit(&self, purpose: AdmissionPurpose, room: u32, window: SignedDuration,
                   now: Timestamp) -> Result<AdmissionOutcome, StoreError>;
    /// Starts newer than `since`, oldest first. Read only.
    async fn starts_since(&self, since: Timestamp) -> Result<Vec<Timestamp>, StoreError>;
}
```

`TriageDeps` gains `pub admission: Arc<dyn AdmissionRepo>` with a doc comment. `Store` and `InMemoryStore` implement it.

### Core: usage attribution (**fixed**)

```rust
/// How a model call ended, as stored in `llm_usage.outcome`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallOutcome { Ok, RateLimited, Auth, Timeout, Transport, InvalidResponse, Unavailable }
impl CallOutcome { pub fn as_str(self) -> &'static str }   // "ok","rate_limited","auth","timeout","transport","invalid_response","unavailable"
impl From<&LlmError> for CallOutcome { ... }

/// One model call to record.
pub struct UsageEntry<'a> {
    pub provider: &'a str,
    pub model: Option<&'a str>,
    pub outcome: CallOutcome,
    pub tokens_in: u64, pub tokens_out: u64, pub cost_micros: u64,
    pub latency_ms: u64,
    pub at: Timestamp,
}
```

`UsageRepo` gains a **required** method `async fn record_call(&self, entry: &UsageEntry<'_>) -> Result<(), StoreError>`. The existing `record(provider, in, out, cost, at)` becomes a **provided** method that calls `record_call` with `model: None, outcome: Ok, latency_ms: 0` (so existing tests that call `UsageRepo::record` keep compiling). `month_total` is unchanged and still sums every row (a failed call adds zero).

### Core: `LlmError::Timeout` (**fixed**)

```rust
/// The call did not finish within the hard per-call deadline.
#[error("no answer within {after}")]
Timeout { after: SignedDuration },
```

Display of `SignedDuration` is ISO-8601-like (`PT5M`); that is acceptable.

### Core: `IncidentRepo::search` (**fixed**)

`async fn search(&self, text: &str, host: Option<HostId>, limit: u16)`. With `Some(host)` only that host's incidents match. Update the doc comment, the SQLite query (add `AND i.host_id = ?` only when a host is given: build two constant SQL strings, no string formatting of user data), the in-memory impl and the contract.

### `domovoi-llm`: deadline wrapper (**fixed** behaviour)

```rust
/// Wraps `inner` so one `complete` call fails with [`LlmError::Timeout`] when it has not
/// finished `after` the call started, measured on `clock`.
pub fn with_deadline(inner: Arc<dyn LlmProvider>, after: SignedDuration,
                     clock: Arc<dyn Clock>) -> Arc<dyn LlmProvider>;
/// The hard per-call deadline used when none is configured.
pub const DEFAULT_CALL_DEADLINE: SignedDuration = SignedDuration::from_secs(300);
```

The wrapper forwards `id()`, `model()` and `supports_tools()`. In `complete`: compute `deadline_at = clock.now() + after` (on overflow use `now`), then race `inner.complete(req)` against `clock.sleep_until(deadline_at)`:

```rust
let call = inner.complete(req);
let timer = clock.sleep_until(deadline_at);
futures::pin_mut!(call, timer);
match futures::future::select(call, timer).await {
    Either::Left((result, _)) => result,
    Either::Right(_) => Err(LlmError::Timeout { after }),   // the call future is dropped = cancelled
}
```

Dropping the call future cancels the HTTP request (reqwest) and kills the CLI child (the executor kills on drop or on its own timeout; verify in `crates/domovoi-runner` and do not change it). Add `futures = { workspace = true }` to `crates/domovoi-llm/Cargo.toml` (workspace dependency already exists).

### Config (**fixed** key names, defaults)

- `[llm.primary]` and `[llm.secondary]`: `call_deadline: Option<Duration>` ("The longest one model call may take in total, such as `5m`, even while it keeps streaming. Default `5m`."). Must be at least 10 seconds (validation error otherwise, path `llm.<slot>.call_deadline`).
- `[triage.per_run]`: `prompt_bytes: u32` ("Largest prompt sent in one model call, in bytes. Old tool results are shortened first to fit. Default 131072 (128 KiB)."). Must be at least 32768 (validation error `triage.per_run.prompt_bytes`).

`TriageSettings` gains `max_prompt_bytes: usize` (default `131_072`). `settings::triage()` maps it.

### Engine: admission (**fixed** decision table, **convention** code shape)

```rust
pub(crate) enum Refusal {
    BudgetSpent,
    Hourly { retry_at: Option<Timestamp> },
    /// The budget or the persisted window could not be read: nothing may start.
    Unverifiable,
}
pub(crate) async fn admit(shared: &Shared, purpose: Purpose) -> Result<(), Refusal>;
pub(crate) async fn free_slots(shared: &Shared, now: Timestamp) -> Result<usize, StoreError>;
pub(crate) async fn next_slot(shared: &Shared) -> Result<Option<Timestamp>, StoreError>;
/// How long a triage that could not be admitted because limits were unreadable waits.
pub(crate) const ADMISSION_RETRY: SignedDuration = SignedDuration::from_mins(5);
```

`admit` order: (1) if `monthly_budget_micros` is set, read `usage.month_total`; read error -> `Unverifiable`; `cost >= budget` -> `BudgetSpent`. (2) compute `room` exactly as today (`Triage`: `per_hour_runs`; `Question`: `per_hour_runs - reserved_for_incidents`, saturating). (3) `admission.admit(purpose, room, 1 hour, now)`; store error -> `Unverifiable`; `Full{starts}` -> `Hourly{retry_at}` with the same retry arithmetic as today (`room == 0` -> `None`; otherwise the start at index `starts.len() - room` via `slot_after`); `Admitted` -> `Ok(())`. Log `tracing::warn!` with the redacted error text on `Unverifiable` and increment `domovoi_llm_admission_rejections_total{reason}` for every refusal (`reason` = `budget`, `hourly`, `unverifiable`).

### Hub/API mapping (**fixed**)

New `TriageError::AdmissionUnavailable` (`#[error("the model limits could not be checked; try again shortly")]`). In the three hub matches map it to `ApiError::Unavailable("the model limits could not be checked; try again shortly".to_owned())`.

## Ordered implementation steps

1. **Inspect and verify the repository context; report discrepancies.** Open every file in "Repository context". Confirm: prompt 08 is merged (`UnitOfWork`, new migration numbers exist); `admission.rs` is still the in-memory deque; `agent::complete` still returns before recording usage on error; `similar()` has no host filter. Run `ls crates/domovoi-store/migrations` and note the highest number. If 08 is not merged, stop and report. Write the discrepancies down before editing.
2. **Core ports.** Add `AdmissionPurpose`, `AdmissionOutcome`, `AdmissionRepo`, `CallOutcome`, `UsageEntry`, `UsageRepo::record_call` (+ provided `record`), `LlmError::Timeout`, and the new `IncidentRepo::search` signature. Doc comments on every item. Re-export from `ports/repos/mod.rs` and `ports/mod.rs` following the existing pattern.
3. **Migration** (see Database). Then the SQLite impls: `Store: AdmissionRepo`, `Store: UsageRepo::record_call` (INSERT with the new columns), `IncidentRepo::search` with the optional host.
4. **Testkit.** In-memory `AdmissionRepo` (a `Vec<(AdmissionPurpose, Timestamp)>` in `State`, same semantics, goes through `begin()` so `fail_next` applies), `UsageRepo::record_call` (replace the `UsageRow` tuple with a small struct holding model, outcome, latency; keep `month_total` working), in-memory `search` with host. Contract suites: `admission_repo_contract` (new) and extend `usage_repo_contract` and the incident `search` contract. Register the new contract in `crates/domovoi-store/tests/sqlite/contracts.rs` and wherever the testkit's own tests run the in-memory suites (copy how `usage_repo_contract` is registered).
5. **`domovoi-llm`.** Add `deadline.rs`, export `with_deadline` and `DEFAULT_CALL_DEADLINE`. Write `tests/llm/deadline.rs`.
6. **Config.** Add `call_deadline` and `prompt_bytes`, validation, fix the struct literal in `crates/domovoi-hub/src/runtime/tests.rs:746` and any other `LlmProvider {` literal (`grep -rn "LlmProvider {" crates`). Run `cargo xtask docs-config` and commit the regenerated page.
7. **Hub wiring.** `chain_for` and `fallback_models_for` gain a `clock: &Arc<dyn Clock>` parameter and wrap each built provider with `domovoi_llm::with_deadline(provider, config.call_deadline.map_or(DEFAULT_CALL_DEADLINE, |d| d.as_signed()), clock.clone())`. Update the five call sites and the unit test (pass a `SystemClock` or the test clock already used in `runtime/tests.rs`). `TriageDeps { admission: store.clone(), .. }` in `runtime/mod.rs`; `api_harness.rs` likewise. Map `max_prompt_bytes` in `settings::triage()`.
8. **Engine, admission.** Rewrite `admission.rs`; make `queue.rs` use the async fallible `free_slots`/`next_slot` (on `Err`: treat as zero free slots, defer every due retry again at `now + ADMISSION_RETRY`, `tracing::warn!` once per loop). Remove `starts` from `Limits`. In `run.rs::outcome` reorder: read the latest signal, compute the digest, try the cache (`!fresh`), and **only then** call `admit(Triage)`; a cache hit returns before admission. On `Refusal::Unverifiable` return `Skipped { reason: "the model limits could not be checked, so triage did not run; it tries again shortly" }` and `shared.defer(incident.id(), now + ADMISSION_RETRY, RetryReason::Cap)`. Follow-up and chat: map `Unverifiable` to `TriageError::AdmissionUnavailable`. Add the enum variant and the three hub mappings.
9. **Engine, attribution, cap, redaction** in `agent::complete` (details below).
10. **Engine, redaction of replay and `why`**, **host-scoped `similar()`**.
11. **R-5 measurement** (details below).
12. **Tests**, then docs, then checks and the completion report.

### `agent::complete` after this prompt (**fixed** order of operations)

```text
complete(shared, provider, req):
  1. req = fit_to_cap(req, shared.settings.max_prompt_bytes)      // may trim; records metric if it did
  2. bytes = prompt_bytes(&req); histogram domovoi_llm_prompt_bytes{provider}
  3. started = clock.now()
  4. result = provider.complete(req).await
  5. latency_ms = clock.now() - started (clamped >= 0)
  6. match result:
       Ok(resp)  -> record_call(outcome=Ok, resp.usage, latency); telemetry::record_llm; Ok(resp)
       Err(err)  -> err = redact_error(redactor, err);
                    record_call(outcome=CallOutcome::from(&err), tokens 0, cost 0, latency);
                    counter domovoi_llm_calls_total{provider,model,outcome};  Err(err)
  7. a failed record_call is only logged (tracing::warn), never turns a success into a failure
```

Metrics: add `domovoi_llm_calls_total` (counter with labels `provider`, `model` (use `unknown` when none), `outcome`), `domovoi_llm_prompt_bytes` (histogram), `domovoi_llm_prompt_trimmed_total` (counter), `domovoi_llm_admission_rejections_total` (counter) as constants in `telemetry.rs`, next to the existing ones. Label values are fixed vocabularies or a configured model name: never put error text or incident data in a label.

`redact_error`: for `Transport(s)`, `InvalidResponse(s)`, `Unavailable(s)` replace `s` with `redactor.redact(&s)`; other variants unchanged. A unit test must prove a registered secret in each of the three variants is gone.

`fit_to_cap` algorithm (**fixed**; deterministic, no model involved):

```text
size(req) = len(system) + sum over messages, parts: Text.len | ToolCall: name.len + input.to_string().len | ToolResult: content.len
if size <= cap: return req unchanged
step A: for each ToolResult part in messages BEFORE the last message, oldest first:
          replace content with "[earlier tool output omitted to fit the prompt size limit]"
          (keep id and is_error, so provider tool pairing stays valid); stop as soon as size <= cap
step B: if still over: in the LAST message, for ToolResult parts largest first, cut content with
          tools::cut(content, remaining_budget_for_that_part) where the part may shrink to no less than 1 KiB
step C: if still over: return Err(LlmError::Unavailable("the prompt is larger than the configured limit of N bytes")).
```

Never touch the first user message (it carries the incident context) or the system prompt. The minimum cap of 32 KiB in config validation guarantees the opening (7 parts x 4 KiB, plus the system prompt) fits, so step C is a guard, not a normal path. `fit_to_cap` returns the new request and a `bool trimmed`; when `trimmed` increment `domovoi_llm_prompt_trimmed_total` and `tracing::info!(before, after, cap, "prompt trimmed to the size limit")`.

### Redaction of replay and failure text (**fixed**)

- `followup.rs::remembered(turns)` takes `redactor: &dyn Redactor` and redacts each question and answer before building messages (as `chat.rs::remembered` does). Update both the call site (`followup.rs:~191`) and the unit test `remembered_turns_come_back_as_question_and_answer_messages`.
- `run.rs::providers()`: redact `why` right after it is built (before `tracing::info!` and before `reasons.push`).
- `run.rs::attempt` repair message: pass `error` through `shared.deps.redactor.redact(&error)`.

### Host-scoped `similar()` (**fixed**)

`similar()` calls `search(query, Some(incident.host()), 20)` and keeps the existing `id`, `Resolved`, same-`check` filters. Add a defensive `i.host() == incident.host()` to the filter (defence in depth, in case an implementation ignores the argument). Verify `Incident::host()` exists (it is used at `context.rs:~112`).

### R-5 measurement step (required; changes no default)

Goal: record real prompt sizes so the owner can accept or change the 128 KiB default. R-5 stays open until they do.

1. `domovoi_llm_prompt_bytes{provider}` and the `tracing::debug!(bytes, "llm prompt size")` line from `complete()` are the measurement instruments.
2. `cargo run -p domovoi-hub --example dev_hub` (see `crates/domovoi-hub/examples/dev_hub.rs`) seeds incidents and a host profile but builds the API with `triage: None` (line ~500): it does not run the model path by itself. Therefore: **locally, without committing**, edit the example to spawn a `TriageService` with a `ScriptedLlm` (testkit) whose provider returns a valid report, give the fake agent link large read-tool outputs, and trigger several incidents, a follow-up question and a chat. Alternatively write an `#[ignore]`d test in `crates/domovoi-engine/tests/services/` named `measure_prompt_sizes` that builds a busy world (10 signals, 3 notes, a 4 KiB profile note, 15 tool results of 8 KiB, a 10-turn follow-up history) and prints `prompt_bytes` per call. Either is fine; do not commit the temporary example edit (`git checkout -- crates/domovoi-hub/examples/dev_hub.rs`).
3. Record in the PR description: number of calls, median, p95 and maximum prompt bytes for triage, follow-up and chat, how many calls were trimmed at 128 KiB, and the busy-fixture composition. State plainly that this is a measurement on a fixture, that R-5 is not resolved, and what cap the numbers suggest.
4. Do not change the default 131072 on your own judgement.

## Database and persistence

Migration file: `crates/domovoi-store/migrations/NNNN_llm_admission.sql`, where `NNNN` is the next unused number after the highest existing file (check the directory; prompt 08 adds files after `0005`). Follow the `schema_compat` convention prompt 08 introduced (verify how it records a minimum compatible version and mirror it; if 08 requires a registration step for new migrations, do it).

```sql
CREATE TABLE llm_admission (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    purpose    TEXT NOT NULL CHECK (purpose IN ('triage', 'question')),
    started_at INTEGER NOT NULL
) STRICT;
CREATE INDEX llm_admission_started ON llm_admission (started_at);

ALTER TABLE llm_usage ADD COLUMN model      TEXT;
ALTER TABLE llm_usage ADD COLUMN outcome    TEXT NOT NULL DEFAULT 'ok';
ALTER TABLE llm_usage ADD COLUMN latency_ms INTEGER;
```

- Timestamps are unix milliseconds, as everywhere in the store (`ms(at)` helper in the repos).
- `Store::admit` runs delete, select and conditional insert in **one transaction on the writer pool** (pool of one, so check and insert cannot interleave). Look at how `discard_pending` (`repos/incidents.rs`) or another multi-statement repo method begins a transaction and copy it. Do not use `UnitOfWork`: an admission start is an operational counter, not aggregate state, and has no journal or audit entry.
- Existing `llm_usage` rows keep `outcome = 'ok'` and `model`/`latency_ms` `NULL`.
- Never edit `0001_init.sql`.
- **N-1 upgrade test** in `crates/domovoi-store/tests/sqlite/migrations.rs`: create a database migrated up to the previous version only (use the pattern in that file or the helper from prompt 08/12 if present: `sqlx::migrate::Migrator` limited to earlier versions, or a hand-built DB), insert one `llm_usage` row with the old columns, open with `Store::open`, and assert the row now reads `outcome = 'ok'`, `model IS NULL`, that `llm_admission` exists and is empty, and that a pre-migrate backup file was taken.

## Protocol and compatibility

No wire or protocol change (agents are untouched; protocol stays 1.1). No HTTP API shape change except one new 503 body text for chat and follow-up when limits are unreadable; OpenAPI is unchanged unless the generated spec lists error examples (run `cargo xtask openapi` and commit only if it diffs). Config additions are optional keys with defaults, so existing `hub.toml` files stay valid.

## Error handling and recovery

| Situation | Exact behaviour |
|---|---|
| Hub restarts mid-hour | The window is read from `llm_admission`; the hourly cap is unchanged by the restart. |
| Settings save restarts the runtime | Same: counts survive. |
| `usage.month_total` fails, budget configured | `Refusal::Unverifiable`: no model call. Triage: `Skipped` with the "could not be checked" reason, retry in 5 min (`RetryReason::Cap`). Chat and follow-up: `TriageError::AdmissionUnavailable` -> HTTP 503. |
| No monthly budget configured, month total not needed | Not read; no new failure mode. |
| `AdmissionRepo::admit` fails | Same as above: `Unverifiable`. |
| `starts_since` fails in `retry_deferred` | Zero free slots; every due retry deferred again by 5 min; warning logged. |
| Cache hit | No `admit` call, no budget read, no row in `llm_admission`. The existing proposal re-validation still runs. |
| `fresh` (retriage) | Skips the cache, admits as before. |
| Provider call exceeds the hard deadline | `LlmError::Timeout`; one failed usage row with outcome `timeout`; the chain falls back to the next provider as for any error. |
| Provider returns an error | Error text redacted in `complete`; failed usage row (outcome by variant, zero tokens and cost); metric counted. |
| Writing the usage row fails | Warning logged; the model result is still used. |
| Prompt over the cap | Older tool outputs replaced by a placeholder, then the newest cut; call proceeds; trimmed metric incremented. If it cannot fit: `LlmError::Unavailable` naming the limit. |
| `similar()` search fails | Empty section (as today). |
| Crash between `admit` insert and the model call | The slot stays counted (a start is counted before the call and never refunded: conservative on purpose). |

## Security requirements

- **Fail closed:** every unreadable limit stops the model call. No path may treat a store error as "under the cap".
- **No approval or execution path is touched.** `HumanApproval` and `ReadTool` are not edited. `cargo xtask arch-check` must stay green.
- **Redaction:** provider error text, replayed follow-up turns and parse errors fed back to the model all pass the `Redactor` port. Redaction in `complete()` happens before the error is logged, recorded or returned.
- **No secrets in labels, usage rows or logs:** metric labels and `llm_usage.model` carry only configured identifiers. Do not log prompt text.
- **Cross-host isolation:** `similar()` returns only the incident's host's incidents (repository-level filter plus a defensive check).
- Audit: no new audit event; usage rows are the audit of model spend.

## Testing requirements

Names are fixed. Use `domovoi-testkit` fakes: `InMemoryStore` (with `fail_next`), `FakeClock`, `ScriptedLlm` (`push_text`, `push_err`, `hold`, `release`, `wait_for_request`, `requests()`), `FakeAgentLink`, `ListRedactor`. Tests never sleep: advance `FakeClock`.

**Store and testkit (contract, run on both implementations)**

- `admission_repo_contract` (new, in `contracts/triage.rs` or a new `contracts/admission.rs`): empty -> `admit(room=2)` twice `Admitted`, third `Full{starts}` with two ascending starts; `room=0` -> `Full{[]}` and nothing inserted; starts older than the window are not counted; rows older than 2 windows are deleted (observable through `starts_since(far past)`); `starts_since` ordering.
- `usage_repo_contract` extended: `record_call` rows (including a failed one with zero tokens) leave `month_total` unchanged by failures and add successes; the provided `record` still works.
- Incident `search` contract extended: same title on two hosts, `Some(host)` returns only that host's, `None` returns both, limit applies after the host filter.
- `crates/domovoi-store/tests/sqlite/migrations.rs`: `llm_admission_migration_keeps_existing_usage_rows` (the N-1 test above).

**`crates/domovoi-llm/tests/llm/deadline.rs`**

- `call_deadline_enforced`: wrap a `ScriptedLlm` with `with_deadline(.., 30s, clock)`; `hold()`; spawn `complete`; `wait_for_request()`; `clock.advance(31s)`; the task returns `Err(LlmError::Timeout{ after: 30s })`. Assert it did not return before the advance (poll once with `tokio::task::yield_now`, check `!handle.is_finished()`).
- `call_inside_deadline_returns_the_answer`: scripted `push_text`, no advance, result `Ok`, `id()`, `model()` and `supports_tools()` forwarded.
- `deadline_applies_even_while_the_provider_is_busy_streaming` is **not** testable with the fake; skip it and note that in the report.

**Engine** (`crates/domovoi-engine/tests/services/`; new file `reliability.rs` registered in `services/main.rs`, plus edits in `limits.rs` where an existing test depends on the old window)

- `admission_survives_restart`: world with `per_hour_runs = 1`; run one triage (report). Build a **second** `TriageService` over the **same** `InMemoryStore` and clock (a fresh `Shared`, as after a restart; the harness exposes the store: extend `harness.rs` with a `restart_triage()` helper if none exists). Triage another incident: `Skipped` with "per hour", and the provider saw exactly one request. Advance 61 minutes: it is admitted.
- `cache_hit_uses_no_slot`: `per_hour_runs = 1`; follow the setup of `repeat_incident_uses_cache_without_model_call`; after the cached outcome, `store.starts_since(far past)` still has exactly the one start from the original run, and a third incident with different evidence is admitted (not `Skipped`).
- `budget_read_error_fails_closed_for_questions`: budget configured; `store.fail_next(StoreError::Backend(..))` (verify the real `StoreError` variants) so the first call, `month_total`, fails; `follow_up` returns `Err(TriageError::AdmissionUnavailable)`; the provider saw no request; no row in `llm_admission`.
- `unverifiable_admission_skips_triage_and_leaves_the_card`: same failure injected before a triage run: outcome `Skipped` with "could not be checked"; provider saw no request; an `attach_triage` event still arrives (the card path of `triage_failure_still_leaves_a_card` stays valid); after advancing 5 minutes with the store healthy the deferred retry runs.
- `failed_call_records_usage`: `primary.push_err(LlmError::Timeout{..})` then secondary answers; `store` usage rows show two rows: the failed (outcome `timeout`, zero tokens/cost, provider id, `latency_ms` set) and the success (`ok`, model recorded when the scripted provider reports one: extend `ScriptedLlm` with `with_model` **(new)** only if it has none; verify `model()` support first).
- `provider_error_redacted`: `push_err(LlmError::Unavailable(format!("upstream said {SECRET}")))` (use `SECRET` from the harness); the returned `TriageOutcome::Failed.reason`, the follow-up `TriageError::Provider` text, the stored usage row, and a captured `tracing` line (use the test's subscriber capture if one exists in this crate; otherwise assert on returned and stored values only) contain no `SECRET`.
- `replayed_follow_up_history_is_redacted`: seed a follow-up timeline entry whose stored answer contains `SECRET` (insert the entry directly through `IncidentRepo::append_event` (or its successor if the incident-write prompt renamed it; verify) as the harness does elsewhere, bypassing the store-time redaction), ask a new question, and assert no request from `primary.requests()` contains `SECRET`.
- `prompt_over_the_cap_is_trimmed_from_old_tool_results`: settings with `max_prompt_bytes = 32 * 1024`; `FakeAgentLink` returns 8 KiB for every read tool; script 6 tool-call turns then a final report; assert that every request's size (sum of text and tool-result bytes) is `<= 32 KiB`, the first user message is byte-identical across requests, the newest tool result is still present, and older ones carry the placeholder text.
- `similar_incidents_come_from_the_same_host_only`: two hosts, a resolved incident with the same check and similar title on each; trigger triage on host A; the first request text contains host A's title and not host B's.
- Unit tests in `agent.rs`: `fit_to_cap` cases (under cap unchanged; step A only; step B cut; step C error) and `redact_error` for all three string variants.

**Config:** `call_deadline` and `prompt_bytes` parse, default, and reject values below the minimums (add to the existing config validation tests; find them with `grep -rn "per_hour_runs" crates/domovoi-config/tests crates/domovoi-config/src`).

**Hub:** the three API handlers map `AdmissionUnavailable` to 503 (`crates/domovoi-hub/tests/hub/`, follow an existing chat or follow-up test and inject the failure with `fail_next` if the harness uses `InMemoryStore`; if it uses SQLite, assert the mapping with a unit test on the handler's match function instead and say so).

## Acceptance criteria

1. After a runtime restart over the same store, the number of model runs admitted in the last hour is unchanged (`admission_survives_restart`).
2. A repeat incident answered from the triage cache adds no row to `llm_admission` and never reads the monthly budget (`cache_hit_uses_no_slot`).
3. When the budget or the admission state cannot be read, no provider receives a request for triage, follow-up or chat; chat and follow-up return 503; triage returns `Skipped` and defers a retry (`budget_read_error_fails_closed_for_questions`, `unverifiable_admission_skips_triage_and_leaves_the_card`).
4. A provider built from configuration returns `LlmError::Timeout` once the hard deadline passes on the `Clock`, regardless of idle timeout (`call_deadline_enforced`); the default is 5 minutes and is configurable per provider slot.
5. No request sent to a provider exceeds `triage.per_run.prompt_bytes` (default 131072, minimum 32768) (`prompt_over_the_cap_is_trimmed_from_old_tool_results`).
6. Every provider call, successful or not, writes one `llm_usage` row with provider, model (when known), outcome and latency; a failed call writes zero tokens and zero cost (`failed_call_records_usage`).
7. A secret registered with the redactor never appears in a returned provider failure, a stored usage row, a log line from `complete`/`providers`, or a replayed request (`provider_error_redacted`, `replayed_follow_up_history_is_redacted`).
8. The similar-incidents section of a triage prompt contains only incidents of the same host (`similar_incidents_come_from_the_same_host_only`).
9. `cargo xtask docs-config` produces no diff after the change, and the generated page lists `call_deadline` and `prompt_bytes`.
10. The PR description contains the R-5 measurement and says R-5 is not resolved.
11. `crates/domovoi-engine/tests/gates.rs` is unchanged and green; `cargo xtask arch-check` is green.

## Migration and compatibility

- Existing installs: the migration is additive; old usage rows read as `ok`. The first start after upgrade has an empty admission table (the previous in-memory window is lost once, which is the same as one restart today).
- Config: both new keys are optional with defaults. The `LlmProvider` Rust struct gains a field: update literals.
- `UsageRepo::record` keeps working for other callers (provided method); `IncidentRepo::search` changes signature: only one production caller exists (verify with grep).
- Agents and the protocol are unaffected. No data is deleted.

## Documentation requirements

- Regenerate the config reference: `cargo xtask docs-config`.
- Book: if `docs/book/src` has a page describing triage limits or the LLM settings (search with `grep -rln "per_hour_runs" docs/book/src`), add two short paragraphs: the persisted hourly cap and fail-closed behaviour, the hard deadline and the prompt cap. Run `cargo xtask docs-links`.
- `ARCHITECTURE.md`: do not edit it in this change (it is updated once per milestone, after F4).
- No OpenAPI regeneration unless `cargo xtask openapi` shows a diff.

## Execution constraints

You are implementing **one** change in the Domovoi repository (a Rust workspace with an embedded React UI).
This document is your complete brief. The planning documents it cites are background: what you need is
written here.

1. **Inspect before relying on anything.** Before you edit, open every file, type and function this
   brief names.
   - Confirm that each one exists and behaves as described.
   - If something differs materially, write down the difference **before** changing code. Examples: a
     different signature, a missing module, behaviour already implemented, or a prerequisite prompt not
     merged.
   - If the difference changes what you would build, stop and report it instead of guessing.
2. **Follow the contracts here exactly.** Do not change public interfaces, wire formats, database
   schemas or security behaviour beyond what this brief specifies.
3. **Stay in scope.** No unrelated refactoring, renaming or reformatting. No new dependencies unless
   this brief names them. Add any new dependency to `[workspace.dependencies]` in the root
   `Cargo.toml` and reference it from crates with `{ workspace = true }`.
4. **Never weaken a security check, a gate test or an arch rule to make something pass.**
5. **Stop and report rather than invent.** Do this when the brief contradicts the code, an
   architectural decision is missing, or a prerequisite does not exist.
6. **Be truthful.**
   - Never claim a test passed unless you ran it and saw it pass.
   - Never present stubbed or partial work as complete.
   - Mark anything unfinished as unfinished.

## Repository rules

These come from `AGENTS.md`, which CI enforces.

- **Crate boundaries.** `domovoi-core`, `domovoi-proto` and `domovoi-engine` do no I/O. Only
  `domovoi-runner` spawns processes, always with argv vectors and never `sh -c`. Dependency edges follow
  `ARCHITECTURE.md` §4. `cargo xtask arch-check` enforces all of this.
- **Approvals.** `HumanApproval::new` is called only in `crates/domovoi-hub/src/auth/`, and
  `HumanApproval::restore` only in `crates/domovoi-store/src/`. The LLM tool catalogue stays closed.
  Nothing but an authenticated human approves an action.
- **Code rules.**
  - No `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!` or `dbg!` in library code.
  - No new `#[allow(...)]`.
  - `thiserror` in libraries; `anyhow` only in `domovoi` and `xtask`.
  - Every public item has a doc comment saying what it is for.
  - Non-test source files stay at or under 500 lines; web source files at or under 250 lines.
- **Data and time.** Never edit a merged migration; add a new one. Time comes from the `Clock` port;
  tests advance the fake clock from `domovoi-testkit` and never sleep. Secrets are
  `secrecy::SecretString` and are never logged, put into prompts or returned from the API.
- **Web.** Read `web/DESIGN.md` before touching `web/`. Build only from the kit in
  `web/src/components/`. Use `useActionFeedback` for every mutation and `describeError` for every error
  message, and tokens only for styling.
- **Tests.** Name tests after the behaviour, not the function. Never delete or weaken `tests/gates.rs`.
  Review every `insta` snapshot you accept.
- **Branch and commits.**
  - Work on a branch named as the brief says.
  - Commit messages are one line in Conventional Commits form, with exactly one scope, for example
    `feat(store): ...`. No body and no trailers.
  - Committed files, commit messages and the PR description must not mention prompts, prompt numbers,
    the build plan, coding agents or AI assistance.
- **Checks to run.** Run them for what you changed only. CI runs the full suite.
  - `cargo fmt --all`
  - `cargo clippy -p <crate> --all-targets -- -D warnings` for each crate you edited
  - `cargo nextest run -p <crate>` for each crate you edited, or `cargo test -p <crate>` if nextest
    is not installed
  - `cargo nextest run -E 'rdeps(<crate>)'` when you changed a public item that other crates use
  - `cargo xtask arch-check` when you added or moved modules or touched a `Cargo.toml`
  - for `web/`: `npm run lint`, `npm run typecheck`, and the vitest files for the changed area
  - On Windows, the hub and agent run only under WSL 2; unit and integration tests run natively.

## Completion report

End with a report in exactly this structure:

1. **Implemented behaviour**: what now works, in observable terms.
2. **Files**: added, changed and removed, each with one line on why.
3. **Implementation decisions**: any choice the brief left to repository convention, and what you chose.
4. **Interfaces**: types, functions, endpoints and messages introduced or changed, with signatures.
5. **Database and protocol changes**: migrations (file names), schema changes, wire changes.
6. **Security**: how each security requirement in the brief is met, and where it is tested.
7. **Tests added**: the test names, and which acceptance criterion each covers.
8. **Commands run**: each exact command and its actual result (pass, fail, counts).
9. **Not run**: tests or checks you did not run, and why.
10. **Deviations**: every place you departed from the brief, and why.
11. **Known limitations and remaining work**.
