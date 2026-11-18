# 28 — Journal-backed SSE, `as_of_seq` snapshots and the `incident_events` copy

| | |
|---|---|
| Branch | `journal-sse` |
| Milestone | M3 |
| Depends on (must be merged) | 26 (`AggregateKind::Operation`, the `operation.*` journal kinds, `OperationRepo`); and, transitively, 08 (unit of work, `journal` table, dual-write of timeline rows), 11 (incident manager writes through the unit of work) and 57 (retention config). 24, 25 and 39 are **not** needed. |
| Can run in parallel with | 27, 29 (no dependency forces an order; run one branch at a time, and take the next free migration number when you cut the branch) |
| Produces (shared contracts) | **The SSE contract**: `GET /api/v1/events` wire format (`ready`, `journal`, `reset` frames, `id` = journal `seq`), the topic grammar and the `topics_of` mapping, `Last-Event-ID` / `after` resume, the lag and retention `reset` rule, `JournalFeed` (`floor_seq`, `wait_after`), the `as_of_seq` rule and helper `snapshot_seq`, the web hook `useJournalStream` and the pure `applyJournal` invalidation map, query keys `operations` / `operation(id)`, fixture `web/src/test/fixtures/journal-frames/frames.json`. Consumed by 30, 36, 51 (and by 27 for `as_of_seq` on its read endpoints). |
| Consumes | 08 `JournalRead`, `JournalEntry`, `UnitOfWork`, `Transaction`, `journal.legacy_event_seq`; 26 `AggregateKind::Operation` and journal kinds `operation.created`, `operation.status`, `operation.step_created`, `operation.step_started`, `operation.step_finished`, `operation.execution` (verify names in code); the `Clock` port and `FakeClock`. |
| Launch gate | none |
| Suggested model | medium. The stream state machine and the web hook need care; everything else is mechanical. |

## 1. Objective

Replace the lossy, id-less incident/action event stream with a **journal-backed** stream, and make the
journal complete.

After this change:

1. `GET /api/v1/events` streams committed `journal` rows. Every `journal` frame has `id: <seq>`. A client
   that reconnects with `Last-Event-ID: n` (or `?after=n`) gets every matching row with `seq > n`, in
   order, then the live tail.
2. When the requested `seq` is older than what the journal still holds (pruned), newer than the journal's
   head, or when a client falls too far behind, the server sends a `reset` frame and the client refetches
   its snapshots. A slow client is never buffered without bound and never silently loses events.
3. A client chooses what it receives with `topics=` (`incidents`, `approvals`, `operations`,
   `operation:<id>`; `hosts` and `conversations:<id>` are reserved names).
4. Every snapshot read the live UI depends on returns `as_of_seq`, the journal head read **before** the
   snapshot was loaded, so a client applies only events with `seq > as_of_seq`.
5. Every event type the web uses from the old stream has a journal equivalent (one gap is closed here: an
   incident fold has no journal row today).
6. A migration copies the legacy `incident_events` rows that are not yet mirrored into `journal`, once,
   with original timestamps, in `(at, seq)` order, inventing nothing. `incident_events` is **not**
   dropped here.
7. The web client resumes by `seq`, handles `reset` by refetching, and exposes a reusable hook
   (`useJournalStream`) that prompts 30, 36 and 51 build on.

## 2. Architectural context

Plain-language summary of the decisions you implement (you do not need to open the planning documents):

- **One journal (RW-15, D-02).** The `journal` table is the single record of operational events (incidents,
  actions, operations, later conversations and knowledge). It has a global `seq`
  (`INTEGER PRIMARY KEY AUTOINCREMENT`) assigned inside the unit-of-work transaction on the single writer
  connection, so `seq` order equals commit order for all writers. The journal is both the experience
  record **and** the change stream the UI replays. There is no third event store.
- **`audit_log` is separate** (security record, hash-chained) and is never streamed. `effects` is the work
  outbox and is never streamed.
- **SSE contract (RW-15, `04` section 6).** `id` = journal `seq`. On connect with `Last-Event-ID: n`: if
  `n` is below the oldest retained position send `reset`; otherwise stream rows with `seq > n` matching
  the topics, then tail. The tail is a notification that "a newer seq is committed" followed by a read
  from the table, so a lagging subscriber re-reads and loses nothing. Handlers on the client are
  idempotent and dedupe by `seq`.
- **Snapshots.** Every list/detail endpoint the live UI uses returns `as_of_seq`; the client applies only
  events with `seq > as_of_seq`.
- **Ephemeral events** (model tokens, install progress; later prompts) carry **no** `id` and are never
  replayed; their final state is a journal row. This prompt defines the naming rule but emits none.
- **Journal pruning raises the floor** (`04` section 14): clients behind it get `reset`. Journal pruning
  itself is **not** built here (key `operation_journal_days` stays "not applied yet"; incident timeline
  retention still acts on `incident_events`). This prompt builds the floor bookkeeping and the only
  supported prune primitive so the later retention work cannot break the stream contract.
- **Migration rules (`05` section 2).** Additive migrations only; a legacy table is retired only after a
  copy has shipped for one release. `incident_events` rows are copied in `(at, id)` order with original
  timestamps and typed kinds; nothing is invented. Every migration slice ships an N-1 upgrade test from a
  committed fixture database.
- **Parity before removal (`05` section 7).** "SSE v1 → journal SSE: every event type the web uses has a
  journal equivalent." Section 3.5 enumerates them and a test enforces it.
- **Approvals (INV-1, RW-11).** The stream is read-only. It adds no way to approve anything. Never add a
  command, tool or endpoint here that lets anything but an authenticated human approve an action.
- **Time.** Anything time-dependent (the keepalive) takes the `Clock` port. Tests never sleep.
- **Wire protocol is frozen at 1.2 (RW-42).** This prompt changes the browser-facing HTTP API only. No
  agent-protocol message changes.

## 3. Repository context

Verify every item in step 1; report discrepancies before coding.

### 3.1 Current SSE (to be replaced)

- `crates/domovoi-hub/src/api/events.rs` (existing): `KEEP_ALIVE` = 15 s (axum `KeepAlive`, not the
  `Clock`), `ui_event(&DomainEvent) -> Option<UiEvent>`, `events(rx)` over
  `broadcast::Receiver<DomainEvent>`, handler `stream` (`GET /api/v1/events`, `operation_id =
  "stream_events"`, `tag = "system"`). It emits only `incident` and `action` events, no `id:`, no replay,
  and a lagging receiver silently skips (`Lagged` → counter
  `domovoi_event_bus_lagged_total{consumer="sse"}`). It skips `TriageRequested`, `TriageDegraded`,
  `ActionApproved`.
- `UiEvent` (existing, `crates/domovoi-hub/src/api/dto_admin.rs:301`): `{kind, event, incident_id,
  action_id?, status?}`; the web type `UiEvent` is `Schemas["UiEvent"]` (`web/src/api/types.ts:34`).
- Route registration `routes!(events::stream)` in `crates/domovoi-hub/src/api/mod.rs` (`routes()`);
  `EVENTS_PATH = "/api/v1/events"` in `crates/domovoi-hub/src/middleware.rs:29` exempts the stream from
  the 120 s request timeout. Keep both.
- `ApiDeps` (existing, `crates/domovoi-hub/src/api/mod.rs:135`) has `bus: broadcast::Sender<DomainEvent>`
  whose **only** API reader is `events::stream`. Three constructors build `ApiDeps`:
  `crates/domovoi-hub/src/runtime/mod.rs:306`, `crates/domovoi-hub/examples/dev_hub.rs:501`,
  `crates/domovoi-hub/tests/hub/api_harness.rs:413`. `ApiState` has `shutdown: CancellationToken`
  (`with_shutdown`) that ends the stream at graceful shutdown. Auth is the `Authed` extractor (session
  cookie); `GET /api/v1/events` without a session answers 401 (`tests/hub/api_auth.rs:284`).
- Existing hub tests of the stream: `sse_stream_emits_incident_update`
  (`crates/domovoi-hub/tests/hub/api_routes.rs:290`) and `event_stream_ends_when_the_server_shuts_down`
  (`api_routes.rs:577`). The first asserts the old `incident_acked` frame and **must be rewritten**; the
  second stays valid.
- `DomainEvent`/`EventKind` (`crates/domovoi-engine/src/events.rs`) stay: the dispatcher, notifications and
  the actions executor still consume the bus. Only the SSE consumer goes away.

### 3.2 The journal today

- Table (existing, `crates/domovoi-store/migrations/0006_unit_of_work.sql`): `journal(seq INTEGER PRIMARY
  KEY AUTOINCREMENT, aggregate_type, aggregate_id, kind, payload_v, payload TEXT, actor, occurred_at
  INTEGER ms, legacy_event_seq INTEGER)`, `STRICT`; indexes `journal_aggregate`, `journal_occurred`, and
  `UNIQUE journal_legacy_event(legacy_event_seq) WHERE legacy_event_seq IS NOT NULL`.
- Ports (existing, `crates/domovoi-core/src/ports/uow/journal.rs`): `JournalKind {IncidentTimeline
  "incident.timeline", ActionTransition "action.transition"}` plus the six `operation.*` kinds added by
  prompt 26; `JournalDraft`; `JournalEntry {seq, aggregate: AggregateRef, kind: String, payload_v,
  payload: Value, actor: String, occurred_at}`; `trait JournalRead { entries_after(after_seq, limit),
  entries_for(aggregate, limit), head_seq() }` (`limit` clamped to 1..=500).
- `impl JournalRead for Store`: `crates/domovoi-store/src/uow/reads.rs` (`head_seq` is `SELECT
  COALESCE(MAX(seq), 0) FROM journal`; an unknown `aggregate_type` text is a `corrupt` store error).
  `impl JournalRead for InMemoryStore`: `crates/domovoi-testkit/src/repos/uow.rs:246` (`push_journal`
  assigns `seq = journal.len() + 1`).
- `AggregateKind` (existing, `crates/domovoi-core/src/ports/uow/transaction.rs:15`): `Incident`,
  `Action`, `Note`, plus `Operation` from prompt 26 (and `Fact`, `Relation`, `Entity` if prompt 34 has
  merged).
- **Dual write (existing, prompt 08).** `StateWrite::IncidentEvent(entry)` inserts the `incident_events`
  row **and** a mirrored journal row in the same transaction
  (`crates/domovoi-store/src/uow/writes.rs:119-132`, `crates/domovoi-store/src/uow/mod.rs:58-62`). The
  mirror is exactly `timeline_journal(entry)` (`journal.rs:120-137`): aggregate = the incident, kind
  `incident.timeline`, `payload_v` 1, actor `"system:incidents"`, `occurred_at` = the entry's time,
  payload `{"kind": entry.kind, "summary": entry.summary, "detail": entry.detail}`, and the row's
  `legacy_event_seq` = the new `incident_events.seq`. Test: `timeline_write_stores_legacy_event_seq`
  (`crates/domovoi-store/tests/sqlite/uow.rs:137`).
- The only non-test caller that writes `incident_events` outside the unit of work is none:
  `IncidentRepo::append_event` (`crates/domovoi-store/src/repos/incidents.rs:403`) is called only from
  tests and testkit. So **every** `incident_events` row written since migration 0006 has a mirror;
  rows written before 0006 do not. That is exactly what the copy fills.
- `incident_events` (existing, `0001_init.sql:97`): `seq INTEGER PRIMARY KEY AUTOINCREMENT, incident_id TEXT
  REFERENCES incidents ON DELETE CASCADE, at INTEGER ms, kind TEXT, summary TEXT, detail TEXT DEFAULT
  'null'`. Retention still purges it at 90 days (`crates/domovoi-store/src/purge.rs:83`). Incident detail
  and triage still read it through `IncidentRepo::timeline`. **Do not change any of that.**
- **The journal is not pruned anywhere today.** `purge.rs` has no journal statement. So there is no
  "minimum retained seq" to read; this prompt introduces the floor (section 6.4).
- Fixture database for the upgrade test (existing): `crates/domovoi-store/tests/fixtures/n1-schema5.db`
  (migrations 0001-0005 only, with `incident_events` rows); test file
  `crates/domovoi-store/tests/sqlite/upgrade.rs`. `MIGRATOR` and `MIN_READER` are public in
  `domovoi_store`.
- `Store` (existing, `crates/domovoi-store/src/db.rs:56`): `#[derive(Debug, Clone)] struct Store { read,
  write, options }`, built only in `Store::open` (`db.rs:133-190`). `Store` already has tokio's `sync`
  feature.
- Migrations directory `crates/domovoi-store/migrations/` ends at `0011_plan_binding.sql` plus whatever 26
  added. **Take the next free number on your branch.** Never hard-code one.

### 3.3 Where the web consumes SSE today

- `web/src/api/events.ts` (existing, ~140 lines): `EVENTS_URL`, `FIRST_RETRY_MS`, `PAUSED_AFTER_MS`,
  `MAX_RETRY_MS`, `retryDelay`, `applyEvent(client, UiEvent)`, `useLiveEvents(enabled = true): Live` where
  `Live = {paused: boolean, status: "live" | "connecting" | "offline"}`. It opens an `EventSource` with
  credentials, listens to `incident` and `action`, invalidates `keys.incidentLists`, `keys.incident(id)`,
  `keys.overview` (and `keys.actions` for `action`), and after a reconnect invalidates `keys.incidents`,
  `keys.actions`, `keys.overview`. Backoff 1 s doubling to 30 s; `paused` after 5 s down.
- Its **only** caller is `web/src/app/Layout.tsx:37` (`useLiveEvents()`), which feeds `live.paused` to the
  "Back online" toast and `live.status` to `Sidebar`/`SidebarFooter` (`LiveStatus`). Tests mock
  `useLiveEvents` in `web/src/app/layout.test.tsx` and `palette.test.tsx`; `web/src/api/events.test.ts`
  drives a `FakeEventSource`.
- Query keys (existing, `web/src/api/queryClient.ts`): `incidents`, `incidentLists`, `incident(id)`,
  `actions`, `overview`, `hosts`, ... There are **no** operations keys yet.
- Web rules: files at most 250 lines; build from the kit; no inline style; errors only through
  `describeError`. This work adds no visible UI, so `web/DESIGN.md` constraints are about file size and
  lint only.

### 3.4 Snapshot endpoints that the live UI depends on

`GET /api/v1/incidents` (`IncidentPage`, `crates/domovoi-hub/src/api/dto.rs:83`), `GET
/api/v1/incidents/{id}` (`IncidentDetail`, `dto.rs:247`), `GET /api/v1/actions` (`ActionPage`, `dto.rs:360`)
and `GET /api/v1/overview` (`OverviewDto`, `crates/domovoi-hub/src/api/overview.rs:24`). These are the
queries the old stream invalidated. They gain `as_of_seq`. Operation endpoints belong to prompt 27.

### 3.5 Parity enumeration: every v1 event and its journal equivalent

The old stream emitted exactly two SSE event names, `incident` and `action`, built from `EventKind`
(`crates/domovoi-engine/src/events.rs`, `ui_event` in `events.rs:22-58`). The web reads only `kind`
(`incident` or `action`) and `incident_id`. The equivalent journal rows (verified against
`crates/domovoi-engine/src/manager/decide.rs`, `annotate.rs`, `dismiss.rs` and
`crates/domovoi-engine/src/actions/commit.rs`):

| v1 `UiEvent.event` (from `EventKind`) | Journal row(s) in the same commit | Status |
|---|---|---|
| `incident_opened` (`IncidentOpened`) | `incident.timeline` with payload kind `incident.created`, and `status.changed` | exists |
| `incident_folded` (`IncidentFolded`) | **none today** | **gap: add `incident.folded` (section 6.3)** |
| `incident_escalated` (`IncidentEscalated`) | `incident.timeline` kind `severity.changed` | exists |
| `incident_reopened` (`IncidentReopened`) | `incident.timeline` kind `status.changed` | exists |
| `incident_resolved` (`IncidentResolved`) | `incident.timeline` kind `status.changed` (dismiss also writes `human.dismissed`) | exists |
| `incident_acked` (`IncidentAcked`) | `incident.timeline` kind `human.action` (+ `status.changed`) | exists |
| `incident_muted` (`IncidentMuted`) | `incident.timeline` kinds `human.action`, `status.changed` | exists |
| `incident_unmuted` (`IncidentUnmuted`) | `incident.timeline` kind `status.changed` | exists |
| `triage_completed` (`TriageCompleted`) | `incident.timeline` kinds `triage.report` / `triage.cached` / `triage.skipped` / `triage.failed` | exists |
| `action_changed` (`ActionChanged`) | `action.transition` (aggregate `action:<id>`, payload has `incident`, `from`, `to`) | exists |
| `action_blocked` (`ActionBlocked`) | `action.transition` with `to = "rejected"` (the loop-guard rejection commits through `transition`, `actions/reject.rs:55-70`) | exists |
| (not sent: `TriageRequested`, `TriageDegraded`, `ActionApproved`) | journal rows exist or are irrelevant; the new stream sends the approval `action.transition` as well, a harmless superset | n/a |

Incident rows carry the incident id as `aggregate_id`; action rows carry it as `payload.incident`.
`Write::Timeline(..)` entries all go through `StateWrite::IncidentEvent` and so are mirrored. The `verdict`
timeline row (`annotate.rs:44`) had no v1 event and is a superset now.

## 4. Exact scope

Build, in this order of dependency:

### 4.1 Core (`domovoi-core`, no I/O)

1. `JournalFeed` port and `JournalKind::IncidentFolded` (`"incident.folded"`).
2. The pure topic model: `Topic`, `TopicSet`, `topics_of`.

### 4.2 Store (`domovoi-store`)

3. Migration (next free number, suffix `_journal_feed`): `journal_floor` table, then the `incident_events` →
   `journal` copy.
4. `Store` implements `JournalFeed`; `Store::commit` publishes the committed head; `head_seq` becomes
   `max(MAX(seq), pruned_through_seq)`; `Store::prune_journal_through`.
5. `InMemoryStore` implements `JournalFeed` and a prune helper; the shared `journal_feed` contract suite
   runs against both.

### 4.3 Engine (`domovoi-engine`)

6. Close the parity gap: the incident manager writes one `incident.folded` journal row in the same
   transaction as each fold.

### 4.4 Hub (`domovoi-hub`)

7. Replace `api/events.rs` with a module directory `api/events/` (route, params, wire projection, stream
   state machine, stream cap). Remove `UiEvent` and `ui_event`. Remove `ApiDeps::bus`; add
   `ApiDeps::journal: Arc<dyn JournalFeed>`; update the three constructors.
8. `snapshot_seq` helper and `as_of_seq` on `IncidentPage`, `IncidentDetail`, `ActionPage`, `OverviewDto`.
9. Regenerate OpenAPI and the web API types.

### 4.5 Web (`web/`)

10. `journalFrames.ts` (types, parse, `applyJournal`), `journalStream.ts` (`useJournalStream`), a slimmed
    `events.ts` (`useLiveEvents` over the new hook), `operations` query keys, tests and the shared fixture.

### 4.6 Docs

11. Book page `docs/book/src/reference/api.md` (Events section) and a short note in
    `docs/book/src/concepts/how-it-works.md` (restart and missed events).

Files expected to change or be added (new files marked **(new)**):

| File | Change |
|---|---|
| `crates/domovoi-core/src/ports/uow/journal.rs` | `JournalKind::IncidentFolded`; `trait JournalFeed`; extend `journal_kinds_round_trip` |
| `crates/domovoi-core/src/ports/uow/topic.rs` **(new)** | `Topic`, `TopicError`, `TopicSet`, `topics_of`, unit tests |
| `crates/domovoi-core/src/ports/uow/mod.rs` | re-exports |
| `crates/domovoi-store/migrations/NNNN_journal_feed.sql` **(new)** | floor table + copy |
| `crates/domovoi-store/src/db.rs` | `Store.head: Arc<watch::Sender<u64>>`, initialised in `open` |
| `crates/domovoi-store/src/uow/mod.rs` | publish head after commit |
| `crates/domovoi-store/src/uow/reads.rs` | `head_seq` rule; `impl JournalFeed for Store` |
| `crates/domovoi-store/src/uow/prune.rs` **(new)** | `Store::prune_journal_through` |
| `crates/domovoi-testkit/src/repos/uow.rs`, `repos/mod.rs` | in-memory `JournalFeed`, seq counter, floor, prune helper |
| `crates/domovoi-testkit/src/contracts/uow/feed.rs` **(new)**, `contracts/uow/mod.rs` | `journal_feed` contract suite |
| `crates/domovoi-engine/src/manager/decide.rs`, `commit.rs`, `actor.rs` | `Write::Journal`, fold row |
| `crates/domovoi-hub/src/api/events.rs` | **delete** (replaced by the directory below) |
| `crates/domovoi-hub/src/api/events/{mod,params,wire,stream}.rs` **(new)** | the endpoint |
| `crates/domovoi-hub/src/api/snapshot.rs` **(new)**, `api/mod.rs`, `api/dto.rs`, `api/dto_admin.rs`, `api/incidents.rs`, `api/actions.rs`, `api/overview.rs` | `as_of_seq`, `ApiDeps.journal`, remove `UiEvent` |
| `crates/domovoi-hub/src/runtime/mod.rs`, `examples/dev_hub.rs`, `tests/hub/api_harness.rs` | constructors |
| `crates/domovoi-hub/tests/hub/api_events.rs` **(new)**, `tests/hub/main.rs`, `api_routes.rs` | tests |
| `crates/domovoi-store/tests/sqlite/upgrade.rs`, `tests/sqlite/journal_feed.rs` **(new)** | N-1 and copy tests |
| `crates/domovoi-engine/tests/engine/journal_parity.rs` **(new)**, `tests/engine/main.rs` | parity test |
| `web/src/api/{journalFrames,journalStream,events,queryClient,types}.ts` and tests; `web/src/test/fixtures/journal-frames/frames.json` **(new)** | web client |
| `web/src/api/openapi.json`, `web/src/api/schema.d.ts` | **regenerated, never hand-edited** |
| `docs/book/src/reference/api.md`, `docs/book/src/concepts/how-it-works.md` | docs |

## 5. Explicit non-goals

- **No dropping, renaming or rewriting `incident_events`.** It is retired in a later release, after this
  copy has shipped for one release (`05` section 2). Incident detail, triage and retention keep reading and
  purging it. Do not switch any reader to the journal here.
- **No journal pruning and no retention change.** `operation_journal_days` stays "not applied yet". Only the
  floor bookkeeping and `prune_journal_through` exist. Do not call the prune primitive from production code.
- **No new journal kinds except `incident.folded`.** Prompt 26 owns `operation.*`; prompt 34 owns
  `fact.changed`, `relation.changed`, `entity.changed` (they stay out of every topic here; section 6.2).
- **No operation endpoints** (prompt 27 owns `GET /operations...`); this prompt only defines how they obtain
  `as_of_seq`.
- **No operations UI, conversations UI or attention centre** (prompts 30, 36, 51). No visible UI change.
- **No ephemeral events.** Only the naming rule is fixed.
- **No wire (agent protocol) change.** No change to `ARCHITECTURE.md` unless a section names `GET
  /api/v1/events`.
- **No change to the audit log, the effects table, the bus consumers (dispatcher, notifier, executor).**
- **No per-stream authorization levels, no host-level topic content** (`hosts` is reserved and delivers
  nothing yet).
- Do not hand-edit `web/src/api/openapi.json` or `schema.d.ts`.

## 6. Required interfaces and contracts

### 6.1 The SSE endpoint **fixed**

```
GET /api/v1/events?topics=<comma list>&after=<seq>
Cookie: <session>                 (same Authed extractor as today)
Last-Event-ID: <seq>              (optional; sent by EventSource on native reconnect)
```

- `topics`: optional, comma separated, at most 16, each valid per section 6.2. Absent means the default set
  `incidents,approvals,operations`. An unknown or malformed topic, more than 16 topics, or a non-numeric
  `after` / `Last-Event-ID` answers **400** `bad_request` with an `ErrorBody` (not a stream).
- Resume position: the `Last-Event-ID` header if present, else the `after` query parameter, else none
  (start at the head). `0` is a valid position meaning "everything the journal still has".
- Responses: `200 text/event-stream`; `400`; `401` (not logged in, as today); `503` `unavailable` with
  message `too many open event streams` when the hub already serves `MAX_EVENT_STREAMS` (32) streams.
- Headers: whatever axum's `Sse` sets. The request-timeout middleware keeps exempting `EVENTS_PATH`.

**Frames.** Every frame is UTF-8. Names and payloads are the contract:

1. `event: ready`, `data: {"head_seq": <u64>, "resumed": <bool>, "topics": ["incidents", ...]}`. Sent first
   on a connection that is not rejected with a reset. `topics` is the canonical sorted, de-duplicated list.
   When `resumed` is `false` the frame also carries `id: <head_seq>`; when `true` it has **no** `id` (the
   cursor must not move backwards).
2. `event: journal`, `id: <seq>`, `data:` one JSON object (a `JournalFrame`):

```json
{
  "seq": 4821,
  "aggregate_type": "operation",
  "aggregate_id": "0190a0a0-0000-7000-8000-0000000000a1",
  "kind": "operation.step_finished",
  "payload_v": 1,
  "payload": { "step": "0190a0a0-0000-7000-8000-0000000000b2", "outcome": "succeeded" },
  "actor": "system:operations",
  "at": "2026-11-18T10:00:00Z",
  "topics": ["operations", "operation:0190a0a0-0000-7000-8000-0000000000a1"]
}
```

   `kind` and `aggregate_type` are open strings: a client shows an unknown one generically and never fails
   (the `Open<T>` principle). `at` is `occurred_at` as RFC 3339 UTC. `topics` lists the topics this row
   belongs to (always non-empty; a row that belongs to none is never sent). `payload` is the stored
   payload except: (a) for kind `incident.timeline` it is projected to `{"kind": <timeline kind>,
   "summary": <summary, passed through the hub's `Redactor`>}`, dropping `detail` (the full triage outcome
   is fetched by the incident detail endpoint, never pushed); (b) if the serialized payload exceeds
   `MAX_FRAME_PAYLOAD_BYTES` (16 KiB), `payload` is `null` and the object gains `"payload_truncated": true`.
3. `event: reset`, `id: <head_seq>`, `data: {"reason": "pruned" | "ahead" | "lagged", "head_seq": <u64>}`.
   Meaning: "what you hold may be incomplete; refetch your snapshots". After a reset the server continues
   with the live tail from `head_seq` (the `id` makes a later native reconnect resume from there). A reset on
   connect replaces `ready`.
4. Keepalive: an SSE comment line `: keepalive` (no `event`, no `id`), sent after `KEEP_ALIVE` (15 s) of
   silence, measured with the `Clock` port.
5. Reserved: events whose name starts with `live.` are **ephemeral**: no `id`, never replayed, never
   emitted by this prompt. Prompt 36 will define `live.*` names. A client ignores `live.*` names it does
   not know.

**Connection sequence** (pseudo-code, `topics` already parsed, `head`, `floor` read from the feed):

```
cursor_in = Last-Event-ID or after or None
match cursor_in:
  None                          -> send ready{resumed:false}(id=head); cursor = head
  Some(n) if n < floor          -> send reset{pruned}(id=head);        cursor = head
  Some(n) if n > head           -> send reset{ahead}(id=head);         cursor = head
  Some(n) if head - n > LAG_LIMIT_ROWS -> send reset{lagged}(id=head); cursor = head
  Some(n)                       -> send ready{resumed:true};           cursor = n
loop (until shutdown or the client goes away):
  head = feed.head_seq(); floor = feed.floor_seq()
  if cursor < floor or cursor > head or head - cursor > LAG_LIMIT_ROWS:
      send reset{reason}(id=head); cursor = head; continue
  if head > cursor:
      rows = feed.entries_after(cursor, BATCH_ROWS)
      for row in rows: cursor = row.seq; if set.matches(row): send journal(row)
      continue                         # yield between batches by awaiting the next read
  select:
      feed.wait_after(cursor)          -> continue
      clock.sleep_until(last_send + KEEP_ALIVE) -> send ": keepalive"; last_send = clock.now()
      shutdown                         -> end
```

Constants (**fixed**): `KEEP_ALIVE = 15 s`, `BATCH_ROWS = 100`, `LAG_LIMIT_ROWS = 5_000`,
`MAX_EVENT_STREAMS = 32`, `MAX_TOPICS = 16`, `MAX_FRAME_PAYLOAD_BYTES = 16 * 1024`.

**Backpressure rule (fixed).** The stream is pull-based: it holds a cursor and at most one batch
(`BATCH_ROWS` frames) in memory, and it reads the next batch only after the previous frames were written
to the socket. A client that stops reading parks its task (bounded memory). When it resumes, the lag check
above runs: more than `LAG_LIMIT_ROWS` rows behind means `reset{lagged}`, never an unbounded queue and never
a silent skip. No `broadcast` channel sits between the journal and a client.

### 6.2 Topics **fixed** (names), mapping **fixed**

Wire names: `incidents`, `approvals`, `operations`, `operation:<uuid>`, `hosts` (reserved),
`conversations:<uuid>` (reserved). `<uuid>` is the lowercase hyphenated UUID text. Anything else is invalid.

```rust
// crates/domovoi-core/src/ports/uow/topic.rs  (new)
/// A subscription topic of the journal stream.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Topic {
    /// Every incident row, and every action row (an incident's card shows its actions).
    Incidents,
    /// Rows that change what awaits a human decision: `action.transition` and `operation.execution`.
    Approvals,
    /// Every row of every operation.
    Operations,
    /// Every row of one operation (canonical UUID text).
    Operation(String),
    /// Reserved: host rows. Accepted, delivers nothing until a prompt adds a mapping.
    Hosts,
    /// Reserved: one conversation (canonical UUID text). Accepted, delivers nothing until prompt 36.
    Conversation(String),
}
impl Topic {
    pub fn as_wire(&self) -> String;                                  // "operation:<uuid>" etc.
    pub fn parse(text: &str) -> Result<Topic, TopicError>;            // exact grammar above
}
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TopicError { #[error("unknown topic `{0}`")] Unknown(String), #[error("`{0}` is not a valid id")] BadId(String) }

/// The set a connection asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopicSet(/* BTreeSet<Topic> */);
impl TopicSet {
    pub const MAX: usize = 16;
    pub fn parse_list(csv: Option<&str>) -> Result<TopicSet, TopicError>;  // None => default; empty entries rejected
    pub fn default_set() -> TopicSet;                                      // incidents, approvals, operations
    pub fn wire(&self) -> Vec<String>;                                     // sorted
    pub fn matches(&self, entry: &JournalEntry) -> bool;                   // any of topics_of(entry) is in the set
}
/// The topics one journal row belongs to; empty means "never streamed".
pub fn topics_of(entry: &JournalEntry) -> Vec<Topic>;
```

`topics_of` mapping (**fixed**; written as a `match` on `entry.aggregate.kind` with a final `_ => vec![]`
arm and a comment: "aggregates with no topic yet; add an arm here when a consumer needs it"):

| Aggregate | Kind | Topics |
|---|---|---|
| `Incident` | any | `Incidents` |
| `Action` | any | `Approvals`, `Incidents` |
| `Operation` | `operation.execution` | `Operations`, `Operation(id)`, `Approvals` |
| `Operation` | any other | `Operations`, `Operation(id)` |
| `Note`, and `Fact`/`Relation`/`Entity` if present, or any other | any | none (prompt 34 states its kinds stay out of every topic; adding an arm later is a one-line change) |

`Operation(id)` uses `entry.aggregate.id` as stored (it is a UUID text; if it does not parse as one, return
only `Operations`). The wildcard arm is deliberate so that a later prompt adding an `AggregateKind` variant
does not break compilation of this function.

### 6.3 `incident.folded` **fixed**

```rust
JournalKind::IncidentFolded   // as_str "incident.folded", parse round-trips
```

Payload, `payload_v` 1: `{"signal": "<signal id>", "occurrences": <u32>}`. Aggregate: the incident; actor
`Actor::System(SYSTEM_INCIDENTS)`; `occurred_at` = the command time. Written by the incident manager in the
**same transaction** as the fold. In `crates/domovoi-engine/src/manager/decide.rs`, `Write` gains a variant
`Write::Journal(JournalDraft)`; `Builder::record` pushes it for `IncidentChange::Folded { occurrences }`
exactly when it publishes `EventKind::IncidentFolded` (i.e. when `signal` is `Some`). In `commit.rs`,
`build_transaction` maps `Write::Journal(draft)` to `tx = tx.journal(draft.clone())` (the unit of work
applies explicit journal rows after the mirrored ones; order within a command does not matter to
consumers). `record_versions` and `Versions` ignore it; `actor.rs::remember` ignores it
(`Write::InsertSignal(..) | Write::Timeline(_) | Write::Journal(_) => {}`). Volume note for the PR: a fold
already stores a signal row and an incident update, so one small journal row per fold is the same order of
magnitude; do not add rate limiting here.

### 6.4 `JournalFeed`, the floor and the head **fixed** (trait), **convention** (impl)

```rust
// crates/domovoi-core/src/ports/uow/journal.rs
/// The journal as a change stream: what is gone, and a way to wait for what is new.
#[async_trait]
pub trait JournalFeed: JournalRead {
    /// The highest `seq` that retention has ever deleted; 0 when nothing was pruned. A reader whose
    /// cursor is below this may have missed deleted rows and must refetch.
    async fn floor_seq(&self) -> Result<u64, StoreError>;
    /// Completes once the committed head is greater than `seq`; at once if it already is. Cancel-safe.
    /// Never reads rows: the caller re-reads with `entries_after`.
    async fn wait_after(&self, seq: u64);
}
```

Semantics (**fixed**):

- `head_seq()` becomes `max(MAX(seq) FROM journal, pruned_through_seq)`, so deleting the newest rows never
  moves the head backwards.
- A cursor `n` is **valid** iff `floor <= n <= head`. `n < floor` means pruned; `n > head` means ahead (for
  example the database was restored from an older backup).
- `Store` keeps `head: Arc<watch::Sender<u64>>` (initialised in `Store::open` after migrations from
  `head_seq`). `Store::commit` calls `self.head.send_modify(|h| *h = (*h).max(highest))` with the highest
  `receipt.journal_seqs` **after** `COMMIT` succeeds, never before and never on rollback. `wait_after` is
  `let mut rx = self.head.subscribe(); let _ = rx.wait_for(|h| *h > seq).await;`.
- Pruning (**fixed**): the only supported way to delete journal rows is
  `Store::prune_journal_through(&self, seq: u64) -> Result<u64, StoreError>` (inherent method, new file
  `uow/prune.rs`): in one transaction on the write pool, `DELETE FROM journal WHERE seq <= ?`, then
  `UPDATE journal_floor SET pruned_through_seq = MAX(pruned_through_seq, ?)`; returns the number of rows
  deleted. Its doc comment says: any future retention code that deletes journal rows by age or kind must
  raise `pruned_through_seq` to at least the highest `seq` it deleted, in the same transaction. Nothing in
  production calls it in this prompt.
- `InMemoryStore` implements the same semantics: replace `seq = journal.len() + 1` by a monotonically
  increasing counter in `State`, add `journal_floor`, a `watch::Sender<u64>` updated at the end of a
  successful commit, and a test helper `prune_journal_through(seq)`.

### 6.5 `as_of_seq` **fixed**

- JSON field `as_of_seq: u64` (OpenAPI `integer`, `format: int64`, non-negative) on `IncidentPage`,
  `IncidentDetail`, `ActionPage`, `OverviewDto`.
- Rule: the handler reads `snapshot_seq(&state)` **first**, then loads the data. The value may be lower than
  the true state of the snapshot (an event committed in between is both in the snapshot and replayed
  later), never higher. Replays are idempotent (invalidation), so this is safe.

```rust
// crates/domovoi-hub/src/api/snapshot.rs  (new)
/// The journal head to report as `as_of_seq`; read before the snapshot it describes.
pub(crate) async fn snapshot_seq(state: &ApiState) -> Result<u64, ApiError>;   // store error -> ApiError::internal
```

Prompt 27's operation endpoints (list, detail, trace) must call `snapshot_seq` first and pass the value to
`build_trace(snapshot, as_of_seq)` and into their responses. If those endpoints already exist when you run,
add `as_of_seq` to them using the same helper.

### 6.6 Migration contents **fixed**

File `crates/domovoi-store/migrations/<next free>_journal_feed.sql`:

```sql
-- Highest journal seq that retention deleted. A stream cursor below it must reset.
CREATE TABLE journal_floor (
    id                 INTEGER PRIMARY KEY CHECK (id = 1),
    pruned_through_seq INTEGER NOT NULL
) STRICT;
INSERT INTO journal_floor (id, pruned_through_seq) VALUES (1, 0);

-- Copy the incident_events rows the dual write has not mirrored. Rows written since the dual write
-- began already have a journal row whose legacy_event_seq names them; rows from before it do not.
-- The shape is exactly the mirror's (see timeline_journal): kind incident.timeline, payload
-- {kind, summary, detail}, actor system:incidents, the original timestamp. Nothing is invented.
INSERT INTO journal (aggregate_type, aggregate_id, kind, payload_v, payload, actor,
                     occurred_at, legacy_event_seq)
SELECT 'incident', e.incident_id, 'incident.timeline', 1,
       json_object('kind', e.kind, 'summary', e.summary,
                   'detail', json(CASE WHEN json_valid(e.detail) THEN e.detail
                                       ELSE json_quote(e.detail) END)),
       'system:incidents', e.at, e.seq
FROM incident_events AS e
WHERE NOT EXISTS (SELECT 1 FROM journal AS j WHERE j.legacy_event_seq = e.seq)
ORDER BY e.at, e.seq;
```

Notes the executor must keep: the copied rows get `seq` values above every existing row, in `(at, seq)`
order of the legacy table (`05` section 2: `(at, id)` order, the legacy table's id is `seq`); consumers that
list an incident's timeline from the journal must therefore order by `(occurred_at, seq)`, never by `seq`
alone. `MIN_READER` is **not** raised (additive; the previous binary ignores `journal_floor` and already
reads `incident.timeline`). The statements must not contain `DROP`, `ALTER` or `DELETE`.

**Reconciliation with the migration plan (`05` section 2).** That text says "kinds mapped to typed kinds;
unmappable kinds copied as `Legacy{raw_kind}`". The merged dual write (prompt 08) already established the
typed shape: **one** typed kind `incident.timeline` whose payload carries the original free-text kind in
`payload.kind`. The copy matches the mirror byte-for-byte in shape, so a legacy kind the code does not know
is still "typed" (`incident.timeline`) with its raw string preserved, and there is no separate `Legacy`
journal kind. Do not add one.

### 6.7 Web contracts **fixed** (names)

```ts
// web/src/api/journalFrames.ts
export interface JournalFrame {
  seq: number; aggregate_type: string; aggregate_id: string; kind: string; payload_v: number;
  payload: Record<string, unknown> | null; payload_truncated?: boolean; actor: string; at: string; topics: string[];
}
export interface ReadyFrame { head_seq: number; resumed: boolean; topics: string[] }
export interface ResetFrame { reason: "pruned" | "ahead" | "lagged" | string; head_seq: number }
export function parseJournalFrame(data: unknown): JournalFrame | undefined;   // undefined on malformed
export function applyJournal(client: QueryClient, frame: JournalFrame): Promise<void>;
export function refetchLive(client: QueryClient, atLeast?: number): Promise<void>;

// web/src/api/journalStream.ts
export const JOURNAL_EVENTS_URL = "/api/v1/events";
export interface JournalStreamOptions {
  topics: readonly string[];
  enabled?: boolean;
  onFrame: (frame: JournalFrame) => void;          // called once per seq, in order, never for seq <= last seen
  onReset?: () => void;                            // after reset or a fresh (non-resumed) connection
}
export function useJournalStream(options: JournalStreamOptions): Live;     // Live is the existing type

// web/src/api/events.ts (kept path; Layout keeps importing from here)
export function useLiveEvents(enabled?: boolean): Live;   // = useJournalStream over the default topics + applyJournal
```

Query keys added to `web/src/api/queryClient.ts` (**fixed**): `operations: ["operations"] as const`,
`operation: (id: string) => ["operations", "detail", id] as const`. Prompt 30 reuses them.

`applyJournal` invalidation map (**fixed**): by `aggregate_type`:
`incident` → `keys.incidentLists`, `keys.incident(aggregate_id)`, `keys.overview`;
`action` → `keys.actions`, `keys.incidentLists`, `keys.incident(payload.incident)` (only when it is a string),
`keys.overview`;
`operation` → `keys.operations`, `keys.operation(aggregate_id)`, `keys.overview`;
any other type → nothing. Each invalidation uses a predicate that skips a query whose cached data has
`as_of_seq >= frame.seq` (the snapshot already contains the event). Exact predicate: `((q.state.data as
{as_of_seq?: number} | undefined)?.as_of_seq ?? 0) < frame.seq`.

### 6.8 Implementation notes **convention**

Hub (`crates/domovoi-hub/src/api/events/`, each file at most 500 lines):

- `params.rs`: `EventsQuery { topics: Option<String>, after: Option<String> }` (`Deserialize`, `IntoParams`,
  `parameter_in = Query`), `Resume::from_request(header, query)` (header wins), parsing into `u64`; errors map to
  `ApiError::BadRequest`.
- `wire.rs`: `JournalFrame` (`Serialize`; build it from a `JournalEntry` with `topics_of`, the projection and
  truncation rules of 6.1, `at` formatted with jiff's RFC 3339 `Display`), `ReadyFrame`, `ResetFrame`, and one
  function per frame that returns an `axum::response::sse::Event` (`Event::default().event(..).id(..).json_data(..)`;
  a keepalive is `Event::default().comment("keepalive")`).
- `stream.rs`: the state machine of 6.1 as a `futures::stream::unfold` over a private state struct (cursor,
  pending batch as `VecDeque<Event>`, `last_send: Timestamp`, the `Arc<dyn JournalFeed>`, the `Arc<dyn Clock>`,
  the `TopicSet`, the redactor, and a `StreamGuard`). Wait with `tokio::select!` over `feed.wait_after(cursor)` and
  `clock.sleep_until(last_send + KEEP_ALIVE)`. Do **not** use axum's `KeepAlive` (it uses the wall clock). Keep
  `.take_until(state.shutdown.cancelled_owned())` in the handler.
- `mod.rs`: the `#[utoipa::path(get, path = "/api/v1/events", operation_id = "stream_events", tag = "system", params(EventsQuery), ...)]`
  handler, `KEEP_ALIVE` (keep the existing `pub const` name and value), the other constants, and `StreamGuard`
  (an `Arc<AtomicUsize>` stored in `ApiState`, incremented with a compare-and-add against `MAX_EVENT_STREAMS`,
  decremented in `Drop`). Metrics through the existing `metrics` crate: gauge `domovoi_sse_streams_open`, counter
  `domovoi_sse_frames_total{kind}`, counter `domovoi_sse_resets_total{reason}`. The old
  `domovoi_event_bus_lagged_total{consumer="sse"}` counter label disappears; leave the metric name for the other
  consumers.
- Register `routes!(events::stream)` unchanged in `api/mod.rs`; `pub mod events;` still resolves.

Web (`web/src/api/`):

- `journalStream.ts` generalises the existing `useLiveEvents` body: one `EventSource` (`withCredentials: true`),
  listeners for `ready`, `journal`, `reset` and `error`, a `lastId` ref updated from every frame that has a `seq` (and
  from `reset` and a non-resumed `ready`), a reconnect URL built as
  `/api/v1/events?topics=<topics joined with commas>` plus `&after=<lastId>` when `lastId` is known, the existing backoff
  (`FIRST_RETRY_MS`, `MAX_RETRY_MS`, `retryDelay`, `PAUSED_AFTER_MS` move here and stay exported from `events.ts` too),
  and `status` / `paused` exactly as today. The old "refresh everything after any reconnect" is replaced by the `ready` and
  `reset` rules, so a clean resume costs no refetch. Options must be read through refs so changing `onFrame` does not
  reconnect; `topics` changes do (compare the joined string).
- `journalFrames.ts` holds the types, `parseJournalFrame`, `applyJournal` and `refetchLive` (invalidate `keys.incidents`,
  `keys.actions`, `keys.overview`, `keys.operations`, using the same `as_of_seq` predicate with the head as `frame.seq`).
- `events.ts` keeps `EVENTS_URL` (alias of `JOURNAL_EVENTS_URL`), the retry constants and `useLiveEvents`, now
  `useJournalStream({ topics: ["incidents", "approvals", "operations"], onFrame: (f) => void applyJournal(client, f) })`.
  Delete `applyEvent` and the `UiEvent` import; remove `UiEvent` from `types.ts`.
- Pages that need another topic (`conversations:<id>` in prompt 36) call `useJournalStream` themselves with that topic and
  their own `onFrame`; pages that only need operation updates rely on the shell's `operations` topic (the shell's
  `applyJournal` already invalidates `keys.operation(id)`).

## 7. Ordered implementation steps

1. **Inspect and verify the repository context; report discrepancies.** List
   `crates/domovoi-store/migrations/` and note the next free number. Open every file and symbol in section 3
   and confirm it exists as described. Confirm prompt 26 is merged: `AggregateKind::Operation`, the six
   `operation.*` `JournalKind` strings, `OperationRepo`. Confirm `StateWrite::IncidentEvent` still mirrors
   with `legacy_event_seq` (`uow/writes.rs`, `uow/mod.rs`), that no production code calls
   `IncidentRepo::append_event`, and that `journal` is not pruned anywhere (`grep -n journal
   crates/domovoi-store/src/purge.rs`). Check whether prompt 27 or 29 already merged and added operation
   read endpoints or fields to `ApiDeps`. Check whether prompt 34 added `AggregateKind::{Fact, Relation,
   Entity}` (keep them out of every topic). Run `graphify query "JournalRead" --budget 1500` only as a map.
   If anything differs materially, write it down before coding, and stop if it changes what you would build.
2. **Core.** `JournalKind::IncidentFolded`; `JournalFeed`; `topic.rs` with unit tests (6 below). Update every
   exhaustive `match` the compiler flags. Commit.
3. **Store: migration, floor, head, prune.** Write the migration; extend `Store` with `head`; implement
   `JournalFeed for Store`; change `head_seq`; add `prune.rs`. Update the store tests that assert the journal
   is empty after upgrade (`upgrade.rs` section 4 asserts `journal` count 0: it now equals the number of
   legacy `incident_events` rows; fix it deliberately and say so). Commit.
4. **Testkit.** In-memory `JournalFeed`, counter-based seq, prune helper, `journal_feed` contract suite run for
   `Store` (`crates/domovoi-store/tests/sqlite/contracts.rs`) and `InMemoryStore`. Commit.
5. **Engine.** `Write::Journal`, the fold row, update the manager tests flagged by the compiler. Commit.
6. **Hub, endpoint.** `api/events/` module (params, wire projection, stream, cap), `ApiDeps.journal`, remove
   `ApiDeps.bus`, `UiEvent`, `ui_event`; rewrite the old SSE test; add `api_events.rs`. Keep the shutdown
   test passing. Commit.
7. **Hub, snapshots.** `snapshot.rs`, `as_of_seq` on the four DTOs and handlers; update the hub tests that
   construct or compare those DTOs. Regenerate OpenAPI (`cargo xtask openapi`), accept the changed insta
   snapshot `hub__api_routes__openapi.snap` only after reading it, then `cd web && npm run gen:api`. Commit.
8. **Web.** Fixtures first (`frames.json`, section 12), then `journalFrames.ts`, `journalStream.ts`,
   `events.ts`, keys, tests; remove the `UiEvent` type export. Keep every file at or under 250 lines. Commit.
9. **Parity and N-1 tests** (section 12), then docs (section 15). Commit.
10. **Run the checks and write the completion report** (standard block).

## 8. Database and persistence

- **Migration file:** `crates/domovoi-store/migrations/<next free number>_journal_feed.sql`, contents as in
  section 6.6. Number it by listing the directory on your branch; never assume.
- **Transactions.** The migration runs inside sqlx's migration transaction: the floor table and the copy
  commit together or not at all. `Store::commit` is unchanged except for the post-commit head publish
  (after `COMMIT`, on success only). `prune_journal_through` is one transaction (delete + floor update).
- **Idempotence.** The copy is guarded by `NOT EXISTS (... legacy_event_seq = e.seq)` and the unique index
  `journal_legacy_event`, so rerunning it (or running it on a database whose rows are partly mirrored) never
  duplicates.
- **N-1 upgrade test.** From the committed fixture `tests/fixtures/n1-schema5.db` (schema 5: no journal at all)
  and from a partly mirrored database built in the test (section 12, tests 1-6). Do not add a new fixture
  file for the schema-5 case; reuse it. The fixture must not be regenerated.
- **Size.** The copy duplicates `incident_events` content (timeline rows, including `triage.*` detail) into
  `journal` until the old table is retired; mention it in the PR. No `VACUUM` is needed.

## 9. Protocol and compatibility

- **Agent wire protocol:** unchanged (1.2 frozen, RW-42).
- **Browser API:** `GET /api/v1/events` changes behaviour and frame names. The embedded web UI ships in the
  same binary, so hub and UI change together. An old browser tab opened before the upgrade keeps an
  `EventSource` that sees unknown frame names (`ready`, `journal`, `reset`) and ignores them, so it stops
  refreshing until reloaded; that is accepted and not worked around.
- **OpenAPI:** `stream_events` documents the query parameters and the frame names in its description; the
  `UiEvent` schema is removed; `as_of_seq` is added to four schemas. Additive for every other client.
- **Database:** additive migration; `MIN_READER` unchanged. The previous release binary can open the upgraded
  database (it ignores `journal_floor`, and reads `incident.timeline` rows).
- **Journal kinds:** `incident.folded` is a new string; readers already tolerate unknown kinds
  (`JournalEntry.kind` is a `String`).

## 10. Error handling and recovery

| Situation | Exact behaviour |
|---|---|
| No session | `401` `unauthorized`, as today (`Authed`) |
| Unknown or malformed `topics` entry, more than 16 topics, non-numeric `after` or `Last-Event-ID` | `400` `bad_request`, message names the offending value; no stream |
| Hub already serves `MAX_EVENT_STREAMS` streams | `503` `unavailable`: `too many open event streams` |
| `Last-Event-ID` below `floor_seq()` | first frame `reset{reason:"pruned"}` with `id: head`; stream continues from the head |
| `Last-Event-ID` above `head_seq()` (restored database) | first frame `reset{reason:"ahead"}` |
| Client is more than `LAG_LIMIT_ROWS` behind when polled | `reset{reason:"lagged"}`, cursor jumps to the head |
| Client reads slowly but stays within the limit | every matching row arrives, in order, exactly once; memory stays at one batch |
| Journal read fails while streaming (`StoreError`) | log a warning with the error, end the stream (the client reconnects with its last id and resumes); never emit a partial frame |
| A row's `aggregate_type` is unknown to this build | the store read already fails as `corrupt`; same as the line above. (Later prompts add the variants they write.) |
| A row has no topic | skipped silently; the cursor still advances |
| Payload over 16 KiB | frame sent with `payload: null` and `payload_truncated: true` |
| Hub shutdown | the stream ends (`take_until(state.shutdown.cancelled_owned())`) |
| Client disconnects | the stream is dropped, the stream-cap guard decrements (RAII), `domovoi_sse_streams_open` drops |
| Slow client never reads | its task is parked on the socket; the cap bounds how many such tasks exist |
| Web: `error` event or closed source | close, mark offline, reconnect with backoff, passing `?after=<lastId>` |
| Web: `reset` frame | adopt `head_seq` as the last id, call `refetchLive`, call `onReset` |
| Web: malformed frame | ignored; the next frame or reconnect refreshes |
| Web: frame with `seq <= lastId` | ignored (dedupe) |
| Migration copy meets a non-JSON `detail` | stored as a JSON string of the raw text; the migration never fails on it |
| `prune_journal_through` with a seq above the head | deletes everything up to the head and raises the floor to `seq`; the head stays `max(MAX(seq), floor)` |

## 11. Security requirements

- **Authentication:** unchanged. The `Authed` extractor guards the stream; there is no unauthenticated mode.
  Add `/api/v1/events` with a `topics` query to the existing 401 test list.
- **Read-only.** The stream writes nothing, approves nothing, and exposes no command. INV-1 is untouched:
  `HumanApproval::new` stays only in `crates/domovoi-hub/src/auth/`. Do not add any approval path.
- **No new data exposure.** Frames carry journal payloads that their owners already write without secrets,
  without tool output and with 8-hex plan hashes only (prompt 26 section 6.4; prompt 34 states no fact value
  is journaled). `incident.timeline` payloads are projected so the full triage `detail` is never pushed and
  the summary is redacted. Oversized payloads are dropped. Never log frame payloads.
- **Audit and effects are never streamed.** `topics_of` has no arm for them and the endpoint reads only
  `journal`.
- **Abuse resistance:** at most 32 concurrent streams, at most 16 topics, bounded batch, bounded frame size,
  pull-based reads (no per-client queue). A closed/revoked session is not re-checked mid-stream (same as
  today); note it as a known limitation in the PR, do not widen scope.
- **Fail closed:** unknown topic or cursor syntax is a 400, never "treated as all"; a topic mapping that does
  not exist delivers nothing.
- **No wire/secret changes:** secrets stay `SecretString`; nothing here touches them.
- **Time:** the keepalive uses `Clock`; tests use `FakeClock`.

## 12. Testing requirements

Name tests after behaviour. Use real fakes: `FakeClock` and `InMemoryStore` (testkit, existing); the hub
harness already builds a real `Store` on a temp file with a `FakeClock` (`tests/hub/api_harness.rs:268-302`,
`Api.state`, `Api.admin()`, `Api.get`, `Api.post`, `Api.advance`). Tests never sleep; where a stream test must
await a frame, wrap `body.next()` in `tokio::time::timeout(Duration::from_secs(10), ...)` as the existing
stream test does (an upper bound, not a wait). To write journal rows from a test, commit a `Transaction` with
`JournalDraft`s through `UnitOfWork` on the harness store (read the harness to find the `Arc<Store>` it keeps).
Tests that need an operation aggregate use `AggregateRef::operation(OperationId)` from prompt 26 (verify the
constructor name in code).

### 12.1 Core (`crates/domovoi-core/src/ports/uow/topic.rs`, `journal.rs`, unit tests)

| Test | Setup, action, expected |
|---|---|
| `topic_grammar_accepts_the_documented_names_and_rejects_the_rest` | parse `incidents`, `approvals`, `operations`, `operation:<uuid>`, `hosts`, `conversations:<uuid>` succeed and `as_wire` round-trips; `Incidents`, `operation:`, `operation:xyz`, `incident`, the empty string and `operations:<uuid>` fail with `Unknown` or `BadId` |
| `default_topic_set_is_incidents_approvals_operations` | `parse_list(None)` equals `default_set()`; `wire()` is `["approvals","incidents","operations"]` |
| `topic_set_rejects_more_than_sixteen_and_empty_entries` | 17 distinct topics, `a,,b` and a trailing comma are errors; duplicates collapse |
| `incident_rows_belong_to_incidents` | an `incident.timeline` entry maps to `[Incidents]` |
| `action_rows_belong_to_approvals_and_incidents` | `action.transition` maps to both |
| `operation_rows_belong_to_operations_and_their_own_topic` | `operation.step_finished` maps to `Operations` and `Operation(id)` |
| `operation_execution_rows_also_belong_to_approvals` | `operation.execution` adds `Approvals` |
| `knowledge_and_note_rows_belong_to_no_topic` | a `Note` entry (and `Fact` if the variant exists) maps to `[]` |
| `reserved_topics_match_nothing` | a set of `hosts` and `conversations:<uuid>` matches none of the entries above |
| `journal_kinds_round_trip` (extended) | includes `IncidentFolded` / `"incident.folded"` |

### 12.2 Store (`crates/domovoi-store/tests/sqlite/journal_feed.rs` **(new)**, and `upgrade.rs`)

| Test | Setup, action, expected |
|---|---|
| `copy_fills_unmirrored_incident_events_from_the_schema5_fixture` | copy `n1-schema5.db`, count its `incident_events`, open with `Store::open`; there is one `incident.timeline` journal row per legacy row, `legacy_event_seq` = the legacy `seq`, `occurred_at` = `at`, `aggregate_type` `incident`, `aggregate_id` = `incident_id`, `payload_v` 1, `actor` `system:incidents`, payload = `{kind, summary, detail}` equal (as parsed JSON) to the legacy columns |
| `copy_orders_rows_by_time_then_legacy_seq` | insert legacy rows with out-of-order `at` and equal `at` values before opening; the copied rows' `seq` order equals `(at, legacy seq)` order |
| `copy_skips_rows_the_dual_write_already_mirrored` | build a `Migrator` from `MIGRATOR` filtered to versions below the new migration (`Migrator::with_migrations`, as `db.rs` does), run it on a copy of the fixture through a raw pool, insert two `journal` rows with distinct `seq` and a `legacy_event_seq` naming two legacy rows (as the dual write does), then `Store::open`; the two rows keep their `seq`, no legacy row has two journal rows, and every legacy row has exactly one |
| `copy_keeps_unknown_legacy_kinds_and_invalid_detail` | before opening insert a legacy row with kind `weird.kind` and detail `not json`; the journal row has kind `incident.timeline`, `payload.kind == "weird.kind"`, `payload.detail == "not json"` (a JSON string) and the migration succeeded |
| `reopening_after_the_copy_changes_nothing` | open twice; the journal row count and every `seq` are identical |
| `journal_mirrors_every_incident_event_after_the_copy` | after the copy, commit a `StateWrite::IncidentEvent`; the new row has the mirror shape; for every `incident_events.seq` exactly one `journal.legacy_event_seq` matches; copied and mirrored rows have the same payload key set |
| `journal_feed_migration_is_additive` | `include_str!` the new migration; it contains none of `DROP`, `ALTER`, `DELETE`; `MIN_READER` is still `"0.0.0"` |
| `journal_floor_starts_at_zero_after_the_upgrade` | after opening the fixture, `floor_seq()` is 0 and `head_seq()` equals the highest copied `seq` |
| `upgrade_from_previous_schema_keeps_every_row` (fix) | the old assertion "journal starts empty" (step 4 in the existing test) becomes "journal has one row per legacy event"; keep every other assertion |
| `prune_raises_the_floor_and_the_head_never_moves_backwards` | commit 5 rows, `prune_journal_through(5)`; `floor_seq() == 5`, `head_seq() == 5`, `entries_after(0, 10)` is empty; a later commit gets `seq` 6 |
| `prune_never_lowers_the_floor` | prune through 5 then through 3: the floor stays 5 |
| `commit_publishes_the_head_after_the_commit_only` | with `futures::FutureExt::now_or_never`: `wait_after(head)` is pending; after a successful commit it is ready; a commit that fails (a stale-version conflict) leaves a fresh `wait_after(head)` pending |

**Contract suite** `journal_feed_contract` in `crates/domovoi-testkit/src/contracts/uow/feed.rs`, registered for
`Store` and `InMemoryStore` like the existing `unit_of_work` suite: floor is 0 initially; `wait_after(n)` is ready
when `head > n`; pending when `head == n`; ready after a commit with a journal row; head equals the highest
receipt seq.

### 12.3 Engine (`crates/domovoi-engine/tests/engine/journal_parity.rs` **(new)**, add to `tests/engine/main.rs`)

Use the existing manager harness (`tests/engine/harness.rs`, `World::drain()` returns the published
`DomainEvent`s) over `InMemoryStore`; the action scenarios use `tests/services/actions_harness.rs` (so that file
may be the better home; place the test where the needed harness exists, and say so in the report).

| Test | Setup, action, expected |
|---|---|
| `every_v1_event_has_a_journal_equivalent` | table-driven, one scenario per row of section 3.5 that can be produced: open, fold, escalate, reopen, resolve, ack, mute, unmute, triage completed (`attach_triage`), and via the action service `action_changed` and `action_blocked` (a loop-guard rejection). For each scenario record `head = journal.head_seq()`, run the command, drain the published events, then assert that `entries_after(head)` is non-empty and that for **every** published event that the old stream would have sent (the table), a journal row names the same incident: `aggregate == incident:<id>`, or (action rows) `payload.incident == <id>`. The expected journal kinds per event come from the section 3.5 table, written inline in the test |
| `fold_writes_an_incident_folded_row_in_the_same_commit` | two firing signals with the same fingerprint; after the second, the journal has one `incident.folded` row with `payload == {"signal": <id>, "occurrences": <count>}`, aggregate `incident:<id>`, actor `system:incidents`; the built `Transaction` carries it in `tx.journal` |
| `a_signal_that_does_not_fold_writes_no_folded_row` | a resolving signal and a signal for an unknown group write none |

### 12.4 Hub (`crates/domovoi-hub/tests/hub/api_events.rs` **(new)**; add `mod api_events;` to `tests/hub/main.rs`)

| Test | Setup, action, expected |
|---|---|
| `sse_replays_missed_events` | commit 3 incident rows (seq `a`, `b`, `c`); connect with `Last-Event-ID: a`; frames are `ready{resumed:true}` (no `id`), `journal` id `b`, `journal` id `c`, in order; none for `a` |
| `after_parameter_resumes_like_last_event_id_and_the_header_wins` | `?after=a` gives the same frames; both given with different values, the header value is used |
| `ready_without_a_cursor_carries_the_head_and_starts_the_live_tail` | connect with no cursor after committing rows; first frame `ready{resumed:false, head_seq: head}` with `id: head`; earlier rows are **not** sent; a new commit arrives as `journal` with the next `seq` |
| `sse_resets_beyond_retention` | commit 5 rows, `prune_journal_through(3)`; connect with `Last-Event-ID: 1`; first frame `reset{reason:"pruned", head_seq: 5}` with `id: 5`, no `ready`; a later commit streams normally; connecting with `Last-Event-ID: 3` replays 4 and 5 |
| `sse_resets_when_the_cursor_is_ahead_of_the_head` | `Last-Event-ID: head + 100` gives `reset{reason:"ahead"}` |
| `lagged_subscriber_loses_nothing` | connect with no cursor, do not poll the body, commit 300 rows in several transactions, then poll: all 300 `journal` frames arrive in order, exactly once, no `reset` |
| `sse_resets_a_client_that_falls_too_far_behind` | connect, commit `LAG_LIMIT_ROWS + 1` rows in one transaction, poll: the next frame is `reset{reason:"lagged"}` whose `head_seq` is the head; a row committed afterwards streams |
| `sse_filters_by_topic` | commit one incident row, one action row, one operation row (operation A), one operation row (operation B), one `operation.execution` row for A, one note row. `incidents` receives the incident and action rows; `approvals` receives the action and the execution row; `operations` receives the three operation rows; `operation:<A>` receives only A's two rows; the note row is never sent; the cursor still advances past skipped rows |
| `journal_frames_match_the_committed_fixture` | build the scenario entries, render them through the production frame function and compare with `web/src/test/fixtures/journal-frames/frames.json` (shape in 12.6); with `JOURNAL_FRAMES_UPDATE=1` rewrite it; read every file you generate before committing |
| `incident_timeline_frames_drop_detail_and_redact_the_summary` | a timeline row whose detail is a large triage outcome and whose summary contains a registered secret: the frame payload is exactly `{"kind", "summary"}` and the secret is replaced |
| `oversized_payload_is_dropped_with_a_flag` | an `operation.step_finished` row with a 20 KiB payload: `payload: null`, `payload_truncated: true`; the frame is still delivered |
| `keepalive_is_sent_after_fifteen_seconds_of_clock_time` | connect, poll once to start the stream, `a.advance(15 s)`: the next chunk is `: keepalive`; after a committed row, advancing 14 s stays silent and 1 s more gives a keepalive |
| `invalid_topic_is_a_bad_request` | `topics=incidents,bogus`, `topics=operation:not-a-uuid`, 17 topics, `after=abc`, `Last-Event-ID: x` each answer 400 `bad_request` |
| `too_many_streams_are_refused_and_the_slot_frees_on_disconnect` | open `MAX_EVENT_STREAMS` streams; the next gets 503 `unavailable`; drop one body; a new connection succeeds |
| `stream_requires_a_session` | add `/api/v1/events?topics=incidents` to the 401 list in `api_auth.rs` |
| `snapshots_carry_as_of_seq` | for `/api/v1/incidents`, `/api/v1/incidents/{id}`, `/api/v1/actions`, `/api/v1/overview`: `as_of_seq` equals `head_seq()` at request time; after committing a journal row it is larger |
| `sse_stream_emits_incident_update` (rewrite, `api_routes.rs:290`) | ack the fixture incident through the API; the stream delivers a `journal` frame with `aggregate_type "incident"`, kind `incident.timeline`, payload kind `human.action`, `aggregate_id` = the incident id |
| `event_stream_ends_when_the_server_shuts_down` | unchanged; must still pass |

### 12.5 Web (`web/src/api/journalStream.test.ts`, `journalFrames.test.ts`, `events.test.ts`)

Reuse the `FakeEventSource` pattern from the existing `events.test.ts`; add recording of the constructed URL.

| Test | Expected |
|---|---|
| `topics_are_sent_comma_joined` | the URL is `/api/v1/events?topics=incidents,approvals,operations` for `useLiveEvents` |
| `a_reconnect_resumes_from_the_last_seq` | emit `journal` ids 5 and 6, then `error`; after the backoff timer (fake timers) the new source URL contains `after=6` |
| `frames_at_or_below_the_last_seq_are_ignored` | emit 7, 7, 6: `onFrame` is called once |
| `reset_adopts_the_head_and_refetches_the_live_queries` | emit `reset{head_seq: 40}`: `invalidateQueries` is called for incidents, actions, overview and operations; the next reconnect URL has `after=40`; `onReset` ran |
| `a_fresh_ready_refetches_only_snapshots_older_than_the_head` | seed the query cache with one incident list at `as_of_seq` 10 and one at 50; emit `ready{resumed:false, head_seq: 40}`: only the first is invalidated |
| `a_resumed_ready_refetches_nothing` | `ready{resumed:true}` causes no invalidation |
| `a_snapshot_newer_than_the_event_is_not_refetched` | cached `as_of_seq` 12 with frame `seq` 12 or 9: not invalidated; frame 13: invalidated |
| `journal_frames_cover_every_v1_event` | for each v1 event of section 3.5, the matching fixture frame invalidates at least the keys the old `applyEvent` invalidated (incident lists, incident detail, overview, and `actions` for action events) |
| `unknown_aggregate_types_and_kinds_are_ignored` | a frame with aggregate_type `fact` and an unknown kind causes no error and no invalidation |
| `fixture_frames_parse` | every `journal` fixture entry passes `parseJournalFrame`; a malformed string returns `undefined` |
| existing `retryDelay`, pause-timer and "Back online" tests | keep passing (update only what the new URL and frame names require) |

### 12.6 Shared fixture shape (**fixed**)

`web/src/test/fixtures/journal-frames/frames.json`:

```json
{
  "schema_version": 1,
  "frames": [
    { "name": "incident_acked", "event": "journal", "id": 12,
      "data": { "seq": 12, "aggregate_type": "incident", "aggregate_id": "...", "kind": "incident.timeline",
                "payload_v": 1, "payload": { "kind": "human.action", "summary": "user:..." },
                "actor": "system:incidents", "at": "2026-11-18T10:00:00Z", "topics": ["incidents"] } },
    { "name": "incident_folded", "event": "journal", "id": 13, "data": { "...": "kind incident.folded" } },
    { "name": "action_changed", "event": "journal", "id": 14, "data": { "...": "kind action.transition, topics approvals+incidents" } },
    { "name": "operation_step_finished", "event": "journal", "id": 15, "data": { "...": "topics operations + operation:<id>" } },
    { "name": "ready_fresh", "event": "ready", "id": 15, "data": { "head_seq": 15, "resumed": false, "topics": ["approvals","incidents","operations"] } },
    { "name": "reset_pruned", "event": "reset", "id": 15, "data": { "reason": "pruned", "head_seq": 15 } }
  ]
}
```

Fixed ids and times; the Rust test renders the same entries and compares. Include one fixture frame per row of the
section 3.5 table so the web parity test can pick them by name (`incident_opened`, `incident_folded`,
`incident_escalated`, `incident_reopened`, `incident_resolved`, `incident_acked`, `incident_muted`,
`incident_unmuted`, `triage_completed`, `action_changed`, `action_blocked`).

## 13. Acceptance criteria

1. `GET /api/v1/events` with a session returns `200 text/event-stream`; without one, `401`.
2. Every `journal` frame has `id: <seq>` equal to the journal row's `seq`; frames of one connection are in
   strictly increasing `seq`.
3. With `Last-Event-ID: n` (or `?after=n`; the header wins) a client receives exactly the matching rows with
   `seq > n`, in order, once each, then the live tail (`sse_replays_missed_events`).
4. A cursor below the floor, above the head, or more than 5,000 rows behind yields a `reset` frame with
   `id: <head>` and reason `pruned`, `ahead` or `lagged`, never a silent gap.
5. A client that falls behind by less than the limit receives every row (`lagged_subscriber_loses_nothing`);
   the server holds no per-client queue and no `broadcast` channel feeds the stream.
6. `topics=` filters as in the section 6.2 table; unknown topics are `400`; `hosts` and `conversations:<id>`
   are accepted and deliver nothing; knowledge and note rows are in no topic.
7. The keepalive is `: keepalive` after 15 s of `Clock` silence and is tested with `FakeClock`.
8. `ApiDeps.bus` and `UiEvent` no longer exist; `ApiDeps.journal` does; the three constructors compile.
9. `IncidentPage`, `IncidentDetail`, `ActionPage` and `OverviewDto` carry `as_of_seq`, read before the data;
   OpenAPI and `schema.d.ts` are regenerated, not hand-edited.
10. Every `EventKind` the old stream sent has a journal row naming the same incident in the same commit
    (`every_v1_event_has_a_journal_equivalent`); a fold writes `incident.folded` in the same transaction.
11. The migration copies exactly the unmirrored `incident_events` rows, one `incident.timeline` journal row each,
    original timestamp, `(at, seq)` order, mirror-identical shape, no invented rows, idempotent; unknown legacy
    kinds survive in `payload.kind`; `incident_events` is untouched and still read and purged as before.
12. `journal_floor` exists with one row; `head_seq` never moves backwards after pruning; `Store::commit`
    publishes the head only after a successful `COMMIT`.
13. The migration is additive, `MIN_READER` is unchanged, and the schema-5 fixture upgrades with every
    pre-existing row intact (`upgrade_from_previous_schema_keeps_every_row` updated deliberately).
14. `useJournalStream` resumes by `after=<lastSeq>`, dedupes by `seq`, handles `reset` and a fresh `ready` by
    refetching only what is older, and `useLiveEvents` keeps its external behaviour (`Live`, backoff, `paused`)
    for `Layout`.
15. No visible UI change; no file in `web/src` exceeds 250 lines; `npm run lint`, `npm run typecheck` and the
    vitest files of `web/src/api` and `web/src/app` pass.
16. `incident.timeline` frames never contain the triage `detail`; payloads over 16 KiB are dropped with
    `payload_truncated`.
17. No new dependency; `cargo xtask arch-check` passes.
18. `sse_replays_missed_events`, `sse_resets_beyond_retention` and `lagged_subscriber_loses_nothing` exist under
    exactly those names.

## 14. Migration and compatibility

- **Existing installs.** On first start of the new release the migration creates `journal_floor` and copies
  every legacy timeline row that has no mirror. The old `incident_events` table keeps being written (dual
  write) and purged (90 days) exactly as before, so the previous release can run unchanged against the
  upgraded database. A later release retires `incident_events` after this one has shipped (`05` section 2);
  that is a separate change and you must not start it.
- **Seq ordering after the copy.** Copied rows have higher `seq` than the earlier mirrored rows although they
  are older in time. Any consumer that lists a timeline from the journal orders by `(occurred_at, seq)`.
  Nothing reads timelines from the journal today; a change that starts to must follow this rule.
- **Streams across the upgrade.** The old stream had no ids, so no client holds a cursor from before the
  upgrade; a tab left open across the upgrade stops refreshing until reloaded (section 9).
- **Retention interplay.** The journal is not pruned in this release, so old timeline rows live longer in
  `journal` than in `incident_events` (which still purges at 90 days). That is intentional, temporary and
  deletes no data. Prompt 57's keys are unchanged.
- **Agents and the wire:** no change; agents need no upgrade.
- **Config:** none added; no `docs-config` regeneration.
- **Downstream.** Prompt 30 (operations timeline), 36 (conversations; adds the `conversations:<id>` mapping
  and the `live.*` frames) and 51 (control room, attention centre) consume the contract in section 6. Prompt 27
  adds `as_of_seq` to its operation endpoints with `snapshot_seq`. Prompt 34's journal kinds stay in no topic
  until a consumer needs them; adding one is a new arm in `topics_of`.

## 15. Documentation requirements

- `docs/book/src/reference/api.md`, section "Events" (it currently describes `incident` and `action` events and
  a 15 s comment): rewrite to describe `GET /api/v1/events`, `topics`, `after` / `Last-Event-ID`, the frames
  `ready`, `journal` (with the example above), `reset` (reasons, and what a client does), the keepalive,
  `as_of_seq` on snapshot reads and the 32-stream cap. Plain, short, specific.
- `docs/book/src/concepts/how-it-works.md`, section "Restart and missed events": add two sentences saying the
  browser stream resumes from the last event it saw and refetches when the hub tells it to.
- Run `cargo xtask openapi` then `cd web && npm run gen:api` (the API changed), and `cargo xtask docs-links`
  (book pages changed). No `cargo xtask docs-config` (no config change). Add a line to `web/DESIGN.md` only if
  a guardrail forces it (none expected).
- `ARCHITECTURE.md`: change only if a sentence there names the old event stream or `UiEvent`.
- Commit messages, the PR description and every committed file must not mention prompts, prompt numbers, the
  build plan or coding agents: write "coming in a later release", never a prompt number. (Numbers in this
  brief are for you only.)


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
