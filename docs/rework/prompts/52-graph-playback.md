# 52 — Historical playback of an operation's graph

| | |
|---|---|
| Branch | `graph-playback` |
| Milestone | U2 (last deliverable of U2) |
| Depends on (must be merged) | 38 (the graph), and through it 33, 37, 30, 29, 28, 27, 26 and 59. All of these are written but may not be merged when this brief is read: **every name taken from them below is "introduced by prompt NN — verify in code before use"**. If 38 is not merged, stop and report |
| Can run in parallel with | 43, 48–51 (order-flexible, never concurrent: they all edit `web/`, `web/DESIGN.md` and several the hub `api/mod.rs`). Nothing follows it |
| Produces (shared contracts) | (1) Hub read endpoint `GET /api/v1/operations/{id}/history` and its DTOs `OperationHistoryDto`, `HistoryEventDto` (regenerated into OpenAPI and `web/src/api/schema.d.ts`). (2) Web: `useOperationHistory`, the pure `buildFrames` and `traceAt`, the playback reducer, `ReplayBar`, and the kit control `Scrubber`. (3) Two small additions to 38's `GraphView`/`OperationGraph` wiring (a replay trace and a `rememberKey`) and one context read in 30's `StepInspector` |
| Consumes | 26: the trace schema (`steps` in ordinal order, `started_at`, `ended_at`), the journal kinds `operation.created`, `operation.status`, `operation.step_created`, `operation.execution` and the ordinal rule. 27: the read-handler pattern of `GET /operations/{id}/trace` (404 and 503 behaviour, `snapshot_seq` first, DTO file, OpenAPI registration). 28: `JournalRead::entries_for`, `snapshot_seq`, `ApiDeps.journal`, `keys.operation(id)` and `applyJournal`. 29: the rule that reads stay in `api`. 38: `OperationGraph`, `GraphView`, `StepGraph`, `motionMs`, `planTransitions`, tokens. 37: categories and `Visible` |
| Launch gate | none beyond the dependencies |
| Suggested model | medium: one small read handler, one pure fold with a property-style test set, and one control bar; the judgement is in keeping the replay honest (never inventing history) and never letting it touch a decision |

## 1. Objective

Let the person **replay how an operation unfolded**, on the graph of prompt 38: move back to any earlier moment
and see which steps existed, which were pending, running or finished, and which edges joined them, then step forward
or play to the present. Concretely:

- **One new hub read endpoint**, `GET /api/v1/operations/{id}/history`, that returns a small, safe projection of the
  operation's journal rows (when each step was created, when each action was dispatched, when the status changed).
  It is the only backend change. It carries no intent, summary, output or free text.
- **A pure client-side fold**: the page already holds the operation's final trace (`GET /operations/{id}`). Replay
  rebuilds the trace **as of a moment** from that final trace plus the history, and hands it to the existing
  `OperationGraph`. No second trace schema, no per-moment request.
- **A replay bar** in the Graph view: a "Replay" toggle, a position slider (the new kit control `Scrubber`), Previous
  and Next moment, Play and Pause, speed, and "Back to live", each with a keyboard path.
- **Reduced motion**: no automatic playback; the person steps through moments with Previous, Next and the slider, and
  the graph changes instantly (D-12).
- **Honesty about gaps**: when the oldest records were pruned, or the operation has more records than one read can
  hold, the bar says so and replays only what exists. Nothing is invented.
- **Replay never decides anything.** It changes only what the graph draws. The banner, "Waiting for you", the approval
  cards, the controls and the inspector stay live, and the inspector says so while a replay is showing.

D-25: playback is the last U2 deliverable; the timeline and the operation's history stay fully useful without it.
Removing this change must leave prompt 38's graph and prompt 30's timeline untouched.

## 2. Architectural context

Everything needed is here; the cited documents are background.

**D-25 / RW-21 (playback last).** Historical playback of the orchestration graph ships last in U2. The step timeline
already covers history; playback is a convenience on top of the graph, never a requirement.

**Why a backend addition is needed (verified, do not revisit).**

- Prompt 27's `GET /api/v1/operations/{id}/trace` (and `.../{id}`) accept **no** `as_of` or position parameter: they
  return the trace as it is now, tagged with `as_of_seq` (the journal head read *before* loading, so a client applies
  only later events).
- The trace (`schema_version: 1`, frozen by prompt 26) holds, per step, `started_at` and `ended_at` (null while not started or
  not ended) but **no creation time**; a step that is still pending (an approval waiting) or was refused before it
  started has no timestamp at all. **Edges carry no time.** So "which steps existed at 10:02" cannot be read from the trace.
- The only record of creation order and time is the **journal** (prompt 26 writes one `operation.step_created` row per
  step, with `occurred_at`). Prompt 28 publishes it as a live stream (`GET /api/v1/events`), which is not random access:
  it replays only from a cursor at or above the retention floor, caps the frame payload, and is built to be consumed
  once, not scrubbed.
- Therefore replay needs the operation's journal rows through an ordinary read. The trace contract is **not** changed
  (it is frozen; adding a field would change the golden fixtures and the Rust builders).

**What the journal rows are (prompt 26 section 6.4, one `JournalKind` each, all with aggregate type `operation` and
`aggregate_id` the operation id).**

| `kind` | payload (`payload_v` 1) |
|---|---|
| `operation.created` | `{operation, kind, goal, incidents, parent}` |
| `operation.status` | `{from, to, pause_reason?, cycle}` (status names) |
| `operation.step_created` | `{step, ordinal, kind, phase, parent, cycle, intent}` |
| `operation.step_started` | `{step}` |
| `operation.step_finished` | `{step, outcome, summary?, reason?}` |
| `operation.execution` | `{step, execution, from, to, reason?}` (execution state names; the execution id equals its Execution step id) |

The endpoint projects each row to `{seq, at, kind, step, state}` and drops everything else (above all `intent`, `summary`
and `reason`, which are model- or host-derived text).

**The replay model (fixed; section 6.3 gives the algorithm).**

- A **moment** is one whole second (UTC) in which at least one journal row of the operation was recorded. Frames are the
  distinct such seconds in ascending order. Everything recorded in the same second shows together, which is also the
  burst batching D-12 asks for.
- At moment `T` a step **exists** when its `operation.step_created` row is at or before `T`. A step's outcome at `T` is
  derived from the **final** trace and its own timestamps: its final outcome from the second of `ended_at` on; `running`
  from the second of `started_at` (or of an `operation.execution` row whose `state` is `dispatched`) until then;
  `pending` before. An edge exists when both of its steps exist (the dependent step is always the newer one, so an edge
  never precedes its dependent step).
- The last moment of a history that reaches the present **is** the live state, so it is shown as "Live" and is the
  real trace, not a reconstruction.
- This is a reconstruction **from what the trace and the journal record**. It is exact to the second for creation and
  for final outcomes, and shows `running` only where `started_at` or a dispatch row says so. It never invents a step,
  an outcome or an edge, and where the history is incomplete it says so.

**Journal retention (prompt 28).** The only supported pruning primitive raises a *floor*: rows at or below it are gone.
Operation journal pruning is not applied yet (`operation_journal_days` is "not applied yet"), but the endpoint must be
honest when it happens: if the first returned row is not `operation.created`, the head of the history was pruned.

**Hub use-case rule (prompt 29).** Reads stay in `crate::api`; commands go through `crate::services`. This endpoint is a
read, so it lives in `api` and uses `ApiDeps.journal` (a `JournalFeed`, which is a `JournalRead`) and the operation
repository for the existence check. It adds **no** engine handle to `ApiDeps`.

**D-12 and prompt 38 (motion).** Playback drives the graph by feeding it a new `trace` per moment. The graph animates
only the difference and at most five nodes at once (`planTransitions`); durations come from `motionMs`, which is 0 under
reduced motion. Prompt 52 adds **no** motion token and no animation of its own. The speed of playback (the pause
between moments) is user-controlled and is not a transition.

**Prompt 37 (visibility).** New elements are classified with the four questions of 37 section 6.5 (6.8 here). P0 is shown
in every profile; a P0 element is never inside a P2 or P3 container.

**Decisions made here (do not revisit).**

1. **One endpoint, `GET /api/v1/operations/{id}/history`**, read-only, session-authenticated like every other read,
   returning `OperationHistoryDto { as_of_seq, complete, incomplete_reason, events }` (6.1). It reads at most 500 rows
   (`JournalRead::entries_for` clamps to 500) and says so when it hit the cap.
2. **Replay is a client-side fold** (`traceAt`, pure) over the final trace; the trace schema, the five fixtures and
   prompt 27's trace endpoint are untouched.
3. **Frames are whole seconds.** The unit of playback is a moment, not a journal row.
4. **History is fetched on demand**, when the person opens the replay bar, and refetched with the operation's query
   (it lives under `keys.operation(id)`, so the shell's journal stream invalidates it).
5. **The position is a moment (`atMs`) or Live**, not an index, so new history arriving while replaying never moves it.
6. **No autoplay under reduced motion.** `motionMs("layout") === 0` is the signal (the same mechanism as the tokens; no
   `matchMedia` call). Previous, Next and the slider still work.
7. **Replay touches only the graph.** Selection, the inspector, the outline and every decision control keep reading live
   data; the inspector shows a note while a replay is showing.
8. **No visualisation of anything after the latest recorded moment**, and no extrapolation.

## 3. Repository context

Verify each item in step 1. Marks: **(existing, path:line)**, **(new)**, **(introduced by NN — verify in code before use)**.
Line numbers of `web/` files and of `api/mod.rs` may have moved after prompts 27, 28, 29, 30, 37 and 38: find the symbol.

### 3.1 Hub (existing unless marked)

- `crates/domovoi-core/src/ports/uow/journal.rs:70` `JournalEntry { seq: u64, aggregate: AggregateRef, kind: String,
  payload_v: u32, payload: serde_json::Value, actor: String, occurred_at: jiff::Timestamp }` **(existing)**. `kind` is the
  stored string; readers must tolerate kinds they do not know.
- `journal.rs:143` `trait JournalRead { entries_after(after_seq, limit), entries_for(&AggregateRef, limit: u16) ->
  Result<Vec<JournalEntry>, StoreError>, head_seq() }` **(existing)**; `entries_for` returns the **oldest** `limit` rows
  of one aggregate, ordered by `seq`, with `limit` clamped to 1..=500 (`crates/domovoi-store/src/uow/reads.rs:209`).
- `AggregateRef::operation(OperationId)` and `AggregateKind::Operation` **(introduced by 26 — verify)**; the `operation.*`
  journal kinds of the table in section 2 **(introduced by 26 section 6.4 — verify the exact strings in code)**.
- `JournalFeed: JournalRead` with `floor_seq()`, `ApiDeps.journal: Arc<dyn JournalFeed>`, and
  `pub(crate) async fn snapshot_seq(state: &ApiState) -> Result<u64, ApiError>` in `crates/domovoi-hub/src/api/snapshot.rs`
  **(introduced by 28 — verify)**: it must be called **before** the data it describes is loaded.
- `crates/domovoi-hub/src/api/operations.rs`, `dto_operations.rs`, the handler `get_operation_trace`
  (`operation_id = "get_operation_trace"`), the `operations` tag, `operation_error(OperationError) -> ApiError`,
  `ApiDeps.operation_repo` **(introduced by 27 and 29 — verify)**. 27's behaviour, which this endpoint copies exactly: an
  unparsable or unknown id is 404; with orchestration off every endpoint except the two lists answers 503
  "orchestration is not enabled". After prompt 29, `ApiDeps.operations` is gone; find how `get_operation_trace` decides
  "orchestration is off" in the merged code and use the same guard.
- `crates/domovoi-hub/src/api/mod.rs` `ApiDeps` (`:135`), `ApiState` (`:194`), `Authed(pub Current)` (`:248`),
  `#[derive(OpenApi)] struct ApiDoc` (`:288`), `routes()` (`:322`), `openapi()` (`:388`) **(existing)**: a handler is a
  function with `#[utoipa::path(...)]`, registered in `routes()` (`routes!(...)`) and its DTOs are listed in `ApiDoc`'s
  components if the existing code lists them (follow what prompt 27 did for `get_operation_trace`).
- `cargo xtask openapi [--check]` writes or checks `web/src/api/openapi.json` (`xtask/src/openapi.rs`,
  `SPEC_PATH`) **(existing)**; `cd web && npm run gen:api` regenerates `src/api/schema.d.ts`. Both are generated:
  never hand-edit.
- Tests: `crates/domovoi-hub/tests/hub/main.rs` registers modules (`mod x;` under `#[cfg(test)]`); the HTTP harness is
  `tests/hub/api_harness.rs`; prompt 27's `tests/hub/operations_api.rs` builds an operation through the engine; the
  list of unauthenticated routes that must answer 401 is in `tests/hub/api_auth.rs` (the `/api/v1/events` entry is at
  about `:284`) **(existing)**. `InMemoryStore` in `crates/domovoi-testkit` has the journal helpers 28 added, including
  `prune_journal_through(seq)`.

### 3.2 Web (introduced by 30, 37 and 38 unless marked — verify every name in code)

- 38: `features/operations/graph/OperationGraph.tsx` with props `{ trace, selected, onSelect, onOpen, onClear, animate?,
  rememberKey?, label? }`; `features/operations/GraphView.tsx` (composes `OperationGraph` and, below it, the step list);
  `components/graph/StepGraph`; `design/motion.ts` `motionMs(name)` (returns 0 under reduced motion);
  `components/graph/transitions.ts` `planTransitions`; `features/operations/stepSelection.ts` `useStepSelection(trace)`.
- 30: `OperationDetailPage.tsx` (owns the `OperationDetailDto` query and mounts the inspector), `StepInspector.tsx`,
  `InspectorSections.tsx`, `steps.ts`, the vocab lookups `stepKindMeta`, `stepOutcomeMeta`, `executionStateMeta`,
  `operationStatusMeta`, `api/hooks/operations.ts` (`useOperation(id)` → `OperationDetailDto { as_of_seq, operation, trace,
  executions }`), `api/queryClient.ts` keys `keys.operation(id)` = `["operations","detail",id]` and
  `operationStep(op, step)` = `["operations","detail",op,"step",step]`.
- 28: `api/journalFrames.ts` `applyJournal(client, frame)` invalidates `keys.operation(aggregate_id)` by a predicate that
  skips a query whose cached data has `as_of_seq >= frame.seq`; `invalidateQueries` on a key matches every longer key with the
  same prefix, so a query keyed under `keys.operation(id)` is invalidated too.
- Kit (existing): `Button`, `IconButton`, `SegmentedControl` (`label`, options with optional `icon`), `InlineMessage`
  (`tone`, `title`), `LoadError`, `Skeleton`, `QueryBoundary`, `Tooltip`, `InfoTip`, `useActionFeedback` (not needed: nothing is
  mutated), `components/index.ts` (the only import point). Icons: `design/icons.tsx` `ICONS` already has `play`, `pause`,
  `chevron-left`, `chevron-right`, `clock`. No slider exists in the kit: this prompt adds `Scrubber`.
- 37: `Visible`, `useProfile`, `visibilityOf`, the ids of the operation page (`operation.*`).
- API hooks pattern: `api/hooks/incidents.ts`, `hooks.test.ts`; `api/client.ts` `api.GET`, `call<T>`; DTO aliases in `api/types.ts`;
  tests use `src/test/apiMock.ts` (`route("GET /api/v1/operations/{id}/history", handler)`, `failure(status, code, message)`),
  `renderAt`, `stubMedia(matches)` (`src/test/fixtures.ts:100`), vitest fake timers.
- The browser smoke test `web/scripts/smoke.mjs` with `smoke/{server,data,checks}.mjs` and the real-policy server of 38; the
  mock **fails the run on any `/api` request it does not list**.

### 3.3 What does not exist

No history endpoint, `OperationHistoryDto`, `api/hooks/operationHistory.ts`, `features/operations/replay/`,
`components/Scrubber.tsx`, no replay context in the inspector.

## 4. Exact scope

### 4.1 Hub (`crates/domovoi-hub`) — **(new)** unless marked

| File | Change |
|---|---|
| `src/api/operation_history.rs` **(new)** | handler `get_operation_history`, DTOs `OperationHistoryDto`, `HistoryEventDto`, the pure projection `project_row`, the constants `HISTORY_ROW_CAP = 500` |
| `src/api/mod.rs` | `mod operation_history;`, register the route in `routes()`, add the DTOs to `ApiDoc` if the module lists components |
| `tests/hub/operation_history.rs` **(new)**, `tests/hub/main.rs`, `tests/hub/api_auth.rs` | the tests of 12.1; the 401 list gains the new path |

No change under `crates/domovoi-core`, `-engine`, `-store`, `-testkit`, `-proto`, no migration, no `xtask` change (the
arch rules need none: `api` reads through ports it already owns).

### 4.2 Generated

`web/src/api/openapi.json` and `web/src/api/schema.d.ts`: regenerate with `cargo xtask openapi && cd web && npm run
gen:api`; commit the result; `cargo xtask openapi --check` passes.

### 4.3 Web (`web/`)

| File | Change |
|---|---|
| `src/api/types.ts` | aliases `OperationHistory`, `HistoryEvent` from the generated schema |
| `src/api/queryClient.ts` | `operationHistory(id)` = `["operations","detail",id,"history"]` beside 28's and 30's keys (do not redefine them) |
| `src/api/hooks/operationHistory.ts` **(new)** | `useOperationHistory(id, enabled)` (6.2) |
| `src/features/operations/replay/frames.ts` **(new)** | pure `buildFrames`, `frameCaption`, `positionsOf` (6.3) |
| `src/features/operations/replay/traceAt.ts` **(new)** | pure `traceAt` (6.3) |
| `src/features/operations/replay/playback.ts` **(new)** | pure reducer `playbackReducer` and `initialPlayback` (6.4) |
| `src/features/operations/replay/usePlayback.ts` **(new)** | the hook: reducer, play timer, reduced-motion signal, memoised frames and trace |
| `src/features/operations/replay/ReplayBar.tsx`, `replayText.ts` **(new)** | the control bar and its words (6.5) |
| `src/features/operations/replay/ReplayContext.tsx` **(new)** | `ReplayStatusProvider`, `useReplayStatus`, `useSetReplayStatus` (6.6) |
| `src/features/operations/GraphView.tsx` (38) | the "Replay" toggle, the bar, the replay trace into `OperationGraph`, the key wrapper (6.6) |
| `src/features/operations/OperationDetailPage.tsx` (30) | wrap the page content in `ReplayStatusProvider` |
| `src/features/operations/StepInspector.tsx` (30) | one `useReplayStatus()` read and one note (6.6) |
| `src/components/Scrubber.tsx`, `scrubber.module.css` **(new)**, `components/index.ts` | the kit control (6.7) |
| `src/dev/ReplayDemos.tsx`, `src/dev/DesignPage.tsx`, `src/dev/replayDemos.test.tsx` **(new)** | `/dev/design` demos |
| `scripts/smoke/{data,server,checks}.mjs` | the history route of the mock and checks 17 to 20 (6.9) |
| `DESIGN.md`, `../TESTING.md`, `../docs/book/src/concepts/operations.md` | section 15 |
| tests | 12.2 to 12.7 |

## 5. Explicit non-goals

| Not in this prompt | Where it belongs |
|---|---|
| Any change to the trace schema, the five fixtures, `build_trace`, prompt 27's trace endpoints, the journal kinds or payloads | 26, 27. Replay adapts to them; it never edits them |
| A per-moment trace endpoint (`?as_of=`), snapshots of past traces, server-side reconstruction | not needed: the fold is client-side and the history is one small read |
| A journal read with a cursor (`entries_for_after`) or any change to `JournalRead`, `Store`, the testkit | a follow-up if operations routinely exceed 500 journal rows; this prompt reports `capped` honestly instead |
| Replaying the timeline, the tree, the banner, the approval cards, budgets or the inspector's data | the timeline already covers history (D-25); only the **graph** replays |
| Playback of an operation **forward into the future**, forecasts, "what would have happened" | never |
| Any decision control in a replay (approve, deny, resume, cancel, answer) and any mutation hook | forbidden; replay is read-only |
| New motion tokens, a new graph library, changes to the keyboard model of 38, the engine or the shell of 38 | 38 owns them; replay uses `OperationGraph`'s props |
| Persisting the replay position, speed or open state | per-viewer conveniences are not needed |
| Exporting, sharing a link to a moment, a download of the history | not planned |
| Audit entries for viewing history | none: reading is not audited |
| Hub config keys, retention changes, pruning operation journal rows | 57 and later; `operation_journal_days` stays "not applied yet" |
| Chat, Discord or any other surface showing replay | not planned |

## 6. Required interfaces and contracts

Marks: **fixed** (must match exactly; tests depend on it) or **convention** (follow repository style).

### 6.1 The history endpoint **fixed**

```text
GET /api/v1/operations/{id}/history        (session cookie, the same Authed extractor as every other read)
200  OperationHistoryDto
404  not_found   an unparsable or unknown operation id            (same text as GET /operations/{id}/trace)
503  unavailable "orchestration is not enabled"                   (same guard and text as the other operation reads)
401  without a session
```

```rust
// crates/domovoi-hub/src/api/operation_history.rs
/// The most journal rows one history read returns; `JournalRead::entries_for` clamps to this.
pub const HISTORY_ROW_CAP: u16 = 500;

/// How an operation unfolded, as a projection of its journal rows. Replay rebuilds past states from this and the
/// operation's final trace.
#[derive(Debug, Clone, PartialEq, serde::Serialize, utoipa::ToSchema)]
pub struct OperationHistoryDto {
    /// The journal head read **before** the rows: rows with a larger `seq` are not included.
    pub as_of_seq: u64,
    /// True when `events` starts at the `operation.created` row and holds every row up to `as_of_seq`.
    pub complete: bool,
    /// Why not complete: `"pruned"` (the oldest rows are gone), `"capped"` (more rows than `HISTORY_ROW_CAP`: the newest
    /// are missing) or `"both"`. `null` when complete.
    pub incomplete_reason: Option<String>,
    /// Oldest first, ascending `seq`.
    pub events: Vec<HistoryEventDto>,
}

/// One journal row of the operation, reduced to what replay needs. It never carries intent, summary, reason, output or
/// any free text.
#[derive(Debug, Clone, PartialEq, serde::Serialize, utoipa::ToSchema)]
pub struct HistoryEventDto {
    pub seq: u64,
    /// `occurred_at`, RFC 3339 UTC with a trailing `Z` (the same formatter prompt 27's DTOs use).
    pub at: String,
    /// The stored journal kind string; unknown kinds pass through.
    pub kind: String,
    /// The step the row is about, when the payload names one.
    pub step: Option<String>,
    /// The new state the row records: an execution state name for `operation.execution`, an operation status name for
    /// `operation.status`; `null` otherwise.
    pub state: Option<String>,
}

pub fn project_row(entry: &JournalEntry) -> HistoryEventDto;   // pure
```

Wire example (abridged):

```json
{
  "as_of_seq": 42,
  "complete": true,
  "incomplete_reason": null,
  "events": [
    {"seq": 31, "at": "2026-01-01T00:00:00Z", "kind": "operation.created",      "step": null,      "state": null},
    {"seq": 32, "at": "2026-01-01T00:00:02Z", "kind": "operation.step_created", "step": "0190-s0", "state": null},
    {"seq": 33, "at": "2026-01-01T00:00:03Z", "kind": "operation.step_finished","step": "0190-s0", "state": null},
    {"seq": 36, "at": "2026-01-01T00:00:05Z", "kind": "operation.execution",    "step": "0190-s2", "state": "awaiting_approval"},
    {"seq": 37, "at": "2026-01-01T00:00:05Z", "kind": "operation.status",       "step": null,      "state": "waiting_approval"}
  ]
}
```

**Projection (`project_row`, fixed).** Read `payload` only when it is a JSON object and `payload_v == 1`; any field that
is missing or not a string becomes `null`; never fail.

| `kind` | `step` | `state` |
|---|---|---|
| `operation.step_created`, `operation.step_started`, `operation.step_finished` | `payload.step` | `null` |
| `operation.execution` | `payload.step` | `payload.to` |
| `operation.status` | `null` | `payload.to` |
| `operation.created` | `null` | `null` |
| anything else | `payload.step` when it is a string | `null` |

**Handler (pseudo-code, fixed order).**

```text
get_operation_history(state, authed, Path(id)):
  guard: orchestration off -> 503 (as get_operation_trace does); parse id -> 404
  as_of = snapshot_seq(&state)                                  // FIRST, before any row is read (28)
  operation must exist (OperationRepo::get, as get_operation_trace does) else 404
  rows  = journal.entries_for(&AggregateRef::operation(id), HISTORY_ROW_CAP)       // oldest first, at most 500; store error -> ApiError::internal
  capped = rows.len() == HISTORY_ROW_CAP as usize               // cannot tell "exactly 500" from "more": treat as capped
  rows.retain(|r| r.seq <= as_of)                               // a row committed after the snapshot is not part of this answer
  pruned = rows.first().map_or(true, |r| r.kind != "operation.created")
           // an existing operation always has its created row; no rows at all also means pruned
  reason = match (pruned, capped) { (false,false) -> None, (true,false) -> "pruned", (false,true) -> "capped", (true,true) -> "both" }
  Json(OperationHistoryDto { as_of_seq: as_of, complete: reason.is_none(), incomplete_reason: reason, events: rows.map(project_row) })
```

`#[utoipa::path(get, path = "/api/v1/operations/{id}/history", operation_id = "get_operation_history", tag = "operations",
params(("id" = String, Path, description = "Operation id")), responses((status = 200, body = OperationHistoryDto), (status =
404, body = ErrorBody), (status = 503, body = ErrorBody)))]` (use the exact response and security annotations of
`get_operation_trace`). **Convention**: keep the module under 500 lines; the unit tests of `project_row` may sit in the
file.

### 6.2 Web data layer **fixed** (names), **convention** (layout)

```ts
// web/src/api/types.ts
export type OperationHistory = components["schemas"]["OperationHistoryDto"];
export type HistoryEvent = components["schemas"]["HistoryEventDto"];   // { seq, at, kind, step: string | null, state: string | null }
// web/src/api/queryClient.ts: keys.operationHistory(id) = ["operations", "detail", id, "history"] as const
// web/src/api/hooks/operationHistory.ts
export function useOperationHistory(id: string | undefined, enabled: boolean): UseQueryResult<OperationHistory>;
```

`useOperationHistory` is a `useQuery` on `GET /api/v1/operations/{id}/history` through `call(...)`, enabled only when `enabled
&& id` (the replay bar is open); it sits under `keys.operation(id)`, so a journal frame for that operation invalidates it
through 28's predicate (the response has a top-level `as_of_seq`). Failures go through `LoadError` with `describeError`; no
new error-catalogue entry is needed (404 and 503 are `not_found` and `unavailable`; if the route answers a code not in
`HUB_CODES`, stop).

### 6.3 Frames and the fold **fixed** (pure; `replay/frames.ts`, `replay/traceAt.ts`)

```ts
export interface Frame { atMs: number; seq: number; events: readonly HistoryEvent[] }   // atMs: whole second, UTC ms; seq: highest seq in it
export type ReplayPosition = { kind: "frame"; atMs: number } | { kind: "live" };

/** Whole-second moments in ascending order. An event whose `at` does not parse is ignored. Events inside a frame are in seq order. */
export function buildFrames(events: readonly HistoryEvent[]): Frame[];
/** The positions the slider walks. `endsAtLive` is `history.incomplete_reason` not being "capped" or "both": the last frame
 *  IS the live state and is replaced by { kind: "live" }; otherwise live is appended. No frames: [live]. */
export function positionsOf(frames: readonly Frame[], endsAtLive: boolean): ReplayPosition[];
/** One short sentence for a moment, from the words of prompt 30's vocabularies; at most three parts, then "and N more". */
export function frameCaption(frame: Frame, final: OperationTrace): string;
/** The trace as it stood at the end of the second `atMs`. Never mutates `final`. */
export function traceAt(input: { final: OperationTrace; history: OperationHistory; frames: readonly Frame[]; atMs: number }): OperationTrace;
export function floorSecond(iso: string): number | null;     // Date.parse(iso) floored to a whole second; null when it does not parse
```

`traceAt` (pseudo-code; the tests of 12.3 pin it):

```text
T = atMs
createdAt   = Map stepId -> floorSecond(at) of its FIRST "operation.step_created" event
dispatchedAt= Map stepId -> floorSecond(at) of its FIRST "operation.execution" event whose state == "dispatched"
withCreated = final.steps that have a createdAt entry
loOrd = index (ordinal) of the first step in withCreated, hiOrd = index of the last        // steps are in ordinal order; none when withCreated is empty
exists(step, ordinal):
  if createdAt has step.id:                    return createdAt[step.id] <= T
  // no created row: its row was pruned (older than every retained row) or cut by the cap (newer than every retained row)
  if withCreated is empty:                     return history.incomplete_reason is "pruned" or "both"       // all pruned, else hidden until live
  if ordinal < loOrd:                          return true                                                  // pruned head: it existed from the start
  return false                                                                                             // ordinal > hiOrd: capped tail: hidden until live
finalMs(step):                                 // when the step's FINAL outcome starts to apply
  if step.outcome is "pending" or "running":   return null                                                  // it never ended
  if step.ended_at parses:                     return floorSecond(step.ended_at)
  return +infinity                              // a final outcome with no ended_at is shown only at Live
outcomeAt(step):
  f = finalMs(step); if f != null and f <= T:  return step.outcome
  runFrom = min over the values that exist of floorSecond(step.started_at) and dispatchedAt[step.id]
  if step.outcome != "pending" and runFrom exists and runFrom <= T:  return "running"
  return "pending"
steps = for each step with exists: { ...step, outcome: outcomeAt(step),
                                     started_at: step.started_at if floorSecond(started_at) <= T else null,
                                     ended_at:   step.ended_at   if outcomeAt(step) == step.outcome and finalMs(step) != null and finalMs(step) <= T else null }
ids   = Set of step ids kept
edges = final.edges filtered to those with both ends in ids                     // an edge appears with its dependent step
status = state of the last "operation.status" event with floorSecond(at) <= T, else "queued" when "operation.created" <= T, else final.operation.status
return { ...final, as_of_seq: highest seq of the frames with atMs <= T, operation: { ...final.operation, status }, steps, edges }
```

`summary` and every other step field keep their final values (the graph shows the final text; replay never invents text). The
`operation` block other than `status` keeps its final values; the replay never renders it (the page header is live).
`frameCaption` parts, in this order and wording: for `operation.step_created` rows, one part per distinct step kind, `"{kind label}
added"` (`"Evidence ×2 added"` when more than one; `stepKindMeta(kind).label`, the kind taken from `final.steps`; an unknown step id
or kind uses the raw string "step"); for `operation.execution` rows `"Action: {executionStateMeta(state).label}"`; for
`operation.status` rows `"Operation: {operationStatusMeta(state).label}"`; any other kind counts toward `"{n} other changes"`.
Join with "; " and cap at three parts followed by "and {n} more" when there are more; an empty list gives "Recorded".

### 6.4 The playback state machine **fixed** (pure; `replay/playback.ts`)

```ts
export type Speed = 1 | 2 | 4;
export interface PlaybackState { open: boolean; position: ReplayPosition; playing: boolean; speed: Speed; session: number }
export const initialPlayback: PlaybackState;   // { open: false, position: live, playing: false, speed: 1, session: 0 }
export type PlaybackAction =
  | { type: "open" } | { type: "close" } | { type: "live" } | { type: "pause" } | { type: "speed"; speed: Speed }
  | { type: "seek"; position: ReplayPosition }
  | { type: "previous"; positions: readonly ReplayPosition[] } | { type: "next"; positions: readonly ReplayPosition[] }
  | { type: "play"; positions: readonly ReplayPosition[]; reduced: boolean }
  | { type: "tick"; positions: readonly ReplayPosition[]; reduced: boolean }
  | { type: "positions"; positions: readonly ReplayPosition[] };       // the history changed under the person
export function playbackReducer(state: PlaybackState, action: PlaybackAction): PlaybackState;
export const FRAME_INTERVAL_MS = 1000;   // the pause between moments at 1x; the timer waits FRAME_INTERVAL_MS / speed
```

`index(positions, position)`: live is the last index; a frame position is the index of the frame position with that `atMs`,
else of the **last** frame position with a smaller `atMs`, else 0. Rules (each is a test):

| Action | Result |
|---|---|
| `open` | `open: true`, position unchanged (live), `session + 1` |
| `close` | `open: false`, position live, `playing: false`, `session + 1` |
| `live` | position live, `playing: false`, `session + 1` |
| `pause` | `playing: false` |
| `speed` | the speed; playing continues |
| `seek` | the position, `playing: false`, `session + 1` (the person took control) |
| `previous` | the position at `index - 1` (stays at index 0 at the first); `playing: false`; `session` unchanged |
| `next` | the position at `index + 1` (stays live at the last); `playing: false`; `session` unchanged |
| `play` | `reduced` true: **no change at all**. With fewer than 2 positions: no change. At live, or at the last index: position = positions[0], `playing: true`, `session + 1`. Otherwise `playing: true` from the current position |
| `tick` | only while `playing`; `reduced` true: `playing: false`. Advance to `index + 1`; reaching the last (live): position live and `playing: false` |
| `positions` | re-resolve `position` with `index` against the new list: a frame position that vanished moves to the last earlier one (else the first); `live` stays live; `playing` unchanged; `session + 1` only when the position changed |

`session` is what `OperationGraph` receives as `rememberKey` (`String(session)`), so each deliberate jump or restart can
animate its changes again while single steps keep "once" emphasis.

`usePlayback(positions)` runs one `setTimeout` per tick while `playing` (cleared on unmount, on pause, on a position
change and when `reduced` turns true): the delay is `FRAME_INTERVAL_MS / speed`; each tick dispatches `tick` with
`reduced = motionMs("layout") === 0` read at that moment. No timer runs while paused. Tests use vitest fake timers and
never sleep.

### 6.5 The replay bar **fixed** (behaviour and words), **convention** (markup)

Where: inside the Graph view, **between** the toolbar strip and the graph, in `GraphView`. The toggle is a `Button
variant="secondary"` with icon `play` labelled **"Replay"** (and **"Close replay"** while open), placed above the graph,
right-aligned. Closed is the default and fetches nothing. Opening dispatches `open`, enables `useOperationHistory`, and
shows `ReplayBar`.

```text
<ReplayBar>  role="group" aria-label="Replay"
  [Previous moment]  [Play | Pause]  [Next moment]   Speed: (1× | 2× | 4×)      [Back to live]
  Replay position  <Scrubber label="Replay position" min=0 max=positions-1 value=index valueText=…>
  <status line, role="status">   "Moment {i} of {n}: {clock}. {caption}"    |    "Live"
```

Words (fixed): buttons `IconButton icon="chevron-left" label="Previous moment"`, `IconButton icon="chevron-right"
label="Next moment"`, `Button` with icon `play` / `pause` and label "Play" / "Pause", `SegmentedControl label="Speed"` with
options "1×", "2×", "4×", `Button variant="secondary"` "Back to live". Scrubber `valueText`: `"Moment {i} of {n}, {clock}"` or
`"Live"` at the last position. `{i}` counts frame positions from 1 and `{n}` is the number of **frame** positions (live excluded);
`{clock}` is the moment's time of day in the viewer's locale and zone (`Intl.DateTimeFormat(undefined, { timeStyle: "medium" })`, in a
small helper `replay/clock.ts` with a test); at Live the status line is "Live" and nothing else.

`disabledReason` strings (fixed): Previous at the first position "Already at the first moment."; Next, and Back to live, at
live "Already showing the live state."; Play when reduced motion is on "Automatic replay is off because your system asks for
reduced motion. Use Previous moment and Next moment."; Play and the scrubber when there are fewer than two positions
"Nothing to replay yet." (the whole bar is then replaced by that sentence as muted text plus "Back to live" disabled).

States (fixed):

| State | What shows |
|---|---|
| history loading | `Skeleton` where the bar goes; the toggle stays |
| history failed | `LoadError what="load the history"` with Retry; the graph stays live |
| incomplete, `pruned` | a muted note under the bar: "The oldest records of this operation were removed, so the replay starts later than the operation." |
| incomplete, `capped` or `both` | "This operation has more records than a replay can hold, so it stops before the latest changes. Back to live shows the current state." (and, for `both`, the pruned sentence too) |
| replaying (position is not live) | **the replay note** (`InlineMessage tone="info" title="Replay: showing {clock}"`, body "The graph shows this operation as it was. Everything else on this page, including the step list below, is live.") above the graph; the graph region keeps `data-graph` and gains `data-replay="true"` on `[data-graph-frame]` (the wrapper, not the engine) |
| playing | the status line has `aria-live="off"` (no announcement every second); on pause or a manual move it is `aria-live="polite"` and announces the line |

Keyboard (fixed):

- The bar's controls are ordinary tab stops (native buttons and a native range input); Space or Enter presses a button;
  ArrowLeft and ArrowRight, Home and End and PageUp and PageDown move the scrubber (native `input type="range"` behaviour).
- **From the graph.** While the bar is open and focus is on the graph root (`[data-graph]`), a wrapper element around the
  graph handles these keys on `keydown` when `event.target` is the root, the event is not `defaultPrevented` and no modifier
  is held; each calls `preventDefault()`: `,` previous moment, `.` next moment, `Space` play or pause, `l` back to live. 38's keys
  (arrows, Enter, Escape, `+`, `-`, `0`, `a`, `Home`, `End`, `PageUp`, `PageDown`) are not touched. With the bar closed these keys do
  nothing special and are not prevented.
- Escape is **not** used by the replay (the order of the design guide is unchanged).

### 6.6 Wiring **fixed**

`GraphView` (38) gains, and nothing else changes:

```tsx
const playback = usePlayback(...);                 // state, dispatch, positions, frames, current replay trace or undefined
const shown = playback.replayTrace ?? trace;       // trace: the live trace of the page
const selectedInShown = selected && shown.steps.some(s => s.id === selected) ? selected : null;
<OperationGraph trace={shown} selected={selectedInShown} rememberKey={String(playback.state.session)} animate ... />
```

- `replayTrace` is `undefined` at live (the graph then draws the **real** trace, so all of 38's live behaviour is unchanged),
  and `traceAt({ final: trace, history, frames, atMs })` at a frame position. The step list under the graph always gets the
  live `trace`.
- While the history query has no data, or the bar is closed, `replayTrace` is `undefined`.
- Selecting a step that does not exist at the moment shown leaves `?step=` in place; the graph shows no selection.
- While replaying, the page keeps following the live stream: a new live trace changes `final` and the history is refetched;
  the person's moment (`atMs`) is kept, `positions` is dispatched, and the shown trace is recomputed. Live never jumps the position.

**Replay status for the inspector.** `replay/ReplayContext.tsx`: `ReplayStatusProvider` (mounted once by `OperationDetailPage`
around its content), `useReplayStatus(): { replaying: boolean; label: string }`, `useSetReplayStatus()`. `GraphView` sets
`{ replaying: true, label: "{clock}" }` while a replay is showing and clears it when it stops or the view unmounts. `StepInspector` (30)
reads it once and, when `replaying`, renders as its **first** block `InlineMessage tone="info" title="Showing the live record"` with
the body "The graph shows {label}. This panel shows the step as it is now." (P0, 6.8). Nothing else in the inspector changes.

### 6.7 The `Scrubber` kit control **fixed** (new primitive: it goes into the kit, `DESIGN.md` and `/dev/design` together)

```tsx
// web/src/components/Scrubber.tsx
export function Scrubber(props: {
  label: string;              // visible label and accessible name
  value: number; min: number; max: number;       // integers; step is 1
  onChange(value: number): void;
  valueText: string;          // aria-valuetext
  disabled?: boolean;
  disabledReason?: string;    // required when disabled (the lint rule `disabled-needs-reason` applies to page code; the kit follows Button's treatment)
}): JSX.Element;
```

A native `<input type="range">` with a visible `<label>`, `aria-valuetext`, and a CSS module that styles the track and thumb with tokens only
(`accent-color: var(--color-accent)`; focus ring `--shadow-focus`; a 44 px touch height; no animation). It has no tick marks, no
tooltip and no knowledge of replay. Each kit component has a `/dev/design` demo and a test (12.4).

### 6.8 Visibility **fixed** (categories follow 37 section 6.5)

| id | element | cat | mark |
|---|---|---|---|
| `operation.replay-toggle` | the "Replay" / "Close replay" button | P1 | V |
| `operation.replay-bar` | the bar: Previous, Play, Next, Speed, Back to live, the scrubber and its status line | P1 | V |
| `operation.replay-note` | "Replay: showing {clock}" (tells what the graph is showing) | P0 | V |
| `operation.replay-gap` | the pruned and capped notes (they say the data is incomplete) | P0 | V |
| `inspector.replay-note` | "Showing the live record" in the inspector | P0 | V |

All five are rendered inside `operation.graph` (P1, always shown) or the inspector's own P0 area; none is inside a P2 or P3 `Visible`.
The history loading and failure states are P0 (a load error is P0 by 37's rule) and sit inside the P1 wrapper.

### 6.9 Smoke additions **fixed** (what is checked), **convention** (code)

The mock hub (`smoke/server.mjs`, `smoke/data.mjs`) adds `GET /api/v1/operations/{id}/history`, built from the five fixtures:
one `operation.created` row, one `operation.step_created` row per step, `operation.execution` rows for execution steps
(`awaiting_approval`, `dispatched`, the final state), at times derived from the fixture's own timestamps (a step with no timestamp
gets the previous step's time plus one second), all inside one `OperationHistoryDto` with `complete: true`. It is listed among the
mock's known routes, so the "any other `/api` request fails the run" rule keeps holding.

New checks, after those of 38 (numbers continue):

17. Open the verified-success operation in the Graph view (1440 px wide): click "Replay": the bar appears; the scrubber's
    `max` equals the number of positions minus one; set it to the first position: the number of ids in `data-graph-nodes` is
    smaller than the step count and the replay note "Replay: showing" is on screen; press "Next moment": the count does not
    shrink; press "Back to live": the count equals the step count and the replay note is gone.
18. The page stays live during a replay: with the waiting-approval operation and a replay at the first position, the "Waiting for
    you" section and its Approve button are still present, the step list below still lists every step, and the inspector (open on
    a step) shows "Showing the live record".
19. **Autoplay and reduced motion.** With `emulateMedia({ reducedMotion: "no-preference" })`, "Play" advances the position within 3 s
    (the scrubber value increases) and "Pause" stops it. With `"reduce"`, "Play" is disabled and its reason text is on the page; "Next
    moment" changes the graph at once and `document.getAnimations().length === 0` within 50 ms.
20. From the graph: focus `[data-graph]`, press `.`: the position advances; press `l`: Live. No CSP violation and no `console.error`
    during checks 17 to 20.

### 6.10 Guards in the code **fixed**

- The replay code (`features/operations/replay/`, `components/Scrubber.tsx`) imports no mutation hook (nothing from
  `api/hooks/operationControls.ts`, no `useApprove*`, `useDeny*`, `useCancel*`, `usePause*`, `useResume*`, `useAnswer*`, no `.mutate(`):
  a source-scan test proves it (12.5).
- No `setInterval`; one `setTimeout` per tick (6.4). No `Date.now()` in `traceAt`, `buildFrames` or the reducer (they are pure; the
  clock helper only formats).

## 7. Ordered implementation steps

Commit in small green steps (one-line Conventional Commits, exactly one scope, for example `feat(hub): read an operation's history`).
Hub steps run on the host that can build the hub (on Windows the hub tests run natively; the hub and agent processes need WSL 2).

1. **Inspect and verify the repository context; report discrepancies before building anything.**
   - Confirm prompts 26, 27, 28, 29, 30, 37 and 38 are merged: `ls crates/domovoi-hub/src/api/operations.rs
     crates/domovoi-hub/src/api/snapshot.rs web/src/features/operations/graph/OperationGraph.tsx
     web/src/features/operations/GraphView.tsx web/src/design/motion.ts web/src/components/graph`. **If 38 or 27 or 28 is missing,
     stop and report.**
   - Open `get_operation_trace` (27) and write down: its guard for "orchestration is off", how it parses the id, how it
     checks existence, the `utoipa::path` annotations, the tag, the timestamp formatter the DTOs use, and where its tests live.
     Open `snapshot_seq` (28) and `ApiDeps` (`journal`, `operation_repo`).
   - **Verify the journal contract in code, not in this brief**: the exact `JournalKind` strings of 26 (`operation.created`,
     `operation.status`, `operation.step_created`, `operation.step_started`, `operation.step_finished`, `operation.execution`)
     and the payload field names (`step`, `to`). Verify that the writer journals **a creation row for every step, including
     steps born finished** (Assessment, Plan, Note) and steps stored `Refused`. If a kind string or field differs, adapt
     `project_row` and the tests to the merged names and list the difference; **if some steps get no creation row, stop and
     report** (replay would have to guess).
   - Verify with `grep -rn "OperationWrite::Edge\|Edge(" crates/domovoi-engine/src/operations` that edges are written together with
     their dependent (newer) step. If an edge can be added later to an existing step, say so: the rule "an edge appears with its
     dependent step" then shows such an edge early, and the report and the book page must state it.
   - Open the five fixtures and note which steps have no `started_at` and `ended_at`. Read `StepInspector.tsx`,
     `GraphView.tsx`, `OperationDetailPage.tsx`, `OperationGraph.tsx` and `useStepSelection`; note every difference from 3.2.
   - Confirm `cargo xtask openapi --check` and `npm run gen:api` leave no diff on the base; `git status` clean; branch based on the
     latest `overhaul-investigation`.
2. Create branch `graph-playback` from the latest `overhaul-investigation`.
3. **Baseline.** `cargo nextest run -p domovoi-hub`, `cargo xtask openapi --check`, and in `web/`: `npm ci && npm test && npm run
   lint && npm run typecheck && npm run build`. All green before you start; if not, stop and report.
4. **Hub, tests first.** Write the tests of 12.1 (they fail), then `src/api/operation_history.rs` (`project_row`, handler, DTOs),
   register the route, add the 401 entry. `cargo fmt --all`, `cargo clippy -p domovoi-hub --all-targets -- -D warnings`,
   `cargo nextest run -p domovoi-hub`, `cargo xtask arch-check` (a module was added). Commit `feat(hub): read an operation's history`.
5. **Generated API.** `cargo xtask openapi && cd web && npm run gen:api`; read the diff (only the new path and two schemas);
   `cargo xtask openapi --check` passes. Commit `chore(web): regenerate the API types for operation history`.
6. **Pure web logic, tests first.** `frames.ts`, `traceAt.ts`, `playback.ts`, `clock.ts` with the tests of 12.3 and 12.4, run on the
   five fixtures with histories built by a test helper (6.9 describes the shape). Commit `feat(web): replay frames, the trace fold and playback rules`.
7. **Kit control.** `Scrubber`, its demo and test, the barrel export. Commit `feat(web): a scrubber control for the kit`.
8. **Hook, bar, wiring.** `useOperationHistory` and the query key, `usePlayback`, `ReplayBar`, `replayText.ts`, `ReplayContext`, the
   `GraphView` changes, the one block in `StepInspector`, the provider in `OperationDetailPage`, the demos, and the tests of 12.5 to 12.7.
   Every test of 30, 37 and 38 passes unchanged. Commit `feat(web): replay an operation's graph`.
9. **Smoke.** The mock route, checks 17 to 20; `npm run build && npm run smoke` green. Commit `test(web): browser checks for replay`.
10. **Look at it.** `node scripts/screens.mjs` (set `SCREENS_OPERATION`) with the bar open: both themes, 375 / 768 / 1280 / 1440 px; step
    through the five fixtures by hand with the mouse and with the keyboard only; check reduced motion in the browser (Play disabled,
    instant steps); look for a graph that jumps, a clipped bar, an unreadable status line. Fix what you see. Do not commit pictures.
11. **Docs.** `DESIGN.md`, `TESTING.md`, the book page (section 15). Commit `docs(web): document replay`.
12. **Checks and completion report.** `cargo fmt --all`; `cargo clippy -p domovoi-hub --all-targets -- -D warnings`; `cargo nextest
    run -p domovoi-hub`; `cargo xtask arch-check`; `cargo xtask openapi --check`; in `web/`: `npm run lint`, `npm run typecheck`, `npm
    test`, `npm run format:check`, `npm run build`, `npm run smoke`. End with the completion report.

## 8. Database and persistence

Not applicable: no migration, no table, no stored state. The endpoint reads the existing `journal` table through
`JournalRead::entries_for`, which uses the existing index `journal_aggregate (aggregate_type, aggregate_id, seq)`
(`crates/domovoi-store/migrations/0006_unit_of_work.sql:19`), so the read is bounded by the operation, not by the journal size.
Replay position, speed and the open state live in component memory only. No transaction is opened; the read uses the store's read
pool. There is no N-1 upgrade case.

## 9. Protocol and compatibility

- **Wire**: no agent protocol change (`PROTOCOL_VERSION` stays 1.2, RW-42). **HTTP**: one new additive route; no existing route,
  DTO or field changes; the trace schema stays `schema_version: 1`.
- **OpenAPI and generated types** are regenerated (`cargo xtask openapi`, `npm run gen:api`) and committed; CI's `gen:api` diff
  check passes.
- **Open vocabularies.** `kind` and `state` are open strings: an unknown journal kind or state is carried through and counted as
  "other changes" in a caption; it never throws. An unknown step id in the history (not in the final trace) is ignored.
- **Newer traces.** If the trace's `schema_version` is not 1, the page already warns (30); replay still folds what it can.
- **Retention.** If operation journal rows are pruned later, `complete` and `incomplete_reason` tell the client; replay degrades as
  described and never errors.
- The UI is embedded in the hub, so the browser and the endpoint ship together.

## 10. Error handling and recovery

| Situation | Exact behaviour |
|---|---|
| Prompt 38, 27 or 28 is not merged | stop in step 1 and report |
| The merged journal kind strings or payload fields differ from section 2 | adapt `project_row` and tests to the merged names; list every difference in the report |
| Some steps have no `operation.step_created` row in the merged writer | stop and report: replay would have to guess |
| Operation id unparsable or unknown | 404 `not_found`, same text as the trace endpoint |
| Orchestration is off | 503 `unavailable` "orchestration is not enabled", same guard as the other operation reads |
| No session | 401 |
| Store error reading the journal | 500 through `ApiError::internal`; no detail in the body |
| The operation exists but no journal rows are left | 200, `events: []`, `complete: false`, `incomplete_reason: "pruned"`; the client shows "Nothing to replay yet." |
| First row is not `operation.created` | `incomplete_reason: "pruned"` (or `"both"` with the cap) |
| Exactly 500 rows returned | treated as capped: `"capped"` (or `"both"`); the bar says it stops before the latest changes |
| A row committed after `snapshot_seq` | not included (`seq <= as_of_seq`); the stream invalidation refetches |
| A payload is truncated, not an object, or `payload_v` is not 1 | the row is still listed with `step` and `state` null |
| An `at` that does not parse in the browser | that event is ignored by `buildFrames`; with no usable event: "Nothing to replay yet." |
| Times not monotonic with `seq` (clock skew) | frames are ordered by time; events keep their `seq`; no error |
| A step id in the history that is not in the final trace | ignored |
| A final-trace step with no creation row | the ordinal rule of 6.3: before every retained row it existed from the start (pruned), after them it appears only at Live (capped) |
| History query fails when the bar opens | `LoadError` "load the history" with Retry; the graph stays live |
| History refetch fails while replaying | keep the last good history and keep replaying; no new message (the page's own stale note covers a failed page refresh) |
| New history arrives while replaying | the moment (`atMs`) is kept; `positions` is dispatched; a vanished moment snaps to the nearest earlier one; the shown trace is recomputed |
| Reduced motion is on | Play is disabled with its reason; Previous, Next and the slider work; the graph changes instantly (its durations are 0) |
| Reduced motion turns on while playing | the next tick stops playback |
| The person leaves the Graph view, or the page | the bar closes, the timer is cleared, the replay status is cleared; coming back starts closed at Live |
| Fewer than two positions | "Nothing to replay yet."; the slider and Play are disabled with the reason |
| The selected step does not exist at the moment shown | `?step=` stays; the graph shows no selection; the inspector keeps the live record and its note |
| Any step of this brief cannot be done as written | stop and report the difference, as the execution constraints require |

## 11. Security requirements

- **Read-only, authenticated.** The endpoint uses the same session extractor as every other read, changes nothing, needs no
  step-up and writes no audit entry. Without a session it answers 401.
- **No free text leaves the hub.** The projection returns ids, timestamps, kind strings and state names only. `intent`, `summary`,
  `reason`, output, goals and any payload text are dropped. A test seeds an intent containing a marker string and asserts the
  response does not contain it. Ids are UUIDs already visible in the trace.
- **Bounded.** At most 500 rows per call, read through the existing aggregate index; no query parameter lets a caller widen it, and
  no parameter selects another aggregate (the aggregate is built from the path id).
- **The hub's authority is untouched.** The endpoint adds no engine handle to `ApiDeps` and no command; the arch rules of 29 and
  `cargo xtask arch-check` stay green. Nothing but an authenticated human approves an action (INV-1): replay has no approve, deny,
  cancel, pause, resume or answer path, and a source-scan test proves the replay code imports no mutation hook (12.5).
- **Replay cannot mislead a decision.** The replay note is P0 and always visible while a past state is shown; the approval cards,
  banner and Approve controls are never replayed, never hidden and never altered; the inspector says it shows the live record. A
  deep link or the URL never selects a moment (the position is not in the URL).
- **Text is text.** Captions are built from vocabulary labels; any raw value (an unknown kind or state) is rendered as a React
  child. Nothing is injected as HTML.
- **No new network path or storage.** No `fetch` outside `src/api`, nothing in `localStorage`, nothing logged.
- **Honest data.** The replay never invents history: incompleteness is stated (`pruned`, `capped`), steps with no creation row follow
  the ordinal rule, and the last position is the real live trace.

## 12. Testing requirements

Tests are named after behaviour. The hub tests use the existing `api_harness.rs` and 27's operation harness (`InMemoryStore`,
`FakeClock`; tests never sleep). Web tests are vitest files next to the code; every page-level test is also an axe check; timers use
vitest fake timers and never sleep.

### 12.1 Hub (`crates/domovoi-hub/tests/hub/operation_history.rs`, registered in `tests/hub/main.rs`)

| Test | Setup, action, expected |
|---|---|
| `history_lists_the_operation_rows_in_order_with_projected_fields` | create an operation, propose steps and an execution, approve it (27's harness); `GET .../history`: 200, `complete: true`, `incomplete_reason: null`, events ascending by `seq`, the first is `operation.created`, there is one `operation.step_created` per step with its `step` id, `operation.execution` rows carry `state` (`awaiting_approval`, `authorized`, `dispatched`, …), `operation.status` rows carry the status |
| `history_never_returns_intent_summary_reason_or_payload_text` | a step whose intent and summary contain the marker `SECRET-MARKER-9`: the response body does not contain it; the DTO has only the five fields |
| `history_reads_the_journal_head_before_the_rows` | like 27's `operation_reads_carry_as_of_seq_read_before_loading`: a store hook commits a row between `snapshot_seq` and the read; `as_of_seq` equals the head the snapshot saw and the late row is not in `events` |
| `history_ignores_rows_of_other_aggregates` | rows of another operation and of an incident exist: none appears |
| `history_marks_a_pruned_head` | `prune_journal_through(<seq of operation.created>)`: `complete: false`, `incomplete_reason: "pruned"`, the first event is not `operation.created` |
| `history_with_every_row_pruned_is_pruned_and_empty` | prune through the head: `events: []`, reason `pruned` |
| `history_marks_a_capped_list` | commit more than 500 journal rows for one operation (a transaction of journal drafts through the testkit store): exactly 500 events, reason `capped`; with a pruned head as well: `both` |
| `history_of_an_unknown_or_malformed_id_is_404` | a random uuid and `not-an-id` |
| `history_answers_503_when_orchestration_is_off` | the same guard and text as the trace endpoint |
| `history_requires_a_session` | `api_auth.rs`: the path is in the 401 list |
| `unknown_journal_kinds_pass_through_with_null_fields` | a row of kind `operation.future_thing` with a payload of the wrong shape: listed, `step` and `state` null |
| `project_row_reads_only_version_1_object_payloads` | unit tests of `project_row`: each row of the projection table; `payload_v: 2`; payload a string; `step` a number: all give nulls and never panic |
| `history_is_in_the_generated_api_description` | `cargo xtask openapi --check` passes with the route and both schemas present |

### 12.2 Kit (`src/components/scrubber.test.tsx`, `src/dev/replayDemos.test.tsx`)

`scrubber_exposes_label_value_and_value_text` (a range input with the label, `aria-valuetext`), `scrubber_calls_onChange_with_a_number`,
`scrubber_moves_with_arrow_keys_home_and_end` (user-event), `scrubber_disabled_shows_its_reason`, `scrubber_has_no_axe_violations`;
`replay_demos_render_without_violations` (the bar in each state: loading, failed, pruned, capped, replaying, playing, reduced motion).

### 12.3 Frames and the fold (`src/features/operations/replay/frames.test.ts`, `traceAt.test.ts`)

Helper `historyFor(trace)` **(new, test support in `src/test/replayFixtures.ts`)** builds a `OperationHistory` for a fixture: an
`operation.created` row, an `operation.step_created` row per step at the step's `started_at` (else the previous step's time plus one
second; the first at the operation's `created_at`), an `operation.execution` `dispatched` row for an execution step that has a
`started_at`, and `operation.status` rows for the operation status transitions the fixture implies; `complete: true`.

| Test | Setup, action, expected |
|---|---|
| `frames_are_whole_seconds_in_ascending_order` | events inside one second share a frame; `seq` of a frame is its highest; frames ascend; events inside keep seq order |
| `an_unparsable_time_is_ignored` | one bad `at`: the event is dropped, the rest unchanged; all bad: no frames |
| `clock_skew_orders_frames_by_time_and_keeps_seq` | seq ascending but a later row has an earlier time: frames are by time |
| `positions_replace_the_last_frame_with_live_when_the_history_reaches_the_present` | `endsAtLive` true: `[…frames but last, live]`; false: `[…frames, live]`; no frames: `[live]` |
| `a_step_does_not_exist_before_its_creation_second` | first frame of `waiting-approval`: only the steps created by then; the steps' count never decreases from one frame to the next |
| `a_pending_step_with_no_timestamps_appears_pending` | the approval step of `waiting-approval` is pending at every frame after its creation |
| `a_step_becomes_final_at_the_second_of_its_ended_at` | before: `pending` or `running`; from that second: its final outcome; its `ended_at` is null before |
| `a_step_is_running_between_started_at_and_ended_at` | a succeeded step with both times: `running` in between, `pending` before; a step whose final outcome is `pending` is never `running` |
| `a_dispatch_row_makes_an_execution_running_when_it_has_no_started_at` | `operation.execution` with state `dispatched` |
| `a_final_outcome_without_an_ended_at_shows_only_at_live` | strip `ended_at` from a succeeded step: it is never final in a frame |
| `edges_appear_with_both_their_steps` | every edge in a replayed trace has both ends present; none is missing when both ends exist |
| `the_fold_does_not_mutate_the_final_trace` | deep-freeze `final`; call `traceAt` for every frame |
| `the_last_frame_of_a_complete_history_reproduces_the_final_outcomes` | for each of the five fixtures with `historyFor`: `traceAt` at the last frame has the same step ids and outcomes as `final` (a property of consistent data; if a fixture breaks it, the test builder is wrong, not the fold) |
| `a_pruned_head_makes_older_steps_exist_from_the_start` | drop the created rows of the first two steps, reason `pruned`: both exist at frame 0; the rest follow their rows |
| `a_capped_tail_hides_newer_steps_until_live` | drop the created rows of the last two steps, reason `capped`: they exist in no frame |
| `both_gaps_use_the_ordinal_rule` | head and tail missing, reason `both`: head steps from the start, tail steps never in a frame |
| `no_created_rows_at_all_follows_the_reason` | `pruned`: every step exists at once; `capped`: none |
| `operation_status_follows_the_status_rows` | `status` at a frame is the last status row at or before it; before any row but after creation: `queued` |
| `unknown_kinds_states_and_step_ids_do_not_throw` | an unknown journal kind, an unknown execution state, a step id not in the trace |
| `frame_caption_uses_the_vocabulary_words_and_caps_at_three_parts` | creation of two evidence steps and one plan: "Evidence ×2 added; Plan added"; four parts give three then "and 1 more"; nothing recognisable: "Recorded"; unknown kind counted as "other changes" |

### 12.4 Playback reducer and timer (`playback.test.ts`, `usePlayback.test.tsx`)

One test per row of the table in 6.4, named after it: `open_keeps_live_and_bumps_the_session`, `close_returns_to_live_and_stops`,
`seek_pauses_and_bumps_the_session`, `previous_stops_at_the_first_position`, `next_reaches_live_and_stays`,
`play_at_live_restarts_from_the_first_position`, `play_does_nothing_under_reduced_motion`, `play_does_nothing_with_fewer_than_two_positions`,
`tick_advances_and_stops_at_live`, `tick_stops_when_reduced_motion_turns_on`, `positions_snaps_a_vanished_moment_to_the_nearest_earlier_one`,
`positions_keeps_the_moment_and_the_session_when_nothing_changed`, `speed_changes_the_delay_not_the_position`. Hook tests with fake
timers: `playing_advances_one_position_per_interval_at_each_speed` (1000, 500, 250 ms), `pausing_clears_the_timer`,
`unmounting_clears_the_timer`, `no_timer_runs_while_paused`, `reduced_motion_read_at_each_tick_comes_from_motionMs` (stub the token to
0 mid-play), `replay_clock_formats_in_the_viewers_locale`.

### 12.5 Bar, wiring, inspector (`replayBar.test.tsx`, `graphViewReplay.test.tsx`, `stepInspectorReplay.test.tsx`, `replayGuards.test.ts`)

| Test | Setup, action, expected |
|---|---|
| `replay_is_closed_by_default_and_fetches_nothing` | render the Graph view; no request to `/history`; the toggle reads "Replay" |
| `opening_replay_fetches_the_history_and_shows_the_bar_at_live` | one request; the bar shows "Live"; the graph draws the live trace (`data-graph-nodes` equals the step count) |
| `moving_to_an_earlier_moment_draws_the_past_and_shows_the_replay_note` | seek to the first position: fewer ids in `data-graph-nodes`; "Replay: showing {clock}" and `data-replay="true"` |
| `back_to_live_restores_the_live_graph_and_removes_the_note` | the live trace again, the note gone |
| `the_step_list_and_waiting_for_you_stay_live_during_a_replay` | the outline lists every step and the Approve button is present while replaying |
| `previous_and_next_have_their_disabled_reasons` | exact strings of 6.5 |
| `play_is_disabled_with_its_reason_under_reduced_motion` | stub the token to 0 |
| `play_advances_and_pause_stops` | fake timers |
| `the_scrubber_announces_the_moment_and_live` | `aria-valuetext` strings; the status line is `aria-live="off"` while playing and `"polite"` otherwise |
| `incomplete_history_says_so` | pruned, capped and both: the sentences of 6.5 |
| `history_failure_shows_a_load_error_with_retry_and_leaves_the_graph_live` | `failure(500, …)` |
| `fewer_than_two_positions_shows_nothing_to_replay_yet` | a history with one frame |
| `new_history_while_replaying_keeps_the_moment` | invalidate with a longer history: the position is unchanged |
| `selecting_a_step_not_yet_created_keeps_the_url_and_clears_the_graph_selection` | `?step=` names a later step |
| `graph_keys_comma_period_space_and_l_work_only_with_the_bar_open` | with it closed they are not prevented and do nothing; open: previous, next, play or pause, live; with a modifier or from another target: ignored |
| `the_inspector_says_it_shows_the_live_record_during_a_replay` | the note and its text; absent at live |
| `replay_status_is_cleared_when_the_view_changes` | switch to Timeline: the status clears, the bar closes, the timer is cleared |
| `every_component_has_no_axe_violations` | automatic; bar open at live, replaying, playing |
| `replay_code_has_no_mutation_hooks` | source scan of `features/operations/replay/` and `components/Scrubber.tsx`: no import of `operationControls`, `useApprove*`, `useDeny*`, `useCancel*`, `usePause*`, `useResume*`, `useAnswer*`; no `.mutate(`; no `setInterval`; no `dangerouslySetInnerHTML` |

### 12.6 Hook (`src/api/hooks/operationHistory.test.ts`)

`history_hook_requests_the_path_and_sits_under_the_operation_key`, `history_hook_is_disabled_until_enabled`,
`a_journal_frame_for_the_operation_invalidates_the_history` (through `applyJournal`, with the `as_of_seq` predicate).

### 12.7 Existing suites

Every test of prompts 30, 37 and 38 passes **unchanged**; the only edits allowed in their files are the ones named in 4.3 (list each in
the report). No gate, snapshot or guardrail is deleted or weakened. Run each new browser check (smoke 17 to 20) once against a
deliberately broken input (for example make the mock return a history with one frame, or drop the reduced-motion guard) to see it fail;
report that you did.

## 13. Acceptance criteria

1. `GET /api/v1/operations/{id}/history` returns `OperationHistoryDto` with `as_of_seq` read before the rows, rows of that operation
   only, ascending, at most 500, projected to `{seq, at, kind, step, state}`; 404, 503 and 401 behave like the trace endpoint.
2. The response never contains intent, summary, reason, output or any payload text (test with a marker string).
3. `complete`, `incomplete_reason` (`pruned`, `capped`, `both`) are correct in the tests of 12.1.
4. `cargo xtask openapi --check` and `npm run gen:api` leave no diff; `openapi.json` and `schema.d.ts` were regenerated, never
   hand-edited; `cargo xtask arch-check` passes; no change under `domovoi-core`, `-engine`, `-store`, `-testkit`, `-proto`.
5. `buildFrames`, `positionsOf`, `traceAt`, `frameCaption` and the playback reducer are pure, never mutate their inputs, and pass the
   tests of 12.3 and 12.4, including the ordinal rule and the five fixtures.
6. The replay bar is closed by default and fetches nothing until opened; opening shows Live; moving to an earlier moment draws the past
   in the existing graph and shows the P0 replay note; "Back to live" restores the real live trace.
7. The step list, "Waiting for you", the banner, approval cards and every control stay live and unchanged during a replay; the inspector
   shows "Showing the live record".
8. Play advances one moment per `FRAME_INTERVAL_MS / speed`; reaching the end stops at Live; under reduced motion Play is disabled with
   its reason, Previous, Next and the scrubber work, and no animation runs (`document.getAnimations().length === 0`).
9. The graph's keys `,` `.` `Space` `l` work only while the bar is open; the keys of the graph shell are unchanged; Escape order is
   unchanged.
10. Incomplete histories say so in the words of 6.5 and replay only what exists; nothing is invented.
11. The `Scrubber` is a kit control with a demo, a test, a `DESIGN.md` entry, token-only styling and a visible label and `aria-valuetext`.
12. The replay code has no mutation hook and no approval path (source-scan test); no `setInterval`; every source file is within its size
    limit (500 lines Rust, 250 web).
13. The smoke checks 17 to 20 pass with the hub's real policy (zero CSP violations, no `console.error`); the mock lists the new route.
14. `DESIGN.md`, `TESTING.md` and the book page are updated; "Planned" no longer lists playback.
15. `cargo fmt --all`, `cargo clippy -p domovoi-hub --all-targets -- -D warnings`, `cargo nextest run -p domovoi-hub`,
    `cargo xtask arch-check`, `cargo xtask openapi --check`, and in `web/` `npm run lint`, `typecheck`, `npm test`, `format:check`, `build`
    and `smoke` pass; the completion report lists each command with its real result and what was not run.

## 14. Migration and compatibility

- **Existing installs.** Nothing to migrate: no schema, config key or stored state. Operations exist only behind
  `[orchestration] enabled` (off by default until prompt 58's change), so most installs never call the endpoint.
- **Existing operations.** Replay works for any operation whose journal rows still exist; older or pruned ones show the incomplete-history
  note. Operations created before the journal kinds existed do not exist (the aggregate is new).
- **Hub and agents.** No wire change, no agent change, N-1 does not apply. The UI ships inside the hub, so the new route and the new
  bar arrive together; the web code does not need to handle a hub without the route.
- **Rollback.** Reverting removes the route, the bar and the control; the graph and the timeline are unaffected (D-25).

## 15. Documentation requirements

- **`web/DESIGN.md`** (same PR; quote the branch's text before editing): "Components": add `Scrubber` (when to use, when not, keyboard) and
  a paragraph on replay in the graph's entry (the bar, the position is a moment or Live, the keys `,` `.` `Space` `l`, the replay note, what is
  and is not replayed, reduced motion has no autoplay); "How people use it", "Operations": one paragraph on replay and its honesty about gaps;
  "Planned": remove the Playback row (it now exists) and keep everything else; "Checks that will fail your PR": no new row is needed unless a
  new lint or test guard was added.
- **`TESTING.md`**: the smoke test also replays an operation (checks 17 to 20) and the mock hub serves `/history`.
- **Book** `docs/book/src/concepts/operations.md`: a section "Replaying an operation" for users, no prompt numbers: where the Replay button is,
  what a moment is (one second in which something was recorded), what is and is not replayed, that the rest of the page stays live, the
  controls and keys, reduced motion, and what the two notes about incomplete history mean. State plainly that the replay is rebuilt from the
  records and the final state and shows `running` only where the records say so. Run `cargo xtask docs-links`.
- **Generated**: `web/src/api/openapi.json` and `schema.d.ts` (regenerate, never edit); `cargo xtask openapi --check` in CI.
- **No** `ARCHITECTURE.md`, config reference, ADR or `AGENTS.md` change: no rule, command or module boundary changes (a read endpoint in the
  existing `api` module).

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
- **Data and time.** Never edit a merged migration; add a new one. Choose a migration's number by listing
  `crates/domovoi-store/migrations/` on your branch and taking the next unused number; never assume it
  from a plan or a predecessor. Renumber only your own unmerged file if the branch gained a migration
  first, and describe the migration by its contents, not its number. Time comes from the `Clock` port;
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

