# 35 — Resolution summaries, experience search and similar operations

| | |
|---|---|
| Branch | `experience-search` |
| Milestone | M5 |
| Depends on (must be merged) | 26, 27, 29, 31, 32, 34 (and what they need: 22, 23, 24, 25, 28). Prompts 24, 25, 26, 27, 28, 29, 31, 32 and 34 are written but may not be merged when this brief is read: **every name taken from them below is "introduced by prompt NN — verify in code before use"**. 32 is listed as a dependency because it introduces `TriageDeps.extra_context`, the one place a context source is registered, and says that 34 and 35 merge after it. If one is missing, stop and report |
| Can run in parallel with | 37, 42, 33 (order-flexible, never concurrent). 35 adds a migration, so it runs after any earlier migration-adding prompt (26, 28, 34, 42, 59) has merged and takes the next free number |
| Produces (shared contracts) | Core: `domovoi_core::experience::*` (`SummaryField`, `SummarySource`, `SummaryRevision`, `ResolutionSummary`, `ResolutionOutcome`, `ExecutedContract`, `ResolutionBrief`, `ExperienceScope`, `ExperienceQuery`, `ExperienceHit`, `SimilarQuery`, `SimilarHit`, `SimilarReason`, `code_fields`, `HISTORICAL_LABEL`), the port `ExperienceRead`, `OperationWrite::SummaryField`, `EffectKind::DraftResolution`, `JournalKind::OperationSummary`, `MetaTool::SearchExperience` (the seventh meta-tool) and `MetaToolCall::SearchExperience`. Engine: `domovoi_engine::context::ExperienceSource` (a `ContextSource`), the `DraftResolution` handler, `OperationHandle::edit_summary_field`. Store: tables `resolution_summary_fields`, `operation_resolutions`, `operation_resolutions_fts`, `operation_resolution_hosts`, `operation_resolution_subjects`. Hub: `GET /api/v1/operations/{id}/summary`, `POST /api/v1/operations/{id}/summary/{field}`, `GET /api/v1/operations/{id}/similar`, `GET /api/v1/incidents/{id}/similar-operations`, `GET /api/v1/experience/search`; DTOs `ResolutionSummaryDto`, `SummaryFieldDto`, `SummaryRevisionDto`, `SimilarOperationDto`, `SimilarOperationsDto`, `ExperienceSearchDto`, `ExperienceItemDto`, `EditSummaryFieldRequest`, `ResolutionBriefDto`; `OperationCommands::edit_summary_field` |
| Consumes | 26: `Operation`, `OperationSnapshot`, `StepRecord`, `StepKind`, `StepOutcome`, `StepIntent`, `PinnedSubject`, `OperationWrite`, `OperationRepo`, the writer, `decide`, `OperationHandle`, `EffectCancel`, the aggregate-scoped effect runner, audit kind constants. 27: `TurnOutput`, the `DraftSummary` handler (its meaning of that kind), `OperationSummaryDto`, `operation_error`. 28: `snapshot_seq`, `applyJournal`, query keys. 29: `Services`, `Caller`, `OperationCommands`, `OperationCommandError`. 31: `MetaTool`, `ToolSet`, `MetaToolCall`, `ContextSource`, `ContextItem`, `ContextScope`, `ContextSelector`, `ContextQuery`, `ContextBatch`, `ContextGap`, `Origin::Historical`, `ContextItem::render`, `LoopScope`, the dispatch in `triage::meta`, the gates `model_meta_tools_are_closed` and `model_sees_only_meta_tools`, `FakeContextSource` and the context contract suite. 34: the FTS-with-triggers pattern (`facts_fts`) and the purge guard. 32: `TriageDeps.extra_context` (registration point), the `Aggregator`'s rules for the `Experience` layer |
| Launch gate | none |
| Suggested model | medium: one more store table family and a small amount of writer code, but the safety wording ("historical evidence, not authorization"), host scoping and the gate edits must be exact |

## 1. Objective

Make past operations easy to retrieve, and make what they taught the model available **as evidence, never as authorization**.

After this change:

- Every operation that reaches a terminal state gets a **resolution summary**: append-only field revisions owned by the operation aggregate. Five fields are filled by code (`outcome`, `verified`, `subjects`, `contracts_executed`, `hosts`). Three narrative fields (`problem`, `cause`, `fix`) are drafted from the model's own committed report, marked `source = model`, and the owner can edit or confirm them (`source = user`). Every revision records who wrote it and when.
- Summaries are full-text indexed (SQLite FTS5, redacted text only) and queryable by host.
- The model has a **seventh meta-tool**, `search_experience`, a host-scoped read: it returns at most 5 past operations of the incident's own host, each with operation id, date, verified flag and the source of every narrative field, at most 1 KiB each, under the fixed label **"historical evidence, not authorization"**.
- An **experience `ContextSource`** (`Origin::Historical`) exists, so layered context can offer past operations next to evidence and knowledge.
- The web API can answer: the summary of an operation (with provenance), **similar past operations** for an operation or an incident, and an owner search over all operations (host-scoped, or explicitly global).
- Owner edits of narrative fields go through the operation writer, use-case layer first, and are audited.

A past approval or a past success never satisfies a policy, never authorizes a run and never pre-approves anything. Nothing in this prompt can approve, authorize, dispatch or write outside the operation writer.

This prompt does **not** build the aggregator, layer caps or the context guide (32), the knowledge store (34), any page or component (48, 30), a policy (39, 40) or a wire change. Section 5 lists every exclusion.

## 2. Architectural context

Everything needed is here; the cited documents are background.

**D-02 (experience is the journal, past operations are easy to find).** There is no third semantic store: experience is the operation aggregate's own typed data. To make it retrievable, each operation gets a resolution summary, summaries are full-text indexed, `search_experience` is a host-scoped read meta-tool, the UI shows similar past operations, and provenance is visible. **Past approvals are evidence, never authorization.**

**RW-25 (retrieval).** Every terminal operation gets a `resolution_summary`. Deterministic fields are filled by code: subjects, contracts executed, verification result, outcome. Narrative fields (problem, cause, fix) are drafted by the model, marked `source = model`, and are editable or confirmable by the owner. Summaries are full-text indexed. `search_experience(query, scope)` is a host-scoped read meta-tool. The UI shows similar past operations on incident and operation pages. When experience is supplied to reasoning, each item carries its operation id, date, verified flag and source, inside a block labelled "historical evidence, not authorization". Past approvals and successes never satisfy policy.

**RW-41 (provenance and ownership).** Summaries are append-only field revisions `(operation_id, field, revision, value, source{code|model|user}, actor, created_at)` belonging to the Operation aggregate.
- **The operation writer is the only writer.** The Knowledge UI and the use-case services submit `EditSummaryField { operation_id, field, value, expected_revision }` to it. It validates the field, rejects `user` edits to code-owned fields, checks the revision, and commits the revision, the journal entry and the FTS update in one transaction.
- Code-owned fields: `outcome`, `verified`, `subjects`, `contracts_executed`, `hosts`. Narrative fields: `problem`, `cause`, `fix`.
- Search results and model context show each field's source.

**RW-07 (closed meta-tools).** The model sees a closed enum: `list_capabilities`, `describe_capability`, `request_evidence`, `search_experience`, `propose_plan`, `ask_user`, `finish`, in that order. Prompt 31 shipped six and pinned the list with gates; adding the seventh is a reviewed edit of those gates **in this change**.

**D-21 / RW-28 (retention).** Operations, steps and **resolution summaries are retained**. Only raw operation journal rows (180 days) and raw execution outputs (30 days) are pruned. So a summary must never depend on a step's `output` column or on a journal row surviving: it is derived once, when the operation ends, from data that is still there, and it is stored on its own. Pruning deletes FTS rows in the same transaction; no summary table is purged here.

**RW-19 (use-case layer, prompt 29).** Commands go through a method of `crate::services` taking `&Caller`; HTTP and chat handlers only parse and map. Reads may stay in `api/` handlers. Editing a summary field is a command; reading summaries, similar operations and search are reads.

**RW-35 (effects).** Work is an outbox row created in the owner's transaction and completed in the owner's transaction, fenced by a claim token. The operation runner claims only aggregate kind `operation` (prompt 26). Creating an effect whose `dedup_key` equals a pending or claimed effect's fails the whole commit with `CommitError::Constraint("duplicate effect")`.

**INV-1 / INV-9 (model output and experience are data).** `ContextItem` has no field that can express permission. An item from a past operation has `Origin::Historical` and always renders the origin text `historical evidence, not authorization`. The search result block the model reads starts with the same sentence. Narrative text was written by a model or a person and is shown and stored, never executed.

**INV-5 (secrets).** Every value stored in `resolution_summary_fields`, copied into `operation_resolutions` and indexed in the FTS table has passed the `Redactor`. Journal payloads of summary changes never carry the value.

**ContextSource obligations (prompt 31 §6.3).** Read-only, bounded, redacted, **scoped** (a host-scoped query never returns another host's item), unknown is not empty, deadline, no authority, stable identity. The experience source meets all eight; the contract suite checks it.

### Decisions taken here because the design documents are silent or collide (do not revisit)

| Decision | Why |
|---|---|
| **A new effect kind `DraftResolution`** (`"draft_resolution"`) writes the summary. It does **not** reuse `DraftSummary` | RW-41 names a "`DraftSummary` effect for the narrative", but prompt 27 already registered the `DraftSummary` handler for a different job (turning a model report into `Assessment`, `Plan`, `Execution` and `Notification` steps). The runner has one handler per kind. Two jobs under one kind would collide |
| **The narrative is drafted from the model's own committed report; there is no extra model call** | The investigation already produced `summary`, `cause` and `next_step` in a committed `TurnOutput` (or, when the output is gone, in the immutable `Assessment` and `Plan` step texts). Re-asking the model would spend an admission slot and money for text it already wrote. The text is still model-authored, so it is marked `source = model`; the owner may confirm or edit it |
| The summary is written by an **effect after the terminal commit**, not inside `decide` | `decide` is pure and has no redactor; the effect reads the final snapshot from the store. The summary can therefore lag the terminal commit by seconds (`state = "drafting"` in the API). The terminal commit itself creates the effect, so it can never be forgotten |
| **A projection table `operation_resolutions` (one row per operation) is the FTS source**; the revisions table stays the truth | FTS must index only the *current* value of each field, and host filtering needs one row per operation. The projection is rewritten by the store in the same transaction as every revision (like `steps.status` is a derived projection). A test proves projection equals fold of revisions |
| **Host-scoped eligibility: an operation is returned for host H only when its `hosts` set is exactly `{H}`** | Fail closed. Today every operation has one pinned host. A future multi-host operation would put host B's subjects and narrative into a host-A answer; it is therefore retrievable only by an explicit global owner search |
| **`ExperienceScope::Global` exists only in the read port and only two production callers may use it:** the owner's search endpoint and the Operator conversation whose scope the owner chose as global (prompt 36). It is **not** a `ContextScope`; the `ContextSource` implementation cannot answer globally; `search_experience` has no model-controlled scope argument | "Never return cross-host results unless the search is explicitly global and initiated by the owner." A gate scan pins the callers |
| `search_experience` takes only `query`. The RW-25 `scope` is carried by the loop (`LoopScope`), never by model input | A model must not be able to widen its own scope |
| Matching is two-pass: all words first, then any word, results of pass 1 rank above pass 2 | Model-written queries are sentences; an all-words match alone returns nothing for most of them. Words are always quoted (no FTS operator can come from user text) |
| "FTS over summaries + journal kinds" (04 §10) is implemented as **`tags`**: a small fixed vocabulary of outcome words derived from the typed steps (`denied`, `expired`, `verification_failed`, `retried`, `outcome_unknown`, `question_asked`, `escalated`, `cancelled`, `refused`) added to the indexed text | Raw journal rows are pruned at 180 days and are not a stable search base; the typed step outcomes are retained with the operation and say the same thing |
| Confirming a model draft is `EditSummaryField` with the **same value**; it appends a `source = user` revision. Submitting the same value over a `user` revision is a no-op (`Unchanged`, no revision) | One command, no second verb; append-only history stays free of duplicates |
| A user edit needs a terminal operation, a narrative field, a value of 1 to 1000 characters (clearing is not supported), and `expected_revision` equal to the field's newest revision (0 when the field has none) | RW-41 revision check; a stale edit is rejected |
| `edit_summary_field` returns its own error type, `EditSummaryError { Summary(SummaryError), Operation(OperationError) }` | Adding a variant to `OperationError` would break the exhaustive matches that prompts 27, 29 and 59 wrote |
| A late result on a terminal operation (an execution that settles after a cancel) queues one more `DraftResolution`, which appends new **code** revisions only if a code field changed | The summary must tell the truth about a run that finished after the operation closed. Narrative is never redrafted |
| `OperationSummaryDto` gains one nullable `resolution` field | A list row can show outcome and verified without a second request |

## 3. Repository context

Verify each item first. Marks: **(existing, path:line)** or **(new)**. Line numbers are those of the committed tree when this brief was written; prompts 26 to 34 will have moved some.

### 3.1 Existing code this prompt builds on (all verified)

| What | Where |
|---|---|
| `EffectKind` (11 kinds), `as_str`, `parse` (snake_case strings validated in Rust, no SQL `CHECK`) | **(existing, `crates/domovoi-core/src/ports/uow/effects.rs:19-80`)**; the test `effect_kinds_round_trip` lists every kind **(`:264`)** |
| `JournalKind`, `as_str`, `parse`; test `journal_kinds_round_trip` | **(existing, `crates/domovoi-core/src/ports/uow/journal.rs:15-45`, test `:172`)**; prompt 26 adds the six `operation.*` kinds |
| `EffectDraft { id, kind, aggregate, payload, not_before, max_attempts, dedup_key, created_at }` | **(existing, `effects.rs:176-195`)** |
| A second pending or claimed effect with the same `dedup_key` fails the **whole** commit with `CommitError::Constraint("duplicate effect")` | **(existing, `crates/domovoi-store/src/uow/effects.rs:32-45`; the partial unique index is in `migrations/0006_unit_of_work.sql:47`)** |
| FTS query building: `words`, `match_all` (every word quoted and ANDed), `match_all_in` | **(existing, `crates/domovoi-store/src/fts.rs:9-30`)** |
| The external-content FTS pattern: virtual table, `content = 'notes'`, `content_rowid = 'rowid'`, `tokenize = 'unicode61 remove_diacritics 0'`, three triggers | **(existing, `crates/domovoi-store/migrations/0001_init.sql:198-216` for notes, `:56-74` for incidents)**; prompt 34's `facts_fts` follows the same pattern |
| Ranked search: `-bm25(notes_fts, 10.0, 1.0, 1.0) AS score`, `snippet(..)`, host filter, candidate cap `MAX_CANDIDATES = 500` | **(existing, `crates/domovoi-store/src/repos/notes.rs:24, 185-215`)** |
| The full-text guard: every virtual table's stored SQL must contain `content = '<table>'`, and a purged table with an index needs `FTS_CLEANUP` | **(existing, `crates/domovoi-store/src/purge.rs:14-28`; test `purged_tables_with_a_full_text_index_clean_it_in_the_same_transaction` in `crates/domovoi-store/tests/sqlite/purge.rs` near line 425)**. The new tables are **not** purged, so neither list changes, but the new FTS table must keep the exact `content = '...'` text |
| N-1 upgrade pattern: copy `tests/fixtures/n1-schema5.db`, open with `Store::open`, assert | **(existing, `crates/domovoi-store/tests/sqlite/upgrade.rs:1-60`)**; `Fresh` helper in `tests/sqlite/common.rs`; migration-list helper `applied` in `tests/sqlite/migrations.rs:9` |
| `Redactor::redact(&self, &str) -> String`, idempotent | **(existing, `crates/domovoi-core/src/ports/redact.rs:5`)**; testkit `ListRedactor::new(secrets)` **(`crates/domovoi-testkit/src/redact.rs:12`)** |
| `TriageReport { headline, summary, cause, evidence, verdict, next_step, manual_steps, proposals, details }` and its limits | **(existing, `crates/domovoi-core/src/triage/report.rs:10-74`)** |
| `TriageDeps` (hosts, incidents, redactor, clock, ...), constructed in five places | **(existing, `crates/domovoi-engine/src/triage/mod.rs:132-171`)**; the constructors are listed in prompt 31's section 3.1: `crates/domovoi-hub/src/runtime/triage_build.rs`, `crates/domovoi-hub/tests/hub/api_harness.rs`, `crates/domovoi-engine/tests/drills/world.rs`, `crates/domovoi-engine/tests/services/harness.rs` (twice). Re-run `grep -rn "TriageDeps {" crates` |
| `tools::Scope { host, incident }` and `Scope::of(&Incident)` | **(existing, `crates/domovoi-engine/src/triage/tools.rs:12-29`)** |
| The existing "similar" part of the model's context: resolved **incidents** on the same host and check, via `IncidentRepo::search` (title FTS) | **(existing, `crates/domovoi-engine/src/triage/context.rs:170-205`)**. It stays unchanged |
| The pinned gates: `read_tool_catalogue_is_read_only` (core), `model_sees_only_read_tools` and `model_calling_approve_or_unknown_tool_changes_nothing` (engine) | **(existing, `crates/domovoi-core/tests/gates.rs:29`, `crates/domovoi-engine/tests/gates.rs:311, 335`)**; prompt 31 adds `model_meta_tools_are_closed` (core and engine) and renames the engine one to `model_sees_only_meta_tools` |
| `ApiDeps` (read ports, `clock`, `redactor`, `hosts`, `incidents`) and `routes()` | **(existing, `crates/domovoi-hub/src/api/mod.rs:135-196, 305-365)`** before 27, 28 and 29 moved them |
| Contract suites run against `InMemoryStore` through the `contract_tests!` macro | **(existing, `crates/domovoi-testkit/tests/contracts.rs:1-40`; suites in `crates/domovoi-testkit/src/contracts/`)** |
| Book pages: retention table | **(existing, `docs/book/src/reference/data-model.md:68`)**; concepts index `docs/book/src/SUMMARY.md` |

### 3.2 What earlier prompts provide (introduced by prompts 24 to 34: verify every name in code before use)

- **26:** `OperationId`, `StepId`, `Operation { id, kind, goal, status, cycle, budgets, pinned, parent, created_at, started_at, ended_at, updated_at }`, `PinnedSubject { host, check, target }`, `OperationStatus` (`is_terminal`), `Step`/`StepRecord`/`StepKind`/`StepOutcome`/`StepIntent`, `StepEdge`/`EdgeKind`, `ExecutionRecord`, `OperationSnapshot { operation, version, steps, edges, executions, incidents }`, `OperationRepo` (`get`, `load`, `list`, ...), `OperationWrite` (the single `StateWrite::Operation` payload), `Transaction::cancel_effects`, the writer (`writer/mod.rs`, `writer/commit.rs`) with crate-private commands and `OperationCommand`, `OperationHandle`, `OperationError`, the pure `decide(state, event, now, ids) -> Decision` (terminal transitions: `Finish`, `Cancel`, `Verified { passed: true }`, the cycle-limit escalation), `decide/status.rs`, audit kind constants in `operations/audit.rs`, journal kinds `operation.*`, `Actor::System("operations")` (`SYSTEM_OPERATIONS`), `AggregateKind::Operation`, the aggregate-scoped `EffectRunner::scoped`, the contract suite `crates/domovoi-testkit/src/contracts/operations.rs`, the in-memory twin (`crates/domovoi-testkit/src/repos/operations.rs`, `uow_operations.rs`), the SQLite contract run (`crates/domovoi-store/tests/sqlite/contracts.rs`), the engine harness `crates/domovoi-engine/tests/operations/harness.rs`.
- **27:** `TurnOutput { v, kind: "report"|"cached"|"skipped"|"failed", from, report, reason, retry_at }` stored as the turn step's `output` (31 adds `"asked"`, `question`, `plan`, `dropped`), the handlers `BuildContext`, `CallModel`, `DraftSummary`, `Notify` under `crates/domovoi-engine/src/operations/investigate/`, `OperationSummaryDto`, `OperationPage`, `OperationDetailDto` and `operation_error` in `crates/domovoi-hub/src/api/{operations,dto_operations}.rs`, the web hooks pattern.
- **28:** `snapshot_seq(&ApiState)` in `crates/domovoi-hub/src/api/snapshot.rs` (call it **before** loading what it describes), `applyJournal` and the query keys in `web/src/api/`. `topics_of` sends **every** journal row of an `Operation` aggregate to topics `operations` and `operation:<id>`, so a new operation journal kind needs no change there.
- **29:** `Services`, `Caller`, `OperationCommands` and `OperationCommandError { NotEnabled, Invalid(String), TypedConfirmationRequired, WebOnly, Conflict(String), NotFound, Operation(OperationError) }` in `crates/domovoi-hub/src/services/operations.rs`; the module-edge arch rules `MOD-1` to `MOD-4`.
- **31:** `MetaTool` (six variants, `ALL`, `READ` = the first three, `name`, `description`, `from_name`, `json_schema`), `ToolSet::{Read, Investigate { questions }}`, `MetaToolCall`, `MetaArgError`, `impl From<MetaTool> for ToolSpec`, `ContextSource` and its data types, `LoopScope { tools, plan, evidence_left, questions_asked }`, `triage::meta::dispatch`, `EvidenceSource` (`crates/domovoi-engine/src/context/evidence.rs`), `FakeContextSource` and `domovoi_testkit::contracts::context::run(source, scope)`, `META_SYSTEM_PROMPT` (`crates/domovoi-engine/prompts/triage_meta_system.md`), the constants `MAX_RESULT_BYTES`, `UNKNOWN_NOTE`.
- **32:** `TriageDeps.extra_context: Vec<Arc<dyn ContextSource>>` (every purpose asks every source in it after the built-in ones), `Aggregator` (asks `Overview` and, for an incident scope, `Search { text }` with the incident's check and target words; forces `Origin::Historical` on every item of the `Experience` layer; heading `Historical evidence, not authorization: experience`; layer cap 4 KiB; `overview_limit` 10, `search_limit` 5, `item_bytes` 4096; redacts then cuts; dedupes by item id; a source with no item and no gap renders `None.`), the built-in `similar_incidents` source (past **incidents**, `Experience` layer; it stays), `CONTEXT_GUIDE` (already explains the `historical evidence, not authorization` tag to the model).
- **34:** the `facts_fts` trigger pattern, `FactRecordId` (never confuse it with this prompt's `SummaryRevision`), `retracted_facts_days`.

### 3.3 How the experience source reaches the model's context (prompt 32)

Prompt 32 owns the aggregator and says that 34 and 35 each add **one** source and register it by pushing it onto `TriageDeps.extra_context` where the hub builds `TriageDeps` (`crates/domovoi-hub/src/runtime/triage_build.rs`); they edit no aggregator, cap, layout or rendering code. This prompt does exactly that (step 12) and nothing more: it does **not** edit `LayerCaps::DEFAULT`, `layout.rs`, `normalize.rs` or the context guide. The aggregator already forces `Origin::Historical` on the `Experience` layer and titles its section "Historical evidence, not authorization: experience", so even a bug in this source could not drop the label. The 4 KiB `Experience` cap is shared with the built-in `similar_incidents` source, so the experience source returns **small** items (6.7: at most 3 items of at most 1 KiB for an overview) and the shrink order (`Experience` goes first) decides what is left out. If, in the investigation drill, both sources cannot be shown, report it; do not change the cap here.

`search_experience` does not go through the aggregator: like `request_evidence` it calls `ExperienceSource::fetch` directly (6.6), because the model asked for exactly that search.

## 4. Exact scope

Each file at most 500 lines (web files at most 250). Split as listed.

### 4.1 `domovoi-core` (no I/O)

| File | Change |
|---|---|
| `src/experience/mod.rs` **(new)** | module doc, re-exports, `HISTORICAL_LABEL` |
| `src/experience/field.rs` **(new)** | `SummaryField`, `SummarySource`, `SummaryRevision` |
| `src/experience/summary.rs` **(new)** | `ResolutionSummary`, `NarrativeField`, `ResolutionOutcome`, `ExecutedContract`, `SummarySubject`, `ResolutionBrief`, `subjects_text`, `contracts_text` |
| `src/experience/derive.rs` **(new)** | `CodeFields`, `code_fields(&OperationSnapshot)`, `outcome_tags` (pure) |
| `src/experience/query.rs` **(new)** | `ExperienceScope`, `ExperienceQuery`, `ExperienceHit`, `MatchPass`, `SimilarQuery`, `SimilarHit`, `SimilarReason`, `SIMILAR_MIN_SCORE`, `score_similar`, `order_similar`, `query_words` (pure; the store and the in-memory twin both call these, so ranking cannot drift) |
| `src/ports/experience.rs` **(new)** | `ExperienceRead` |
| `src/ports/uow/operations.rs` (26) | `OperationWrite::SummaryField(SummaryFieldWrite)` |
| `src/ports/uow/effects.rs` | `EffectKind::DraftResolution` (`as_str`, `parse`, the test list) |
| `src/ports/uow/journal.rs` | `JournalKind::OperationSummary` (`operation.summary`) (`as_str`, `parse`, the test list) |
| `src/meta/{tool,schema,call,tests}.rs` (31) | `MetaTool::SearchExperience` and everything it needs (6.6) |
| `src/ports/mod.rs`, `src/lib.rs` | register and re-export (`pub mod experience;`, `ExperienceRead`) |
| `tests/experience.rs` **(new)**, `tests/gates.rs`, `tests/serde_snapshots.rs`, `tests/snapshots/` | section 12.1 |

### 4.2 `domovoi-store`

| File | Change |
|---|---|
| `migrations/<next free number>_experience_search.sql` **(new)** | section 8. List the directory on your branch and take the next unused number; never copy a number from this brief |
| `src/fts.rs` | add `match_any` and `match_any_in` (every word quoted, joined with `OR`, at most 12 distinct words) and their unit tests; `match_all` and `words` unchanged |
| `src/experience/{mod,apply,project,read,search,similar,rows}.rs` **(new)** | `apply` (the `SummaryField` write), `project` (the projection and side tables), `ExperienceRead for Store` (`read`, `search`, `similar`), row codecs |
| `src/uow/operations.rs` (26) | one new match arm calling `experience::apply` |
| `src/lib.rs` | `mod experience;` |
| `tests/sqlite/experience.rs` **(new)**, `tests/sqlite/main.rs`, `tests/sqlite/contracts.rs` | SQLite tests, the N-1 upgrade test and the registration of the shared suite (12.2) |

### 4.3 `domovoi-testkit`

`src/repos/experience.rs` **(new)** (`ExperienceRead` for `InMemoryStore`, calling the core pure functions), `src/repos/uow_operations.rs` (26) handles `OperationWrite::SummaryField` with the same rules as the store (section 8.2), `src/contracts/experience.rs` **(new)** (the shared suite, 12.2), `tests/contracts.rs` registers it, `src/crash/mod.rs` summarises the new write (`summary:field`).

### 4.4 `domovoi-engine`

| File | Change |
|---|---|
| `src/operations/resolution/mod.rs` **(new)** | module doc, `EditSummaryField`, `EditedField`, `SummaryError`, `EditSummaryError`, constants |
| `src/operations/resolution/plan.rs` **(new)** | pure `plan_draft` and `plan_edit` |
| `src/operations/resolution/narrative.rs` **(new)** | pure `draft_narrative(&OperationSnapshot) -> Option<Narrative>` |
| `src/operations/resolution/handler.rs` **(new)** | the `DraftResolution` effect handler |
| `src/operations/resolution/backfill.rs` **(new)** | the start-up queueing of terminal operations that have no summary |
| `src/operations/decide/resolution.rs` **(new)**, `decide/mod.rs` | the one post-step that adds the `DraftResolution` effect (6.4) |
| `src/operations/writer/{commands,summary,mod}.rs` | commands `WriteResolution`, `EditSummary`, `QueueResolution`; `writer/summary.rs` processes them |
| `src/operations/{mod,audit}.rs` | `OperationHandle::edit_summary_field`, `OperationDeps.experience`, handler registration and backfill in `spawn`, audit constant `OPERATION_SUMMARY_EDITED` |
| `src/context/experience.rs` **(new)**, `src/context/mod.rs` | `ExperienceSource`, `ExperienceDeps` |
| `src/triage/meta/experience.rs` **(new)** | `ExperienceAccess`, the dispatch of `SearchExperience`, the result text |
| `src/triage/meta/{dispatch,loop_run}.rs`, `src/triage/mod.rs`, `src/triage/{run,followup,chat}.rs` | `LoopScope.experience`, `TriageDeps.experience`, counting and recording of the new call, the three callers set the access |
| `prompts/triage_meta_system.md`, the two conversation prompts | name the new tool and the label (6.6) |
| `tests/operations/resolution.rs` **(new)**, `tests/services/experience.rs` **(new)**, `tests/gates.rs`, `tests/drills/probes.rs`, and the existing tests that assert the offered tool list (12.6) | section 12 |
| `Cargo.toml` | `[[test]]` entries only if the binaries need registering (`autotests = false`) |

### 4.5 `domovoi-hub`, web, docs

| File | Change |
|---|---|
| `crates/domovoi-hub/src/api/experience.rs` **(new)** | the five handlers |
| `crates/domovoi-hub/src/api/dto_experience.rs` **(new)** | the DTOs of 6.9 |
| `crates/domovoi-hub/src/api/dto_operations.rs`, `api/operations.rs` (27) | `OperationSummaryDto.resolution`, filled by list and detail from `ExperienceRead::briefs` |
| `crates/domovoi-hub/src/api/mod.rs` | `ApiDeps.experience: Arc<dyn ExperienceRead>`, routes, tag `experience` |
| `crates/domovoi-hub/src/services/operations.rs` (29) | `OperationCommands::edit_summary_field` |
| `crates/domovoi-hub/src/runtime/*` | build the `ExperienceSource` once; pass it to `TriageDeps`, the `ExperienceRead` store handle to `OperationDeps` and `ApiDeps`; push the source onto `TriageDeps.extra_context` (step 12) |
| `crates/domovoi-hub/examples/dev_hub.rs`, `tests/hub/api_harness.rs`, `tests/hub/experience_api.rs` **(new)**, `tests/hub/main.rs`, `tests/gates.rs` | the new field, endpoint tests, the hub half of the global-scope gate |
| `web/src/api/{openapi.json,schema.d.ts}` | regenerated, never hand-edited |
| `web/src/api/hooks/experience.ts` **(new)**, `experience.test.ts` **(new)**, keys, `web/src/test/fixtures/experience/*.json` **(new)** | typed hooks and fixtures only (6.10) |
| docs | section 15 |

## 5. Explicit non-goals

| Not here | Owner / reason |
|---|---|
| The aggregator, layer caps, section layout, the context guide, the one provider-chain loop | 32. This prompt provides a `ContextSource` and pushes it onto `TriageDeps.extra_context` (step 12) |
| Any page, panel or component ("Similar past operations", a summary editor, a search page) | 48 (knowledge pages) and 30 (operations UI). This prompt ships endpoints, DTOs, typed hooks and fixtures |
| Facts, relations, entities, `KnowledgeRead` | 34 (merged). The experience layer never reads facts and facts never read summaries |
| A second model call to write the narrative, or to rewrite a summary | not needed (section 2); it would spend an admission slot |
| Summaries for **legacy** actions and incidents that never had an operation | RW-32: legacy rows are never converted. `incident_history` and the existing similar-incidents context keep covering them |
| Retention of operation journal rows or raw outputs | 57 (config key exists, "not applied yet"). Summaries are retained, so there is nothing to purge here |
| Policy use of experience; any code path where a past approval or success lets a step skip approval or satisfy a rule | forbidden. Experience is evidence (D-02, RW-25) |
| Cross-host results for a model-initiated search, a model-controlled `scope` argument | forbidden (section 11) |
| Embedding or vector search, stemming, synonyms | out of scope; FTS5 `unicode61` only |
| `ContextScope::Global`, or any change to `ContextLayer` | 31 owns `ContextScope`; 32 owns layer additions |
| New config keys, wire changes, `PROTOCOL_VERSION`, any `domovoi-agent`, `domovoi-proto`, `domovoi-runner`, `domovoi-llm` file | none needed (RW-42) |
| A new dependency | none: `serde`, `serde_json`, `jiff`, `async-trait`, `thiserror`, `sqlx` are already in the crates |
| `HumanApproval::new`, `Authorization`, `PolicyPermit` anywhere in the new code | INV-1; the gates of prompt 31 scan the meta, plan and context directories and still pass |
| An arch-rule change in `xtask/src/arch/rules.rs` | none needed, so no new ADR. If `cargo xtask arch-check` fails, stop and report: do not edit the rules |

## 6. Required interfaces and contracts

Marks: **fixed** = names, variants, JSON and semantics must match; **convention** = follow repository style. Every public item gets a doc comment saying what it is for. Never copy a prompt number, "the build plan" or an RW/D id into code, doc comments, commit messages, the book or the PR description: describe behaviour in plain words.

### 6.1 Core types **fixed** (names, variants, serde names, semantics)

```rust
// crates/domovoi-core/src/experience/mod.rs
/// The words that label every past-operation item and result block shown to a model or a person.
pub const HISTORICAL_LABEL: &str = "historical evidence, not authorization";

// crates/domovoi-core/src/experience/field.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub enum SummaryField { Outcome, Verified, Subjects, ContractsExecuted, Hosts, Problem, Cause, Fix }
impl SummaryField {
    pub const ALL: [SummaryField; 8];                     // exactly this order
    pub const fn as_str(self) -> &'static str;            // outcome verified subjects contracts_executed hosts problem cause fix
    pub fn parse(text: &str) -> Option<Self>;
    pub const fn is_code_owned(self) -> bool;             // the first five
    pub const fn is_narrative(self) -> bool;              // Problem, Cause, Fix
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub enum SummarySource { Code, Model, User }             // as_str, parse
impl SummarySource { pub const fn may_write(self, field: SummaryField) -> bool; }   // code-owned: Code only; narrative: Model or User

/// One immutable revision. `value` is canonical JSON: `outcome` a string, `verified` a bool, `subjects`, `contracts_executed`
/// and `hosts` arrays (shapes below), a narrative field a JSON string of 1 to 1000 characters, already redacted.
pub struct SummaryRevision {
    pub operation: OperationId, pub field: SummaryField, pub revision: u32,   // 1-based, dense per (operation, field)
    pub value: serde_json::Value, pub source: SummarySource,
    pub actor: String,                                    // `Actor` in string form, for example "system:operations" or "user:<uuid>"
    pub created_at: Timestamp,
}
```

```rust
// crates/domovoi-core/src/experience/summary.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub enum ResolutionOutcome { Succeeded, Failed, Escalated, Cancelled }               // as_str, parse; the terminal OperationStatus names
pub struct SummarySubject { pub host: HostId, pub check: CheckKey, pub target: Option<TargetKey> }   // JSON {"host","check","target"}; target null when none
pub struct ExecutedContract { pub contract: String, pub host: HostId, pub target: Option<String>, pub outcome: String }
            // outcome is a StepOutcome name: "succeeded" | "failed" | "outcome_unknown" | "running"
pub struct NarrativeField { pub text: String, pub source: SummarySource, pub actor: String, pub revision: u32, pub at: Timestamp }
/// The current value of every field of one finished operation.
pub struct ResolutionSummary {
    pub operation: OperationId, pub goal: String, pub ended_at: Timestamp,
    pub outcome: ResolutionOutcome, pub verified: bool,
    pub subjects: Vec<SummarySubject>, pub contracts: Vec<ExecutedContract>, pub hosts: Vec<HostId>,
    pub problem: Option<NarrativeField>, pub cause: Option<NarrativeField>, pub fix: Option<NarrativeField>,
}
pub struct ResolutionBrief { pub outcome: ResolutionOutcome, pub verified: bool, pub ended_at: Timestamp }
pub fn subjects_text(subjects: &[SummarySubject]) -> String;      // "container.restart_loop plex" words for the index
pub fn contracts_text(contracts: &[ExecutedContract]) -> String;  // "skill.restart_container plex" words for the index
```

```rust
// crates/domovoi-core/src/experience/derive.rs   (pure; reads a loaded snapshot, never the clock or a store)
pub struct CodeFields {
    pub outcome: ResolutionOutcome, pub verified: bool, pub subjects: Vec<SummarySubject>,
    pub contracts: Vec<ExecutedContract>, pub hosts: Vec<HostId>,
}
/// `None` while the operation is not terminal.
pub fn code_fields(snapshot: &OperationSnapshot) -> Option<CodeFields>;
impl CodeFields { pub fn values(&self) -> [(SummaryField, serde_json::Value); 5]; }   // in SummaryField::ALL order
/// A sorted, deduplicated set drawn from the closed vocabulary below. Search aid only: it is indexed, never shown, never a revision.
pub fn outcome_tags(snapshot: &OperationSnapshot) -> Vec<&'static str>;
```

`code_fields` rules (**fixed**):

| Field | Rule |
|---|---|
| `outcome` | the terminal status: `Succeeded`, `Failed`, `Escalated` or `Cancelled` map to the same-named `ResolutionOutcome` |
| `verified` | `true` only when the **highest-ordinal** `Verification` step has outcome `Succeeded` and the `Execution` step it `verifies` (the `verifies` edge points from it to that step) has outcome `Succeeded`. No `Verification` step: `false`. An investigation that only delivered a report is `succeeded` and not verified |
| `subjects` | `operation.pinned` in order |
| `contracts_executed` | every `Execution` step whose `ExecutionRecord.dispatched` is `Some` (something was actually sent), in ordinal order, as `{contract (ContractId string), host, target, outcome (step outcome name)}`; at most 32 entries (later ones are ignored) |
| `hosts` | the sorted, distinct union of the pinned subjects' hosts and every `ExecutionRecord.host` |

`outcome_tags` vocabulary (**fixed**; nothing else may ever be a tag): `denied` (an `Approval` step `Refused`), `expired` (an `Approval` step `NotRun`), `refused` (an `Execution` step `Refused` that has no `Approval` step), `verification_failed` (a `Verification` step `Failed`), `retried` (any `retry_of` edge), `outcome_unknown` (an `Execution` step `OutcomeUnknown`), `question_asked` (any `Question` step), `escalated`, `cancelled` (the operation status).

```rust
// crates/domovoi-core/src/experience/query.rs
pub enum ExperienceScope { Host(HostId), Global }          // see section 11 for who may build Global
pub struct ExperienceQuery { pub scope: ExperienceScope, pub text: String, pub limit: u16 /* clamped 1..=50 */, pub exclude: Option<OperationId> }
pub enum MatchPass { AllWords, AnyWord }                    // as_str: "all_words" | "any_word"
pub struct ExperienceHit { pub summary: ResolutionSummary, pub pass: MatchPass, pub snippet: Option<String> }
pub struct SimilarQuery { pub host: HostId, pub check: Option<CheckKey>, pub target: Option<TargetKey>, pub text: String,
                          pub contracts: Vec<String>, pub exclude: Option<OperationId>, pub limit: u16 /* clamped 1..=20 */ }
#[derive(Clone, PartialEq, Eq)] pub enum SimilarReason { SameCheck, SameTarget, SameContract(String), SimilarWords }
pub struct SimilarHit { pub summary: ResolutionSummary, pub score: u32, pub reasons: Vec<SimilarReason> }
pub const SIMILAR_MIN_SCORE: u32 = 4;
/// Pure. `words_rank` is the candidate's zero-based position in the any-word text matches (None when absent).
pub fn score_similar(query: &SimilarQuery, candidate: &ResolutionSummary, words_rank: Option<usize>) -> (u32, Vec<SimilarReason>);
pub fn order_similar(hits: &mut Vec<SimilarHit>);           // score desc, ended_at desc, operation id desc
pub fn query_words(text: &str) -> Vec<String>;              // distinct lowercase words in first-seen order, at most 12, each at most 40 chars
```

`score_similar` (**fixed**): `8` when `query.check` is `Some` and equals a subject's check; `+4` when `query.target` is `Some` and equals a subject's target; `+2` for each distinct contract id of `query.contracts` that the candidate ran, at most three of them; `+4` when `words_rank` is `Some(0..=4)`, `+2` when `Some(5..=19)`. `reasons` lists `SameCheck`, `SameTarget`, one `SameContract(id)` per counted contract, `SimilarWords` when a words bonus was added, in that order. A candidate scoring below `SIMILAR_MIN_SCORE` is dropped by the caller. A candidate on another host never scores (the stores only fetch candidates for `query.host`).

```rust
// crates/domovoi-core/src/ports/experience.rs
#[async_trait]
pub trait ExperienceRead: Send + Sync {
    /// The current summary of `operation`; `None` when no revision exists yet.
    async fn summary(&self, operation: OperationId) -> Result<Option<ResolutionSummary>, StoreError>;
    /// Every revision of every field, ordered by (field in `SummaryField::ALL` order, revision).
    async fn revisions(&self, operation: OperationId) -> Result<Vec<SummaryRevision>, StoreError>;
    /// The newest revision of each field that has one, in `SummaryField::ALL` order.
    async fn current(&self, operation: OperationId) -> Result<Vec<SummaryRevision>, StoreError>;
    /// Outcome and verified for each operation that has a summary; absent ids are absent from the map.
    async fn briefs(&self, operations: &[OperationId]) -> Result<BTreeMap<OperationId, ResolutionBrief>, StoreError>;
    /// Full-text search over finished operations. Host scope returns only operations whose `hosts` set is exactly that host.
    /// An empty `text` (no words) lists the newest operations of the scope instead. Pass-1 hits (all words) come before pass-2 hits (any word).
    async fn search(&self, query: &ExperienceQuery) -> Result<Vec<ExperienceHit>, StoreError>;
    /// Past operations of `query.host` that resemble the query, best first, each scoring at least `SIMILAR_MIN_SCORE`.
    async fn similar(&self, query: &SimilarQuery) -> Result<Vec<SimilarHit>, StoreError>;
    /// Terminal operations that have no revision at all, oldest `ended_at` first (the start-up backfill).
    async fn undrafted(&self, limit: u16) -> Result<Vec<OperationId>, StoreError>;
}
```
The port has **no write method**: every write is an `OperationWrite::SummaryField` in a `Transaction`.

### 6.2 The write **fixed**

```rust
// crates/domovoi-core/src/ports/uow/operations.rs (prompt 26's enum gains one variant)
OperationWrite::SummaryField(SummaryFieldWrite)
pub struct SummaryFieldWrite {
    pub operation: OperationId, pub field: SummaryField,
    pub expected_revision: u32,                  // the field's newest existing revision, 0 when it has none; the new revision is expected_revision + 1
    pub value: serde_json::Value, pub source: SummarySource, pub actor: String, pub at: Timestamp,
    pub tags: Option<Vec<String>>,               // Some on a code write (the full tag set, replaces the stored one); None on a narrative write
}
```
Store rules are in section 8.2. `CommitReceipt::versions` reports nothing for this write.

### 6.3 Journal and audit **fixed**

| Kind | String | Payload `payload_v 1` | Who |
|---|---|---|---|
| `JournalKind::OperationSummary` | `operation.summary` | `{"operation": "<id>", "field": "<name>", "revision": N, "source": "code"\|"model"\|"user"}` (**never the value**) | the actor of the write; aggregate `AggregateRef::operation(id)` |

Audit kind (constant `OPERATION_SUMMARY_EDITED = "operation.summary_edited"` in `operations/audit.rs`), written **only for `source = user`** revisions, in the same transaction: actor `Actor::User(..)`, subject `operation:<id>`, detail `{"operation": "<id>", "field": "<name>", "revision": N}` and nothing else. Code and model revisions write a journal row and no audit row (they are reconstructable and made by the service itself). No journal payload, audit row or log line contains a summary value.

### 6.4 The effect `DraftResolution` **fixed**

```rust
EffectKind::DraftResolution   // as_str "draft_resolution"; parse round-trips; added to the list in effect_kinds_round_trip
// payload: {"v": 1, "operation": "<uuid>"}   aggregate: AggregateRef::operation(id)   max_attempts 5   not_before: now
// dedup_key: "resolution:<operation>"                       the terminal transition and the start-up backfill
//            "resolution:<operation>:<step>"                a late result on a terminal operation (below)
```

**When it is created (in `decide`, one place).** A new pure post-step `decide/resolution.rs::add_resolution_effect(old: &OperationStatus, decision: &mut Decision, event: &OperationEvent, ids)` runs last in `decide`. It adds exactly one `DraftResolution` `EffectSpec` when:

| Situation | Dedup key |
|---|---|
| the old status was not terminal and `decision.operation.status` is terminal (`Finish { Succeeded | Failed | Escalated }`, `Cancel`, `Verified { passed: true }` finishing the operation, the cycle-limit escalation after `Verified { passed: false }`, any other path that ends the operation) | `resolution:<operation>` |
| the old status was already terminal and the event is `ExecutionSettled` or `Verified` for a step that was still `Running` (prompt 26's "the truth is recorded" rule) | `resolution:<operation>:<step>` |

No other event creates it. The terminal commit's `EffectCancel` (prompt 26) is applied **before** new effects, so it does not cancel this one. A refused event creates nothing.

**The handler** (`operations/resolution/handler.rs`, registered in `OperationService::spawn` on the operation runner beside the others; payload that does not parse is `Dead { "unreadable payload" }`; stale claim is `Handled`):

```text
1. snapshot = OperationRepo::load(operation).        None  -> FinishEffect.        Not terminal -> FinishEffect.
2. existing = ExperienceRead::current(operation)     store error -> Retry.
3. code = code_fields(&snapshot)                     (Some, the operation is terminal)
4. planned = plan_draft(&snapshot, &existing, narrative = draft_narrative(&snapshot), redactor, now)
5. planned.is_empty() -> FinishEffect.
6. one writer command WriteResolution { operation, revisions: planned, held effect (id, claim token) }
   -> one commit: the SummaryField writes (code fields first, in SummaryField::ALL order), one journal row per revision,
      the completion of the held effect.
   CommitError::Conflict (a revision slot was taken meanwhile) -> Retry.   StaleEffect -> Handled.
```
`plan_draft` (**fixed**, pure):
- For each of the five code fields: compute the canonical JSON value (`CodeFields::values`). If the field has **no** revision, or its newest revision's value differs, plan revision `newest + 1` (1 when none), `source = Code`, `actor = "system:operations"`. An unchanged field plans nothing. `tags` is `Some(outcome_tags(..))` on **every** planned code write and the same in each of them.
- For each narrative field: only when the field has **no revision at all** and the draft has text for it, plan revision 1, `source = Model`, `actor = "system:operations"`. A field that has any revision (including a user's) is never redrafted.
- All text values are passed through the `Redactor` and bounded (below) before they are planned.

**`draft_narrative`** (**fixed**, pure, in `narrative.rs`; text only, nothing is fetched). Look for the **newest `ReasoningTurn` step with outcome `Succeeded`** whose `progress.output` parses as the investigation's `TurnOutput` of kind `report` or `cached` (prompt 27/31 shape; tolerate unknown fields and a missing `output`).
- `problem` = `report.summary`; `cause` = `report.cause`; `fix` = `report.next_step`, followed, when `report.manual_steps` is non-empty, by a newline and the manual steps joined by newlines.
- If no such turn exists or its `output` is gone (pruned) or does not parse: fall back to the **immutable step intents**: the newest `Assessment` step's `text` (split at its first newline: the first part is `problem`, the rest is `cause`) and the newest `Plan` step's `text` as `fix`.
- A field whose text is empty after trimming has no draft. Each text is trimmed, cut to 1000 characters on a character boundary (no marker) and has control characters other than `\n` removed. No turn, no assessment: no narrative at all, and the summary is still created with its five code fields.

**Writer command and handle** (names **fixed**, bodies **convention**):

```rust
// crate-private, used only by operations/resolution/handler.rs and backfill.rs
OperationCommand::WriteResolution { operation: OperationId, revisions: Vec<PlannedRevision>, held: (EffectId, ClaimToken) }
OperationCommand::QueueResolution { operation: OperationId }    // creates DraftResolution with dedup_key "resolution:<operation>"; a "duplicate effect" constraint is treated as success
pub struct PlannedRevision { pub field: SummaryField, pub value: serde_json::Value, pub source: SummarySource, pub actor: String, pub tags: Option<Vec<String>> }
```
`WriteResolution` sets `expected_revision` from the loaded `current` revisions at write time. The writer builds **one** `Transaction`: the writes, the journal drafts (`OperationSummary`, `occurred_at = now`, `payload_v = 1`), the completion of the held effect. No `decide` call: a summary is not an operation event.

**Start-up backfill** (inside `OperationService::spawn`, after the startup recovery of prompt 26 and before the runner claims, one pass): `ExperienceRead::undrafted(500)` and one `QueueResolution` per id. Idempotent; a hub restart queues only what is still missing. Say in the report how many were queued in the test. The backfill never runs for a non-terminal operation (the port only returns terminal ones).

### 6.5 Editing a narrative field **fixed**

```rust
// crates/domovoi-engine/src/operations/resolution/mod.rs
pub struct EditSummaryField { pub field: SummaryField, pub value: String, pub expected_revision: u32 }
pub struct EditedField { pub revision: SummaryRevision, pub unchanged: bool }
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SummaryError {
    #[error("that is not a summary field")]                                         UnknownField,
    #[error("this field is filled by Domovoi and cannot be edited")]                CodeOwned(SummaryField),
    #[error("the operation has not finished")]                                      NotTerminal,
    #[error("the summary changed since you opened it")]                             Stale { expected: u32, actual: u32 },
    #[error("write something: an empty value is not accepted")]                     Empty,
    #[error("the text is longer than {max} characters")]                            TooLong { max: usize },
    #[error("the text contains characters that are not allowed")]                   BadText,
}
pub enum EditSummaryError { Summary(SummaryError), Operation(OperationError) }       // thiserror; From both
impl OperationHandle {
    pub async fn edit_summary_field(&self, operation: OperationId, edit: EditSummaryField, actor: Actor)
        -> Result<EditedField, EditSummaryError>;
}
```
`plan_edit` (pure) and the writer's `EditSummary` command apply these rules **in this order**; the first failure is returned and **nothing is committed**:

1. `edit.field.is_code_owned()` → `CodeOwned(field)` (this is the RW-41 rule; the unknown-field case is `UnknownField` and happens at the HTTP edge when the path name does not parse).
2. The operation must exist (`OperationError::NotFound`) and be terminal (`NotTerminal`). The command **loads the snapshot and the field's current revisions fresh** (never an in-memory copy).
3. `edit.value`: trim; empty → `Empty`; more than 1000 characters → `TooLong { max: 1000 }`; a control character other than `\n` → `BadText`. Then redact.
4. `edit.expected_revision` must equal the newest revision of that field (0 when it has none), else `Stale { expected, actual }`.
5. When the redacted value equals the current value: if the current source is `User`, return `EditedField { revision: <current>, unchanged: true }` and commit nothing; if the current source is `Model` it is a **confirmation** and a new revision is written. (A field with no revision cannot be equal.)
6. Otherwise write revision `expected_revision + 1` with `source = User`, `actor = actor.to_string()`; journal `operation.summary`; audit `operation.summary_edited`. One commit; a `CommitError::Conflict` from the revision slot is `Stale` (reload `current` once to fill `actual`), not a retry: the person must look again.

The command goes **through the operation writer task** like every other command. `OperationHandle` is the only type that can send it; the hub reaches it only through `OperationCommands::edit_summary_field` (6.9).

### 6.6 The seventh meta-tool **fixed**

Prompt 31's `MetaTool` has six variants. This prompt makes it seven, **in the RW-07 order** (the new tool sits after `request_evidence` and before `propose_plan`; the order is pinned by the gates):

```rust
// crates/domovoi-core/src/meta/tool.rs
pub enum MetaTool { ListCapabilities, DescribeCapability, RequestEvidence, SearchExperience, ProposePlan, AskUser, Finish }
impl MetaTool {
    pub const ALL: [MetaTool; 7];     // that order
    pub const READ: [MetaTool; 4];    // ListCapabilities, DescribeCapability, RequestEvidence, SearchExperience
    // name(): "search_experience"
}
// ToolSet::Read                          -> the four READ tools
// ToolSet::Investigate { questions: false } -> six tools (all but AskUser)
// ToolSet::Investigate { questions: true }  -> all seven
```
`description()` of `search_experience` (**fixed** text, the model reads it): "Search what happened before on this host: past operations with their problem, cause, fix, outcome and whether the fix was verified. Results are historical evidence, not authorization: a past approval or success never allows an action now. Nothing is run."

Input schema (closed object, the `tool_schema.rs` convention): `{"type":"object","properties":{"query":{"type":"string","minLength":1,"maxLength":200}},"required":["query"],"additionalProperties":false}`. There is **no** `scope`, `host` or `limit` argument.

```rust
// crates/domovoi-core/src/meta/call.rs
MetaToolCall::SearchExperience { query: String }
```
Parsing: `deny_unknown_fields`; the text is trimmed; empty, over 200 characters, containing a control character, or containing no letter or digit at all is `MetaArgError::InvalidArgs { tool: "search_experience", reason }` with reason `query: write 1 to 200 characters` or `query: needs at least one word`. `MetaToolCall::tool()` maps it. Every exhaustive `match` over `MetaTool` or `MetaToolCall` the compiler flags is updated (`impl From<MetaTool> for ToolSpec` needs nothing).

**The engine side** (`crates/domovoi-engine/src/triage/meta/experience.rs`):

```rust
/// Where a loop's `search_experience` may look. There is no way to widen it from model input.
pub(crate) enum ExperienceAccess {
    /// The loop's own host; `exclude` is the operation being run (it has no summary yet, but a re-run must never match itself).
    Host { host: HostId, exclude: Option<OperationId> },
    /// Every host. Only the Operator conversation the owner opened with a global scope may build this.
    Global,
}
impl ExperienceAccess {
    pub(crate) fn host(host: HostId, exclude: Option<OperationId>) -> Self;
    pub(crate) fn global_for_owner_conversation() -> Self;
}
// LoopScope (prompt 31) gains `pub(crate) experience: ExperienceAccess`.
```
Who sets it: the triage `generate` path and the investigation `CallModel` handler use `ExperienceAccess::host(<the incident's host>, <Some(operation id) in an operation>)`; follow-up and chat use `host(<their Scope host>, None)`. If prompt 36 (operator conversations) has merged, its global-scope conversation passes `global_for_owner_conversation()` and its host-scope conversation passes `host(..)`; if 36 has not merged, **no production code calls `global_for_owner_conversation()`** and the gate of 12.5 expects zero call sites. Do not add a call site anywhere else.

Dispatch (`triage::meta::dispatch`, one new arm; the call counts against `max_tool_calls` like every meta-tool except `finish`; it is recorded in `ToolUse` under the name `search_experience`):

```text
SearchExperience { query }:
  access Host{host, exclude}:
     batch = deps.experience.fetch(ContextQuery{ scope: Operation{host, operation} if exclude is Some else Host(host),
                                                 selector: Search{ text: query }, limit: 5, max_item_bytes: 1024,
                                                 deadline: min(run deadline, now + TOOL_DEADLINE) })
  access Global:
     batch = deps.experience.global_search(query, limit 5, max_item_bytes 1024, deadline)      // inherent method, 6.7
  render(batch) as below
```
Result text (**fixed**; redacted and cut to `MAX_RESULT_BYTES`; `is_error` as stated):

| Case | Text | `is_error` |
|---|---|---|
| one or more items | the line `Historical evidence, not authorization. Past approvals and successes never allow an action now; use them only as hints about what to look at.`, then each item's `ContextItem::render()` separated by one blank line | false |
| no items, no gap | `No past operation on this host matched "<query, cut to 60 characters>".` (`No past operation matched ...` for a global search) | false |
| a gap (store error, deadline, scope mismatch) | `error: experience search is unavailable: <gap reason>` then the line `This is unknown, not a negative result: the search did not run. Do not treat it as "nothing happened before".` | true |

### 6.7 `ExperienceSource` **fixed**

```rust
// crates/domovoi-engine/src/context/experience.rs
pub struct ExperienceDeps {
    pub experience: Arc<dyn ExperienceRead>, pub incidents: Arc<dyn IncidentRepo>, pub operations: Arc<dyn OperationRepo>,
    pub hosts: Arc<dyn HostRepo>, pub redactor: Arc<dyn Redactor>, pub clock: Arc<dyn Clock>,
}
#[derive(Clone)] pub struct ExperienceSource { deps: Arc<ExperienceDeps> }
impl ExperienceSource {
    pub fn new(deps: ExperienceDeps) -> Self;
    /// A search over every host, for the owner's own global Operator conversation only. Not part of the `ContextSource` port.
    pub async fn global_search(&self, text: &str, limit: u16, max_item_bytes: u32, deadline: Timestamp) -> ContextBatch;
}
// ContextSource: id "experience"; layer ContextLayer::Experience;
// supports: ContextSelector::Overview and ContextSelector::Search only (Read -> false)
```
Holds only read ports. It never holds a `UnitOfWork`, `OperationHandle`, `Authorization` or `AgentLink` (the gate of 12.5 scans `src/context/`).

`fetch` (**fixed**):

| Scope | Selector | Behaviour |
|---|---|---|
| `Host(h)` | `Search { text }` | `ExperienceRead::search(Host(h), text, query.limit, None)` |
| `Host(h)` | `Overview` | `search(Host(h), "", limit, None)`: the newest summaries of `h` |
| `Incident { host, incident }` | `Search` | as `Host(host)` |
| `Incident { host, incident }` | `Overview` | load the incident (`IncidentRepo::get`; missing → gap `incident not found`; `incident.host() != host` → gap `scope does not match the incident`); `ExperienceRead::similar(SimilarQuery { host, check: Some(incident.check()), target: incident.target(), text: incident.title(), contracts: [], exclude: None, limit })` |
| `Operation { host, operation }` | `Search` | as `Host(host)` with `exclude: Some(operation)` |
| `Operation { host, operation }` | `Overview` | `OperationRepo::get` (missing → gap `operation not found`); the host must be one of the pinned hosts, else gap `scope does not match the operation`; `similar` from the first pinned subject's check and target, `text = goal`, `exclude: Some(operation)` |

**Size rules (fixed constants in `context/experience.rs`):** `ITEM_BODY_MAX = 1024`; the effective body limit of an item is `min(query.max_item_bytes, ITEM_BODY_MAX)` (the aggregator asks for 4096; the model tool asks for 1024); `Overview` returns at most `min(query.limit, 3)` items (so it fits the 4 KiB `Experience` layer that the built-in similar-incidents source also uses); `Search` returns at most `min(query.limit, 5)`.

Before any read: `clock.now() >= query.deadline` → gap `timed out`. A store error → gap `could not read past operations` (no store text). `ExperienceRead` errors never produce an empty batch.

`item_of(summary, max_item_bytes, host_names)` (**fixed** layout):

- `id = ContextItemId::new(&source_id, "op-<uuid>")`; `layer = Experience`; `status = Known`; `truncated` set when the body was cut.
- `provenance = Provenance { origin: Historical, reference: "operation:<uuid>", observed_at: Some(summary.ended_at), actor: None }`.
- `title` (at most 120 characters, redacted): `"<check>[ <target>] - <outcome>[, verified]"` from the first subject, else the goal cut to 60 characters.
- `body`, one line each, in this order (a line without data is omitted):

```text
operation: <uuid>
ended: <RFC 3339 UTC>
host: <host name>                         (only when host_names is Some: the global search)
outcome: <succeeded|failed|escalated|cancelled>
verified: yes|no
ran: skill.restart_container plex (succeeded); ...      (at most 200 characters)
problem [model]: <text>                   (each narrative text pre-cut to 240 characters)
cause [user]: <text>
fix [model]: <text>
```
  The bracket is the field's source (`code` never appears: narrative fields are `model` or `user`). The whole body is cut to the effective body limit on a character boundary. Every string passes the `Redactor` again (idempotent).

### 6.8 Registration

`TriageDeps` gains `pub experience: ExperienceSource` (doc: "past operations, read only; the source of `search_experience`"); update every constructor (`grep -rn "TriageDeps {" crates`). The hub builds **one** `ExperienceSource` from the store, `hosts`, `incidents`, the operation repository, `redactor` and `clock`, puts a clone in `TriageDeps.experience` for the meta-tool dispatch, and pushes the same value (as `Arc<dyn ContextSource>`) onto `TriageDeps.extra_context` (prompt 32's registration point) so every investigation, follow-up and chat bundle asks it, in its `Experience` layer. The `ExperienceRead` handle given to `OperationDeps` and `ApiDeps` is the store itself. Everything is built **even when `[orchestration] enabled = false`**: the tables then hold no rows, every search is a known empty (the bundle shows `None.` in that section), and nothing else changes. Test harnesses that build `TriageDeps` do the same push where they want the experience section.

### 6.9 Hub endpoints and DTOs **fixed** (names and JSON); handlers **convention**

All behind `Authed`; the POST also passes the existing CSRF guard. `utoipa::ToSchema` structs in `api/dto_experience.rs`. Enumerations are `String` fields (a consumer shows an unknown value generically). Timestamps RFC 3339 UTC with a trailing `Z`. Optional fields serialise as `null`. `label` is always `"historical evidence, not authorization"` (`HISTORICAL_LABEL`) on every DTO that carries past operations, so a screen cannot forget it.

```rust
pub struct HostRefDto { pub id: String, pub name: Option<String> }           // name null when the host was deleted
pub struct SummaryFieldDto {
    pub field: String,                    // SummaryField name
    pub value: serde_json::Value,         // canonical JSON of the revision (string, bool or array)
    pub source: String,                   // "code" | "model" | "user"
    pub actor: String, pub revision: u32, pub created_at: String,
    pub editable: bool,                   // narrative field of a finished operation
}
pub struct SummaryRevisionDto { pub field: String, pub revision: u32, pub source: String, pub actor: String, pub created_at: String, pub value: serde_json::Value }
pub struct ResolutionSummaryDto {
    pub operation_id: String, pub as_of_seq: u64,
    pub state: String,                    // "not_ready" (operation still open) | "drafting" (finished, no revision yet) | "ready"
    pub ended_at: Option<String>,
    pub fields: Vec<SummaryFieldDto>,     // current revision of each field that has one, SummaryField::ALL order
    pub history: Vec<SummaryRevisionDto>, // every revision, oldest first; at most 200 (the newest 200 when more exist)
    pub history_truncated: bool,
    pub label: String,
}
pub struct EditSummaryFieldRequest { pub value: String, pub expected_revision: u32 }
pub struct EditSummaryResponse { pub field: SummaryFieldDto, pub unchanged: bool }
pub struct ExecutedContractDto { pub contract: String, pub host_id: String, pub target: Option<String>, pub outcome: String }
pub struct PastOperationDto {             // one finished operation, the shape every list below shares
    pub operation_id: String, pub goal: String, pub ended_at: String,
    pub outcome: String, pub verified: bool,
    pub hosts: Vec<HostRefDto>, pub contracts: Vec<ExecutedContractDto>,
    pub narrative: Vec<SummaryFieldDto>,  // problem, cause, fix: only those that exist, each with its source
}
pub struct SimilarReasonDto { pub code: String, pub detail: Option<String> }   // code: "same_check" | "same_target" | "same_contract" (detail = contract id) | "similar_words"
pub struct SimilarOperationDto { pub operation: PastOperationDto, pub score: u32, pub reasons: Vec<SimilarReasonDto> }
pub struct SimilarOperationsDto { pub items: Vec<SimilarOperationDto>, pub as_of_seq: u64, pub label: String }
pub struct ExperienceItemDto { pub operation: PastOperationDto, pub match_pass: String /* "all_words" | "any_word" */, pub snippet: Option<String> }
pub struct ExperienceSearchDto {
    pub scope: String,                    // "host" | "global"
    pub host: Option<HostRefDto>,         // Some for scope "host"
    pub query: String, pub items: Vec<ExperienceItemDto>, pub as_of_seq: u64, pub label: String,
}
pub struct ResolutionBriefDto { pub outcome: String, pub verified: bool, pub ended_at: String }
// api/dto_operations.rs (prompt 27): OperationSummaryDto gains `pub resolution: Option<ResolutionBriefDto>` (null when no summary yet)
```

| Endpoint | Behaviour |
|---|---|
| `GET /api/v1/operations/{id}/summary` (`get_operation_summary`) | `snapshot_seq` **first**; an unparsable or unknown id → 404; `state` per the DTO comment; `fields` from `ExperienceRead::current`, `history` from `revisions`; `editable` is true only for `problem`, `cause`, `fix` of a terminal operation |
| `POST /api/v1/operations/{id}/summary/{field}` (`edit_operation_summary_field`) | `{field}` must parse as a `SummaryField` (else 400 `that is not a summary field`). Calls `state.services().operations.edit_summary_field(&auth.caller(), op, field, request.value, request.expected_revision)`. Returns `EditSummaryResponse`. Error mapping (one function): `CodeOwned` → 403 `Forbidden`; `UnknownField`, `Empty`, `TooLong`, `BadText` → 400 with the error's own text; `NotTerminal`, `Stale` → 409 with its text (`Stale` adds "reload and try again"); `NotFound` → 404; `NotEnabled` → 503 `orchestration is not enabled`; `Operation(OperationError::Unavailable)` → 503 `the operation service is restarting; try again`; `Operation(other)` → the mapping prompt 27 wrote for `operation_error` (reuse that function, do not copy it); store errors → `ApiError::internal` |
| `GET /api/v1/operations/{id}/similar` (`get_similar_operations`), query `limit` (default 5, clamp 1..=20) | `snapshot_seq` first; 404 for an unknown operation; build `SimilarQuery` from the operation's **first pinned subject** (host, check, target), `text = goal`, `contracts` = distinct contract ids of its Execution steps, `exclude = Some(id)`; an operation with no pinned subject answers an empty list |
| `GET /api/v1/incidents/{id}/similar-operations` (`get_similar_operations_for_incident`), same `limit` | 404 for an unknown incident; `SimilarQuery { host: incident.host(), check: Some(incident.check()), target: incident.target(), text: incident.title(), contracts: [], exclude: None }` |
| `GET /api/v1/experience/search` (`search_experience_owner`), query `q` (optional, at most 200 characters, else 400), `scope` (`host` default or `global`), `host` (a host **name**), `limit` (default 10, clamp 1..=50) | `scope=host` requires `host` (400 `choose a host, or ask for a global search`; unknown name → 404 via `host_named`); `scope=global` with a `host` → 400 `a global search does not take a host`; anything else in `scope` → 400. `scope=global` builds `ExperienceScope::Global` (the only hub site that may). An empty `q` lists the newest operations of the scope |

Every response DTO is built from stored (already redacted) text and passed through `deps.redactor` once more. A failed `ExperienceRead` call is a 500 (`ApiError::internal`), never an empty list; the **one exception** is the optional `OperationSummaryDto.resolution` decoration: `list` and `detail` call `briefs(&ids)` once per response and, if it fails, log at `warn` and set `resolution: null` (the rest of the page is still right).

**Use-case layer** (prompt 29's rule: commands through a service, reads may stay in handlers). `OperationCommands` gains:

```rust
// crates/domovoi-hub/src/services/operations.rs
impl OperationCommands {
    pub async fn edit_summary_field(&self, caller: &Caller, op: OperationId, field: SummaryField, value: &str, expected_revision: u32)
        -> Result<EditedField, EditSummaryError>;
}
```
It is the only caller of `OperationHandle::edit_summary_field` in the hub (a gate scan, 12.5); the actor is `caller.actor()`. When the service holds no operation handle (orchestration off) it returns `SummaryCommandError::NotEnabled`; the method therefore returns `Result<EditedField, SummaryCommandError>` with `enum SummaryCommandError { NotEnabled, Edit(EditSummaryError) }` (`thiserror`, short plain text; this avoids adding a variant to prompt 29's `OperationCommandError`, which other prompts match exhaustively). The handler maps `NotEnabled` as in the table above. The handle never reaches `api/` (prompt 29's `MOD-4` rule still passes).

### 6.10 Web client **fixed** (paths, exported names); internals **convention**

`web/src/api/hooks/experience.ts` (at most 250 lines; split a `experienceKeys` helper if needed): type aliases of the generated schemas (`ResolutionSummary = components["schemas"]["ResolutionSummaryDto"]`, and so on for the other DTOs of 6.9), then

```ts
useResolutionSummary(operationId: string | undefined)                       // GET /operations/{id}/summary; enabled with an id
useSimilarOperations(source: { operation: string } | { incident: string } | undefined, limit?: number)   // GET .../similar or /incidents/{id}/similar-operations
useExperienceSearch(params: { q: string; scope: "host"; host: string } | { q: string; scope: "global" } | undefined)  // GET /experience/search; enabled with params
useEditSummaryField()                                                       // mutation: POST /operations/{id}/summary/{field}
```
Query keys follow prompt 28/30's `keys` object (verify its file and shape): add `keys.operationSummary(id)` and `keys.operationSimilar(id)` **as children of the `keys.operation(id)` prefix**, so prompt 28's `applyJournal` (which invalidates `keys.operation(aggregate_id)` for every `operation` journal row, including the new `operation.summary` kind) refreshes them with **no change to `applyJournal`**; a test proves it. `keys.experienceSearch(params)` and the incident-based similar key hang under `keys.operations` or their own root as the file's convention dictates. The mutation's `onSuccess` invalidates `keys.operation(id)` and nothing is set by hand; it does not retry. Components and pages are not part of this prompt. When a screen later calls the mutation it must report through `useActionFeedback` and show errors only through `describeError`; add the six error messages this prompt introduces to `web/src/design/messages.ts` (`that is not a summary field`, `this field is filled by Domovoi and cannot be edited`, `the operation has not finished`, `the summary changed since you opened it`, `write something: an empty value is not accepted`, `the text is longer than 1000 characters`) **only if** `describeError` maps by message text in the merged code; if it maps by error code, follow its mapping and say so in the report.

### 6.11 The terminal-state invariants the store and the twin both keep **fixed**

See section 8.2 (store rules). A summary exists only for a terminal operation; the first revision of an operation is its `outcome`; revisions are dense and append-only; source and field must agree (`SummarySource::may_write`).

## 7. Ordered implementation steps

1. **Inspect and verify the repository context; report discrepancies before editing.**
   - Open every file and symbol in section 3 and confirm it exists as described. Record moved lines and renamed types.
   - Confirm that 26, 27, 28, 29, 31 and 34 are merged: `OperationSnapshot`, `OperationWrite`, `decide`, `OperationHandle`, `EffectRunner::scoped`, `TurnOutput`, `OperationSummaryDto`, `snapshot_seq`, `Services`, `OperationCommands`, `MetaTool`, `ToolSet`, `ContextSource`, `LoopScope`, `EvidenceSource`, `FakeContextSource`, `facts_fts`. **If any is missing, stop and report.** Use merged names and list each difference.
   - List `crates/domovoi-store/migrations/` and note the next free number; confirm `schema_compat.min_reader` handling in `crates/domovoi-store/src/compat.rs:19` (this migration is additive and does not raise it).
   - Find where `decide` ends (the single place to add the post-step), where the writer turns a `Decision` into a `Transaction` (`writer/commit.rs`), how `Defer`/`FinishEffect` are expressed, and the real name of the writer command type.
   - Confirm 32 is merged (`TriageDeps.extra_context`, `Aggregator`, `LayerCaps`, `CONTEXT_GUIDE`); if it is not, stop and report. Check whether prompts 36 and 58 have merged and note what changes: 36 means its global-scope conversation passes `ExperienceAccess::global_for_owner_conversation()` (6.6); 58 removed the legacy triage path, so skip any edit marked legacy.
   - Run `grep -rn "TriageDeps {" crates` and `grep -rn "MetaTool::\|ToolSet::" crates --include=*.rs` and keep both lists. Run `grep -rn "SearchExperience\|search_experience\|resolution_summary" crates web/src` (it must find nothing relevant).
   - Run `cargo xtask arch-check` and `cargo nextest run -p domovoi-core -p domovoi-store -p domovoi-engine` once to know the baseline is green.
2. Create branch `experience-search` from the latest `overhaul-investigation`.
3. **Core, data and port.** `src/experience/` (types, pure derivation, pure query functions), `ExperienceRead`, `OperationWrite::SummaryField`, `EffectKind::DraftResolution`, `JournalKind::OperationSummary`, re-exports; update every exhaustive `match` the compiler flags (store `uow/operations.rs`, testkit `uow_operations.rs`, the crash-harness summary). Unit tests of 12.1. `cargo nextest run -p domovoi-core` green. Commit `feat(core): resolution summaries and the experience read port`.
4. **Core, the seventh meta-tool.** `MetaTool::SearchExperience`, schema, parse, counts, then edit the pinned core gates and snapshots (12.5). Commit `feat(core): add search_experience to the closed meta-tool list`.
5. **Store.** The migration (section 8.1), `fts::match_any` and `match_any_in`, `experience/` (apply, projection, reads, search, similar), the new arm in `uow/operations.rs`. SQLite tests of 12.3 including the N-1 upgrade test. Commit `feat(store): index resolution summaries for full-text search`.
6. **Testkit.** The in-memory twin, the contract suite (12.2), registration in both stores' contract runs, the crash-harness label. Run the suite against both stores. Commit `test(testkit): experience read contract suite`.
7. **Engine, writing the summary.** `decide/resolution.rs` and its call at the end of `decide`; `operations/resolution/` (pure planning, narrative, handler, backfill); the writer commands; `OperationHandle::edit_summary_field`; `OperationDeps.experience`; registration and backfill in `spawn`; audit constant. Tests of 12.4. Commit `feat(engine): draft and edit resolution summaries through the operation writer`.
8. **Engine, reading it.** `ExperienceSource` and its contract run, `ExperienceAccess`, the dispatch arm, `LoopScope.experience`, `TriageDeps.experience` with every constructor, the prompt texts, the three callers. Then the edits to existing tests listed in 12.6 and the gates of 12.5. Commit `feat(engine): search_experience and the experience context source`.
9. **Hub.** DTOs, the five handlers, the `OperationSummaryDto.resolution` field, `OperationCommands::edit_summary_field`, `ApiDeps.experience`, runtime wiring, `dev_hub` and the API harness. `cargo xtask openapi`. Tests of 12.7. Commit `feat(hub): summary, similar operations and experience search endpoints`.
10. **Web.** `cd web && npm run gen:api`, the hooks, keys, fixtures and their tests (12.8). `npm run lint`, `npm run typecheck`, the vitest files. Commit `feat(web): experience hooks and fixtures`.
11. **Docs** (section 15), `cargo xtask docs-links`.
12. **Registration** (3.3, 6.8): push the one `ExperienceSource` (as `Arc<dyn ContextSource>`) onto `TriageDeps.extra_context` where the hub builds `TriageDeps`, and make the test harnesses of 12.5 do the same. Do not touch the aggregator, `LayerCaps` or the guide.
13. **Checks and the completion report** (the standard block): `cargo fmt --all`; `cargo clippy -p domovoi-core -p domovoi-store -p domovoi-testkit -p domovoi-engine -p domovoi-hub --all-targets -- -D warnings`; `cargo nextest run` for those five crates; `cargo nextest run -E 'rdeps(domovoi-core)'` (public items changed); `cargo xtask arch-check`; `cargo xtask docs-links`; `cargo xtask docs-config` (it must report no change); the web checks.

Commit in small green steps (one-line Conventional Commits, one scope each) after steps 3, 4, 5, 6, 7, 8, 9 and 10. Each point must pass `cargo fmt --all`, clippy and the tests of the crates touched so far, and `cargo xtask arch-check`. Do not carry a red check forward.

## 8. Database and persistence

### 8.1 Migration `<next free number>_experience_search.sql` **fixed** (tables, columns, trigger and index names); wording of comments **convention**

List `crates/domovoi-store/migrations/` on your branch and take the next unused number; describe the migration by its contents everywhere (comments, docs, the report), never by number. It is additive: new tables only, nothing existing is altered or rebuilt, `schema_compat.min_reader` is **not** raised (an older binary ignores unknown tables). Enums are validated in Rust; the only `CHECK`s are formats. It needs the operation tables of the operation-aggregate migration (foreign keys to `operations`), so it must sort after it.

```sql
-- Resolution summaries: append-only field revisions (the truth) and a derived projection with a full-text index.
CREATE TABLE resolution_summary_fields (
    operation_id TEXT    NOT NULL REFERENCES operations(id),
    field        TEXT    NOT NULL,                 -- SummaryField::as_str, validated in Rust
    revision     INTEGER NOT NULL,                 -- 1-based, dense per (operation_id, field)
    value        TEXT    NOT NULL,                 -- canonical JSON, redacted before it gets here
    source       TEXT    NOT NULL,                 -- 'code' | 'model' | 'user', validated in Rust
    actor        TEXT    NOT NULL,
    created_at   INTEGER NOT NULL,
    PRIMARY KEY (operation_id, field, revision),
    CHECK (revision >= 1),
    CHECK (length(value) <= 16384)
) STRICT;
CREATE TRIGGER resolution_summary_fields_append_only_update
BEFORE UPDATE ON resolution_summary_fields
BEGIN SELECT RAISE(ABORT, 'summary revisions are append-only'); END;
CREATE TRIGGER resolution_summary_fields_append_only_delete
BEFORE DELETE ON resolution_summary_fields
BEGIN SELECT RAISE(ABORT, 'summary revisions are append-only'); END;

-- One row per operation that has a summary: the current value of each field, rewritten by the store
-- in the same transaction as every revision. Never written by anything else.
CREATE TABLE operation_resolutions (
    operation_id TEXT PRIMARY KEY REFERENCES operations(id),
    goal         TEXT    NOT NULL,                 -- copied from operations.goal (already redacted)
    outcome      TEXT    NOT NULL,                 -- ResolutionOutcome name
    verified     INTEGER NOT NULL DEFAULT 0,       -- 0 or 1
    problem      TEXT    NOT NULL DEFAULT '',      -- newest revision's text, '' when none
    cause        TEXT    NOT NULL DEFAULT '',
    fix          TEXT    NOT NULL DEFAULT '',
    subjects     TEXT    NOT NULL DEFAULT '',      -- words: core::experience::subjects_text
    contracts    TEXT    NOT NULL DEFAULT '',      -- words: core::experience::contracts_text
    tags         TEXT    NOT NULL DEFAULT '',      -- space-separated outcome_tags vocabulary
    ended_at     INTEGER NOT NULL,                 -- copied from operations.ended_at
    updated_at   INTEGER NOT NULL
) STRICT;
CREATE INDEX operation_resolutions_ended ON operation_resolutions (ended_at DESC, operation_id DESC);

CREATE TABLE operation_resolution_hosts (          -- the `hosts` field, one row per host; scopes host searches
    operation_id TEXT NOT NULL REFERENCES operations(id),
    host_id      TEXT NOT NULL,
    PRIMARY KEY (operation_id, host_id)
) STRICT;
CREATE INDEX operation_resolution_hosts_host ON operation_resolution_hosts (host_id, operation_id);

CREATE TABLE operation_resolution_subjects (       -- the `subjects` field, one row per pinned subject; finds similar operations
    operation_id TEXT NOT NULL REFERENCES operations(id),
    host_id      TEXT NOT NULL,
    check_key    TEXT NOT NULL,
    target       TEXT NOT NULL DEFAULT '',         -- '' when the subject has no target
    PRIMARY KEY (operation_id, host_id, check_key, target)
) STRICT;
CREATE INDEX operation_resolution_subjects_lookup ON operation_resolution_subjects (host_id, check_key, target, operation_id);

CREATE VIRTUAL TABLE operation_resolutions_fts USING fts5 (
    goal, problem, cause, fix, subjects, contracts, tags,
    content = 'operation_resolutions', content_rowid = 'rowid',
    tokenize = 'unicode61 remove_diacritics 0'
);
CREATE TRIGGER operation_resolutions_fts_insert AFTER INSERT ON operation_resolutions BEGIN
    INSERT INTO operation_resolutions_fts (rowid, goal, problem, cause, fix, subjects, contracts, tags)
    VALUES (new.rowid, new.goal, new.problem, new.cause, new.fix, new.subjects, new.contracts, new.tags);
END;
CREATE TRIGGER operation_resolutions_fts_delete AFTER DELETE ON operation_resolutions BEGIN
    INSERT INTO operation_resolutions_fts (operation_resolutions_fts, rowid, goal, problem, cause, fix, subjects, contracts, tags)
    VALUES ('delete', old.rowid, old.goal, old.problem, old.cause, old.fix, old.subjects, old.contracts, old.tags);
END;
CREATE TRIGGER operation_resolutions_fts_update AFTER UPDATE ON operation_resolutions BEGIN
    INSERT INTO operation_resolutions_fts (operation_resolutions_fts, rowid, goal, problem, cause, fix, subjects, contracts, tags)
    VALUES ('delete', old.rowid, old.goal, old.problem, old.cause, old.fix, old.subjects, old.contracts, old.tags);
    INSERT INTO operation_resolutions_fts (rowid, goal, problem, cause, fix, subjects, contracts, tags)
    VALUES (new.rowid, new.goal, new.problem, new.cause, new.fix, new.subjects, new.contracts, new.tags);
END;
```

Checks on this DDL the executor must make: the FTS definition keeps the literal text `content = 'operation_resolutions'` (the guard test in `tests/sqlite/purge.rs` parses it); none of the new tables is added to `PURGED_TABLES` (summaries are retained, so `FTS_CLEANUP` stays as it is); `Store::open` runs the migration on a fresh database **and** on the N-1 fixture; `PRAGMA quick_check` stays `ok`; `PRAGMA foreign_key_check` is empty.

### 8.2 Store rules (`experience/apply.rs`, `experience/project.rs`; typed errors, never a panic) **fixed**

`SummaryFieldWrite` is applied inside the unit-of-work transaction, in the order the writer put it:

1. **Source and field must agree** (`SummarySource::may_write`), else `CommitError::Constraint("source cannot write this field")`.
2. **The operation must exist and be finished**: its stored status is terminal and `ended_at IS NOT NULL`, else `Constraint("a summary needs a finished operation")`.
3. **The first revision of an operation is its `outcome`**: when the operation has no revision at all, `field` must be `outcome`, else `Constraint("the first summary revision is the outcome")`. This guarantees a projection row exists whenever any revision does.
4. **Revisions are dense and optimistic**: `expected_revision` must equal the stored maximum revision of that `(operation, field)` (0 when none), else `CommitError::Conflict` with `Expectation::Version(expected_revision)` and `Observed::Version(actual)`; success inserts `expected_revision + 1`.
5. **Values are validated** by field: `outcome` a string naming a `ResolutionOutcome`; `verified` a bool; `subjects`, `contracts_executed`, `hosts` arrays of the documented shapes (at most 8, 32 and 16 entries); narrative a JSON string of 1 to 1000 characters. A bad value is `Constraint("bad summary value")`.
6. **Project** (same transaction, after the insert; `project::refresh(conn, operation)`): read the newest revision of each field, read `goal` and `ended_at` from `operations`, `INSERT ... ON CONFLICT(operation_id) DO UPDATE` the `operation_resolutions` row (`verified` 0/1; narrative text or `''`; `subjects` and `contracts` through the core text helpers; `tags` from the write's `tags` when it is `Some`, else unchanged; `updated_at = at`). When the write was the `hosts` field, replace that operation's rows in `operation_resolution_hosts`; when it was `subjects`, replace its rows in `operation_resolution_subjects`. The FTS table is maintained by the triggers.
7. The store never recomputes meaning: it stores what it is told, once validated. It does not call `code_fields`.

The in-memory twin applies the same seven rules and keeps an equivalent projection in memory.

### 8.3 Reads **fixed** (behaviour), SQL **convention**

- `current`: newest revision per field, `SELECT ... WHERE operation_id = ? AND revision = (SELECT max(revision) FROM resolution_summary_fields WHERE operation_id = f.operation_id AND field = f.field)`, ordered in `SummaryField::ALL` order.
- `summary`: `current` plus `goal` and `ended_at` from `operations`; `None` when there is no `outcome` revision.
- `search(Host(h), text)`: the words are `core::query_words(text)`; build every FTS string from those words joined by a single space (so the 12-word and 40-character caps apply), never from the raw text. **Pass 1** `match_all`, **pass 2** `match_any` (only when pass 1 gave fewer than `limit` hits; the second pass skips operations already returned), each:
  `SELECT r.operation_id, -bm25(operation_resolutions_fts, 8.0, 6.0, 4.0, 4.0, 3.0, 3.0, 1.0) AS score, snippet(operation_resolutions_fts, -1, '', '', '…', 16) AS snippet, r.ended_at FROM operation_resolutions_fts JOIN operation_resolutions r ON r.rowid = operation_resolutions_fts.rowid WHERE operation_resolutions_fts MATCH ?1 AND <scope> AND (?3 IS NULL OR r.operation_id <> ?3) ORDER BY score DESC, r.ended_at DESC, r.operation_id DESC LIMIT ?4`.
  `<scope>` for `Host(h)` is `EXISTS (SELECT 1 FROM operation_resolution_hosts x WHERE x.operation_id = r.operation_id AND x.host_id = ?2) AND NOT EXISTS (SELECT 1 FROM operation_resolution_hosts y WHERE y.operation_id = r.operation_id AND y.host_id <> ?2)`; for `Global` it is `1`. The scope is part of the SQL; there is no post-filter in Rust that the scope depends on. An empty word list skips FTS and lists `operation_resolutions r WHERE <scope> ORDER BY r.ended_at DESC, r.operation_id DESC LIMIT ?`. Each hit's summary is loaded afterwards with `current` for the page's ids (one query, `IN (...)`).
- `similar`: candidates (a) rows of `operation_resolution_subjects` on `query.host` with the same `check_key` or, when `query.target` is `Some`, the same `target`, newest 100, and (b) the top 20 of an any-word match restricted to the columns `{goal problem cause}` and the single-host `<scope>` for `query.host`; remove `query.exclude`; load summaries; call `core::score_similar` for each with its `words_rank`; drop scores below `SIMILAR_MIN_SCORE`; `order_similar`; truncate to `limit`.
- `briefs`: one query over `operation_resolutions` for the ids.
- `undrafted`: `SELECT id FROM operations o WHERE o.status IN ('succeeded','failed','escalated','cancelled') AND NOT EXISTS (SELECT 1 FROM resolution_summary_fields f WHERE f.operation_id = o.id) ORDER BY o.ended_at, o.id LIMIT ?`.
- All user text reaches SQL only as a bound parameter, and FTS text only through `fts::match_all`, `match_any` or `match_any_in` (every word quoted).

### 8.4 Transaction boundaries

Every `WriteResolution` and `EditSummary` command is exactly one `UnitOfWork::commit`: the revision writes, the journal drafts (and the audit draft for a user edit), and the completion of the held effect when there is one. Nothing writes these tables outside a `Transaction`; a gate scan (12.5) checks that `INSERT INTO resolution_summary_fields` and `INSERT INTO operation_resolutions` appear only under `crates/domovoi-store/src/experience/`. The `DraftResolution` effect is created inside the terminal transaction of the operation itself (6.4), so a terminal operation without a draft effect or a summary can only come from before this migration, and the backfill covers it.

### 8.5 N-1 upgrade test (required)

`migration_adds_the_summary_tables_and_keeps_existing_rows` in `crates/domovoi-store/tests/sqlite/experience.rs`: copy `tests/fixtures/n1-schema5.db` into a temporary directory (the `copy_fixture` pattern of `upgrade.rs`), record the row count of every pre-existing table through a raw connection, `Store::open` it with the current binary (which applies every migration up to and including this one), then assert: every pre-existing table keeps its row count; the five new tables and the FTS table exist and are empty; `PRAGMA foreign_key_check` and `PRAGMA quick_check` are clean; `schema_compat.min_reader` is unchanged by this migration; a search and a `briefs` call on the upgraded database return empty results (known empty, no error). A second test, `an_operation_finished_before_the_migration_is_found_by_the_backfill`, inserts through a raw connection (after the upgrade) an `operations` row with a terminal status and `ended_at` and **no** summary rows, and asserts `ExperienceRead::undrafted(10)` returns it. Do not modify or regenerate the fixture.

## 9. Protocol and compatibility

- **No wire change.** `PROTOCOL_VERSION` stays 1.2; no agent file, no message, no golden frame changes (RW-42). Experience is hub-only data.
- **Model-facing protocol.** The tool list sent to a tool-capable provider grows from six to seven (investigation) and from three to four (follow-up and chat). A provider without tool calling is unchanged (it never sees tools). Stored timeline entries, cached reports and `TurnOutput` keep their shape.
- **HTTP.** Additive: five routes and one nullable field on `OperationSummaryDto`. Run `cargo xtask openapi && cd web && npm run gen:api` and commit the regenerated files; never hand-edit them.
- **Journal and effects.** `operation.summary` and `draft_resolution` are new strings. Readers already tolerate unknown journal kinds (`JournalEntry::kind` is a `String`) and the SSE `topics_of` sends every operation row to its topics. An older hub binary (a rollback) never registers `draft_resolution`, so such an effect would stay pending and harmless; the next upgrade claims it, or the backfill queues the operations that still lack a summary.
- **Schema.** Additive tables only; `schema_compat.min_reader` is not raised.
- **Config.** No new key and no new section.

## 10. Error handling and recovery

| Situation | Exact behaviour |
|---|---|
| An operation becomes terminal | the same commit creates one `DraftResolution` effect (`resolution:<id>`); a crash after the commit leaves it pending and the runner claims it after restart |
| The effect runs and the operation is missing or not terminal | `FinishEffect`, nothing written |
| The effect cannot read the snapshot or the current revisions (store error) | `Retry`; after `max_attempts` the effect is dead-lettered, the operation is unaffected, and the next start's backfill queues it again because the operation still has no revision |
| `WriteResolution` hits `CommitError::Conflict` (a revision slot was taken meanwhile) | `Retry`; the next run re-reads `current` and plans only what is missing |
| `StaleEffect` | `Handled`; nothing committed |
| The effect is delivered twice | the second run plans nothing (every code field equals its newest revision, narrative already exists) and finishes the effect; no duplicate revision |
| The investigation had no report (failed turn, skipped, a non-investigation operation) or its `output` was pruned and no `Assessment` exists | five code fields only; no narrative revision; the DTO shows no `problem`, `cause`, `fix`; the owner can still write one (`expected_revision` 0) |
| `output` is gone but `Assessment` and `Plan` steps exist | the narrative comes from their immutable intents (6.4) |
| A run settles after the operation was cancelled | one more `DraftResolution` (`resolution:<id>:<step>`); it appends a revision only for a code field whose value changed; narrative is untouched |
| The owner edits a narrative field with a stale `expected_revision` | `Stale { expected, actual }`, 409, nothing committed |
| The owner edits a code-owned field, an unknown field, an open operation, empty or over-long or control-character text | `CodeOwned` (403), `UnknownField` (400), `NotTerminal` (409), `Empty`/`TooLong`/`BadText` (400); nothing committed, no journal row, no audit row |
| The owner submits the text a `user` revision already holds | `unchanged: true`, no revision, no journal row |
| The owner submits the text of a `model` revision | a `user` revision (a confirmation) |
| Orchestration is off | edit endpoint 503 `orchestration is not enabled`; reads answer 200 with empty or `not_ready` data |
| The writer is shutting down | `OperationError::Unavailable`, 503 |
| `search_experience` with a query that has no word, is empty, longer than 200 characters or has a control character | `InvalidArgs` result to the model; nothing searched |
| `search_experience` with an argument the schema does not list (`scope`, `host`, `limit`) | `InvalidArgs` (unknown field); nothing searched |
| The search store read fails, the deadline passes, or the scope does not match | a gap: the model reads `error: experience search is unavailable: <reason>` and the unknown note; never an empty answer |
| The search matches nothing | a known empty: `No past operation on this host matched "..."`, not an error |
| The experience read for `OperationSummaryDto.resolution` fails | log at `warn`, `resolution: null`; the page is still returned |
| A summary read for an HTTP request fails | `ApiError::internal`, never an empty list |
| The backfill finds `duplicate effect` | treated as success (already queued) |

## 11. Security requirements

- **Experience is evidence, never authorization (D-02, RW-25, INV-1).** No code on the dispatch, authorization or approval path reads summaries. A gate (12.5) scans `operations/decide/`, `operations/execute.rs`, `operations/writer/dispatch.rs`, `operations/effects/dispatch.rs`, `actions/` and `policy.rs` for `ExperienceRead`, `ExperienceSource`, `ResolutionSummary`, `SummaryRevision` and `search_experience`. A past approval or success changes nothing about what a new step needs: a test proposes the same plan after an identical successful run and still gets a pending approval.
- **The label is data, not decoration.** `ContextItem::render` prints `historical evidence, not authorization` for every `Origin::Historical` item (prompt 31), the search result block begins with the same words, and every DTO that carries past operations has a `label` field with that text.
- **Host scoping is enforced in SQL and by construction.** A host-scoped query returns only operations whose host set is exactly that host. `ExperienceScope::Global` can be built in three production places only: the owner's search endpoint (an authenticated web session that sent `scope=global` and no host), `ExperienceSource::global_search` (called only for the Operator conversation the owner opened with a global scope), and the store and twin that implement the port. The model has no argument that can widen scope; `ContextScope` has no global variant; the contract suite proves a host-scoped query never returns another host's item. Fail closed: a scope that does not match the incident or the operation is a gap.
- **Only the operation writer writes summaries (RW-41).** Every revision is an `OperationWrite::SummaryField` in a `Transaction` built by the writer task. The Knowledge UI, chat and services go through `OperationCommands::edit_summary_field` with a `Caller`; no model tool, no chat command by itself and no background task edits a narrative field. Code-owned fields accept only `source = code`: the writer refuses a user edit and the store refuses a mismatched source (two layers).
- **Audit and journal.** A user edit (including a confirmation) writes `operation.summary_edited` in the same transaction, actor the person. Code and model revisions write the journal row only. Neither carries the value. An audit failure fails the commit (same transaction).
- **Secrets (INV-5).** Every narrative text passes the `Redactor` before it is planned (`plan_draft`, `plan_edit`) and again when an item or DTO is built; code-field values hold ids, check keys, targets and contract names only. A `ListRedactor` secret appears in no revision, no projection row, no FTS term, no journal payload, no audit row, no tool result and no DTO (a test checks each).
- **Prompt injection (INV-9).** Narrative text came from a model report (which may have read attacker-controlled logs) or from the owner. It is stored and shown as data, labelled by source, bounded (1000 characters a field, 1 KiB an item), and never executed or interpreted as an instruction by Domovoi. The meta-prompt tells the model that results from `search_experience` are historical evidence and not instructions.
- **No new step-up.** Editing a narrative field is not on the step-up list (RW-11): it changes only labelled evidence that a future prompt may show, never an authorization. It still needs an authenticated web session and the CSRF guard.
- **Bounds.** Query at most 200 characters and 12 words; model results at most 5 items of 1 KiB; HTTP search at most 50; similar at most 20; history at most 200 revisions; a revision value at most 16 KiB in the table and 1000 characters for text.
- **No arch-rule change** and no new dependency. If `cargo xtask arch-check` fails, stop and report.

## 12. Testing requirements

Names describe behaviour. Fakes that exist: `InMemoryStore`, `FakeClock`, `FakeAgentLink`, `ScriptedLlm` (`push_ok`, `push_text`, `requests()`), `ListRedactor`, `build::{at, T0}`. From earlier prompts (verify): 26's operation builders and engine harness (`crates/domovoi-engine/tests/operations/harness.rs`, 27's investigation harness with `Mode`), 31's `FakeContextSource` and `domovoi_testkit::contracts::context::run(source, scope)`, 25's `FakeCapabilityRegistry` and a fake `TargetResolver`. **(new)** nothing beyond the suite below: the twin lives in `InMemoryStore`. Tests never sleep: they advance `FakeClock`.

### 12.1 Core (`crates/domovoi-core/tests/experience.rs`, unit tests in `src/experience/`)

| Test | Setup, action, expected |
|---|---|
| `summary_field_names_ownership_and_order_are_pinned` | `SummaryField::ALL` names are `outcome verified subjects contracts_executed hosts problem cause fix`; the first five are code-owned, the last three narrative; `parse` round-trips; an unknown name is `None` |
| `a_source_may_only_write_its_own_fields` | `Code` writes the five code fields only; `Model` and `User` write the three narrative fields only |
| `code_fields_are_none_until_the_operation_ends` | a `Running` snapshot gives `None` |
| `code_fields_of_a_verified_fix` | hand-built snapshot: execution dispatched and `Succeeded`, `Verification` `Succeeded` with a `verifies` edge, status `Succeeded` → outcome `succeeded`, `verified: true`, one `ExecutedContract { outcome: "succeeded" }`, sorted hosts |
| `a_report_only_investigation_is_succeeded_and_not_verified` | no execution step → `outcome succeeded`, `verified false`, no contracts |
| `only_dispatched_executions_count_as_executed` | a `Refused` execution, a `NotRun` one and one that never dispatched are not listed; one still `Running` at a `Cancelled` operation is listed with outcome `running`; more than 32 are cut to 32 |
| `verified_needs_the_last_verification_to_pass_and_its_execution_to_succeed` | a failed verification then a passing one: true; a passing one then a later failing one: false; a passing verification of a failed execution: false |
| `outcome_tags_stay_inside_the_closed_vocabulary` | for every scenario of the five trace fixtures of prompt 26, each tag is in the vocabulary of 6.1, the list is sorted and deduplicated; denied, expired, refused, retried, outcome_unknown, question_asked and verification_failed each appear for a snapshot built to show them |
| `similarity_weights_are_pinned` | a table over `score_similar`: check only 8, check and target 12, target only 4, one shared contract 2 (three shared 6, a fourth adds nothing), a words rank of 0 adds 4 and of 5 adds 2 and of 20 adds 0; reasons come in the documented order |
| `similar_hits_order_by_score_then_recency_then_id` | equal scores break by newer `ended_at`, then larger operation id |
| `query_words_dedupe_and_cap` | repeated and mixed-case words collapse, order of first use kept, at most 12, each cut to 40 characters, operators and punctuation vanish |
| `effect_kinds_round_trip` and `journal_kinds_round_trip` | the existing tests, extended with `DraftResolution` and `OperationSummary` |
| `summary_wire_shapes_snapshot` | `insta` snapshot (read it before accepting) of one `SummaryRevision` per field, and of `ResolutionSummary`'s serde |
| the prompt-31 tests that pin the meta-tool list | updated as 12.5 says |

### 12.2 Shared contract suite (`crates/domovoi-testkit/src/contracts/experience.rs`; registered in `crates/domovoi-testkit/tests/contracts.rs` and in `crates/domovoi-store/tests/sqlite/contracts.rs`; operations are created with 26's builders and finished through `OperationWrite::Operation`)

The suite asserts **membership, scope, pass order and documented tie-breaks**, never raw `bm25` scores (the twin cannot reproduce them).

| Function | Expected |
|---|---|
| `first_revision_must_be_the_outcome` | writing `problem` first is `Constraint`; writing `outcome` first succeeds and `summary` is then readable |
| `a_summary_needs_a_finished_operation` | a `Running` operation refuses any revision (`Constraint`) |
| `revisions_are_dense_and_a_stale_revision_conflicts` | revision 1 then 2 succeed; a write that expects 1 when 2 exists conflicts with `Expectation::Version(1)` and `Observed::Version(2)` and changes nothing |
| `source_must_match_field_ownership` | `User` writing `outcome` and `Code` writing `problem` are `Constraint` |
| `current_returns_the_newest_revision_of_each_field_in_order` | after edits, `current` has one row per written field in `SummaryField::ALL` order; `revisions` has them all, ordered by (field order, revision) |
| `the_projection_equals_the_fold_of_the_revisions` | after a mixed sequence (code fields, model draft, user edit, a later code revision), `summary()` equals what folding `revisions()` gives |
| `search_is_host_scoped` | operations on host A, host B and one whose hosts are `{A, B}`: `Host(A)` returns only A's single-host operations; `Host(B)` only B's; `Global` returns all three. This is the base of `experience_search_is_host_scoped` |
| `search_returns_all_word_matches_before_any_word_matches` | a query of two words: the operation containing both is first, the one containing one comes after, and a hit with neither is absent; `pass` is `AllWords` then `AnyWord` |
| `search_with_no_words_lists_the_newest_first` | an empty and a punctuation-only query list by `ended_at` descending then id descending |
| `search_never_reads_user_text_as_syntax` | queries `"NEAR("`, `a OR b`, `zfs*`, `title:x`, `^x` match only as plain words and raise no error |
| `search_excludes_the_given_operation` | `exclude` removes that operation |
| `search_sees_only_the_current_text` | after a user edit replaces a narrative text, the old words no longer match and the new ones do |
| `similar_scores_check_target_contract_and_words` | candidates built to hit each weight come back with the documented reasons and order; one below `SIMILAR_MIN_SCORE` is absent |
| `similar_stays_on_one_host_and_never_returns_the_excluded_operation` | a perfect match on another host is absent; the operation named by `exclude` is absent |
| `undrafted_lists_only_finished_operations_with_no_revision` | running, drafted and undrafted-finished operations: only the last is listed, oldest first |
| `briefs_cover_only_operations_that_have_a_summary` | ids without a summary are absent from the map |

### 12.3 SQLite (`crates/domovoi-store/tests/sqlite/experience.rs`, registered in `main.rs`; unit tests in `src/fts.rs`)

| Test | Expected |
|---|---|
| `migration_adds_the_summary_tables_and_keeps_existing_rows`, `an_operation_finished_before_the_migration_is_found_by_the_backfill` | section 8.5 |
| `summary_revisions_cannot_be_updated_or_deleted` | a raw `UPDATE` and a raw `DELETE` on `resolution_summary_fields` fail with `summary revisions are append-only` |
| `the_full_text_index_passes_its_integrity_check_after_edits` | after code, model and user revisions, `INSERT INTO operation_resolutions_fts(operation_resolutions_fts) VALUES('integrity-check')` succeeds |
| `the_full_text_table_keeps_its_content_table_text` | the new virtual table's stored SQL contains `content = 'operation_resolutions'`; the existing `purged_tables_with_a_full_text_index_clean_it_in_the_same_transaction` still passes unchanged |
| `summaries_survive_pruning_of_outputs_and_journal_rows` | set `steps.output` to `NULL` and delete the operation's journal rows through a raw connection: `summary`, `search` and `similar` return the same results |
| `host_scoped_search_uses_the_host_index` | `EXPLAIN QUERY PLAN` of the host scope mentions `operation_resolution_hosts_host` |
| `match_any_quotes_every_word_and_caps_the_list` and `match_any_in_restricts_to_columns` (unit, `src/fts.rs`) | `match_any("a OR b*")` is `"a" OR "or" OR "b"`; 13 words give 12 terms; `match_any_in(&["goal","problem"], "x")` is `{goal problem} : ("x")` |
| `a_failed_commit_leaves_no_revision_projection_or_index_row` | a transaction holding a revision write and a failing audit draft (or the crash harness hook) rolls back all of them |

### 12.4 Engine: writing the summary (`crates/domovoi-engine/tests/operations/resolution.rs`, registered in that binary's `main.rs`; 26's and 27's harness)

| Test | Setup, action, expected |
|---|---|
| `every_terminal_transition_creates_exactly_one_draft_effect` | drive `Finish` (succeeded, failed, escalated), `Cancel`, `Verified { passed: true }` and the cycle-limit escalation: each leaves one pending `draft_resolution` effect with `dedup_key resolution:<id>` and payload `{"v":1,"operation":..}`; a non-terminal event and a refused event create none; the effect is not cancelled by the same commit's `EffectCancel` |
| `a_finished_investigation_gets_code_fields_and_a_model_narrative` | scripted report (`summary`, `cause`, `next_step`, two `manual_steps`), approve, dispatch, verify; run the effects: five code revisions (`source code`, actor `system:operations`) and three narrative revisions (`source model`); `problem` is the report `summary`, `fix` is `next_step` plus the manual steps on new lines; `verified true` |
| `the_narrative_costs_no_model_call` | the number of `ScriptedLlm` requests is the same before and after the effect ran |
| `a_summary_without_a_report_has_code_fields_only` | a failed turn: five code revisions, no narrative revision, the effect finishes |
| `the_narrative_falls_back_to_the_assessment_and_plan_when_the_output_is_gone` | null `steps.output` of the turn before the effect runs: `problem` and `cause` come from the `Assessment` text split at its first newline and `fix` from the `Plan` text |
| `a_redelivered_draft_effect_writes_nothing_twice` | run the handler twice on the same snapshot: revisions unchanged, the second run finishes the effect |
| `a_late_result_after_a_cancel_appends_only_the_changed_code_field` | cancel while an execution is `Running`: `contracts_executed` shows `running` (revision 1); settle it `Succeeded`: one more effect `resolution:<id>:<step>`, `contracts_executed` revision 2, `outcome` still revision 1, narrative untouched |
| `summary_text_is_redacted_everywhere` | a `ListRedactor` secret in the report's `cause` and in a user edit: no `resolution_summary_fields` value, no projection column, no FTS hit for the secret as a word, no journal payload, no audit row, no log line contains it |
| `code_owned_summary_fields_reject_user_edits` | `edit_summary_field` on `outcome`, `verified`, `subjects`, `contracts_executed` and `hosts` returns `Summary(CodeOwned(_))`; no revision, journal row or audit row is written |
| `summary_edit_goes_through_operation_writer` | one edit writes exactly one `user` revision, one `operation.summary` journal row (no value in the payload) and one `operation.summary_edited` audit row (no value) in **one** commit; a forced audit failure leaves none of the three |
| `stale_summary_edit_rejected` | two edits that both expect revision 1: the second returns `Stale { expected: 1, actual: 2 }` and changes nothing; an edit of a field with no revision expects 0 |
| `editing_an_open_operation_is_refused` | `NotTerminal`, nothing written |
| `confirming_a_model_draft_appends_a_user_revision_and_repeating_it_is_a_no_op` | same text over a `model` revision: revision 2, `source user`; the same text again: `unchanged: true`, no row |
| `edit_text_is_validated` | empty (after trim) is `Empty`; 1001 characters is `TooLong { max: 1000 }`; a control character other than newline is `BadText`; text of exactly 1000 characters is accepted |
| `the_backfill_queues_each_undrafted_finished_operation_once` | two finished operations without a summary, one drafted, one running: start the service twice; exactly two effects were queued the first time and none the second |
| `a_summary_survives_a_restart_between_the_terminal_commit_and_the_draft` | finish an operation, stop the service before the effect runs, start it again: the effect is claimed and the summary appears |
| `past_success_does_not_change_what_a_new_step_needs` | a finished operation with an approved, succeeded, verified execution; propose the identical execution in a new operation: it gets a **pending** Approval step and is not authorized |

### 12.5 Engine: the meta-tool, the source and the gates (`crates/domovoi-engine/tests/services/experience.rs` and `tests/gates.rs`; core `tests/gates.rs`)

| Test | Setup, action, expected |
|---|---|
| `search_experience_is_offered_after_request_evidence` | `MetaTool::ALL` names in order: `list_capabilities describe_capability request_evidence search_experience propose_plan ask_user finish`; `ToolSet::Read` is the first four; `Investigate { questions: false }` is six, `{ true }` seven; the `search_experience` schema has only `query` and is a closed object |
| `experience_search_is_host_scoped` | operations finished on host A and host B; a scripted investigation on host A calls `search_experience`; the result contains only A's operations; the `ToolSpec` the provider received has no `scope`, `host` or `limit` property |
| `the_model_cannot_widen_the_scope` | calls with `{"query":"x","scope":"global"}`, `{"query":"x","host":"nas"}`, `{"query":"x","limit":50}`: each is an `InvalidArgs` result (unknown field); nothing was searched (the fake read saw no query) |
| `experience_block_labelled_not_authorization` | the result starts with `Historical evidence, not authorization.`; every item renders the origin text `historical evidence, not authorization`; the next request to the provider contains that text |
| `search_results_carry_operation_id_date_verified_and_field_sources` | an item shows `operation: <uuid>`, `ended: <RFC 3339>`, `verified: yes|no` and `problem [model]`, `cause [user]`, `fix [model]` as written |
| `search_returns_at_most_five_items_of_one_kib` | seven matching operations with long narratives: five items, each body at most 1024 bytes, `truncated` set where cut, the whole result at most `MAX_RESULT_BYTES` |
| `an_empty_result_is_a_known_empty_and_a_failed_read_is_unknown` | no match: `No past operation on this host matched "..."`, `is_error false`; a failing fake: `error: experience search is unavailable: ...` plus the unknown note, `is_error true` |
| `an_operation_never_finds_itself` | a loop run inside operation X never returns X |
| `a_malformed_query_is_refused_before_searching` | empty, 201 characters, a control character, `"***"`: each `InvalidArgs`; the fake read saw nothing |
| `the_experience_source_satisfies_the_context_contract` | `contracts::context::run(&ExperienceSource, scope)`: bounded, redacted (`ListRedactor`), unknown is not empty, deadline gap, stable ids |
| `a_registered_experience_source_appears_in_every_context_bundle` | with the source pushed onto `extra_context`, `context::build` (investigation), the follow-up bundle and the chat bundle of a host contain the heading `Historical evidence, not authorization: experience`; with a finished similar operation on the host the section holds its item (an investigation for the same check), with none it reads `None.`; an operation on another host never appears |
| `the_experience_section_stays_small` | seven similar operations with long narratives: the overview holds at most 3 items of at most 1024 bytes each, and `LayerCaps::DEFAULT` was not edited |
| `experience_items_are_always_historical` | every item has `Origin::Historical`, `ItemStatus::Known`, a reference `operation:<uuid>`, layer `Experience`, and the serialised keys are exactly `id, source, layer, title, body, status, provenance, truncated` |
| `the_incident_overview_returns_similar_operations_and_a_mismatch_is_a_gap` | `Incident { host, incident }` with `Overview`: similar operations of that incident; an incident of another host, a missing incident and a past deadline each give a gap and no item |
| `global_search_returns_other_hosts_with_names_only_through_the_owner_path` | `ExperienceSource::global_search` returns operations of two hosts with a `host:` line; a host-scoped `fetch` never does |
| `the_meta_prompt_names_search_experience_and_the_label` | `META_SYSTEM_PROMPT`, the follow-up prompt and the chat prompt each name `search_experience`; the meta prompt contains `historical evidence, not authorization`; the prompt-31 rendering test for the `ask_user` marker still passes |

**Gates (extend; never delete or weaken).**
- Core `crates/domovoi-core/tests/gates.rs`: **`model_meta_tools_are_closed`** now pins the seven names in the order above; none contains `approve`, `exec`, `write`, `delete`, `restart`, `run_skill`, `shell`, `command`, `authorize`, `permit` or `dispatch`; `ALL.len() == 7`; every schema is a closed object; `ToolSet::Read` is a subset of `ALL` without `propose_plan`, `ask_user`, `finish`; the exhaustive `match` over `MetaToolCall` in the test lists exactly seven variants. `read_tool_catalogue_is_read_only` is untouched.
- Engine `crates/domovoi-engine/tests/gates.rs`: **`model_meta_tools_are_closed`** (if the engine has its own copy) is updated the same way; `model_sees_only_meta_tools` needs no edit (it compares with `ToolSet::tools()`) but must still pass with seven and four; `model_calling_approve_or_unknown_tool_changes_nothing` is unchanged. New:
  - `experience_never_reaches_authorization_or_dispatch`: no line of `src/operations/decide/` (apart from `resolution.rs`, which names none of these anyway), `src/operations/execute.rs`, `src/operations/writer/dispatch.rs`, `src/operations/effects/dispatch.rs`, `src/actions/` or `src/policy.rs` contains `ExperienceRead`, `ExperienceSource`, `ResolutionSummary`, `SummaryRevision` or `search_experience`.
  - `experience_global_scope_has_known_callers`: in the production sources of the engine, `ExperienceScope::Global` appears only in `src/context/experience.rs`, and `global_for_owner_conversation(` is called nowhere outside its definition in `src/triage/meta/experience.rs` — or, **if prompt 36 has merged**, exactly once, in the operator conversation's module (name that path in the test).
  - `summary_revisions_have_one_writer`: `SummaryFieldWrite {` is constructed in production only in `src/operations/writer/summary.rs`; no string `INSERT INTO resolution_summary_fields` or `INSERT INTO operation_resolutions` appears in the engine or hub sources.
  - The prompt-31 gates `meta_tools_cannot_reach_the_writer_or_an_authorization` and `the_loop_never_stores_a_plan` must pass **unchanged** with the new files in `src/context/` and `src/triage/meta/` (so `experience.rs` never names `OperationHandle`, `OperationCommand`, `Authorization`, `.propose(` and the like).
- Hub `crates/domovoi-hub/tests/gates.rs`: `experience_global_scope_is_built_only_by_the_owner_search_endpoint` (`ExperienceScope::Global` appears only in `src/api/experience.rs`, once); `summary_edit_has_one_production_call_site` (`.edit_summary_field(` on an `OperationHandle` only in `src/services/operations.rs`). The arch rules `MOD-1` to `MOD-4` still pass.
- Drill probe `only_meta_tools` (prompt 31) passes with seven; no threshold, score weight or YAML case changes.

### 12.6 Edits to existing tests (nothing else may change; list each in the report)

1. The prompt-31 tests that pin six tools and three read tools become seven and four: `meta_tool_names_and_order_are_pinned`, `read_set_is_the_first_three_and_investigate_depends_on_questions` (rename it to say "first four"), `every_meta_tool_schema_is_a_closed_object`, `meta_tool_schemas_snapshot` (review the new snapshot), `from_json_parses_each_tool` (add a case), and every exhaustive `match` the compiler flags. If prompt 36 has merged, its `the_operator_model_is_offered_only_read_tools` is already phrased through `ToolSet::Read.tools()`; check it still passes.
2. The five `TriageDeps` constructors gain `experience`; the test harnesses build an `ExperienceSource` over the in-memory store.
3. Every `grep -rn "ToolSet::\|MetaTool::ALL\|MetaTool::READ" crates` assertion that hard-codes a count.
4. `effect_kinds_round_trip` and `journal_kinds_round_trip` gain a variant; the in-memory and SQLite `StateWrite` summaries (`crash/mod.rs`) gain one label.

### 12.7 Hub (`crates/domovoi-hub/tests/hub/experience_api.rs`, registered in `main.rs`; the operation harness with orchestration on, `FakeClock`, `ListRedactor`)

| Test | Expected |
|---|---|
| `the_summary_endpoint_reports_not_ready_drafting_and_ready` | open operation → `not_ready`, no fields; finished with no revision → `drafting`; after the effect → `ready` |
| `the_summary_shows_every_field_with_its_source_and_history` | eight fields in `SummaryField::ALL` order with `source`, `actor`, `revision`, `created_at`; narrative fields `editable: true`, code fields `false`; `history` oldest first; `as_of_seq` read before loading |
| `editing_a_narrative_field_returns_the_new_revision_and_is_audited` | POST returns `EditSummaryResponse` with `source "user"`, the audit page lists `operation.summary_edited`, and a second client's `GET` sees revision 2 |
| `a_stale_edit_is_409_and_a_code_owned_edit_is_403_and_an_unknown_field_is_400` | exact statuses and the texts of 6.9 |
| `editing_needs_a_session_and_the_csrf_guard` | no session 401; a missing or wrong CSRF token is refused like the other POSTs |
| `editing_is_503_when_orchestration_is_off_and_reads_are_empty` | edit answers `orchestration is not enabled`; summary, similar and search answer 200 with empty data |
| `similar_operations_of_an_operation_exclude_itself_and_stay_on_its_host` | scores and reasons as built; no item from another host; the operation itself absent |
| `similar_operations_of_an_incident_use_its_check_target_and_title` | the incident route returns the operation that fixed the same check |
| `owner_search_is_host_scoped_by_default` | `scope=host` without `host` → 400; with `host=nas` → only nas operations; an unknown host → 404 |
| `global_search_needs_scope_global_and_no_host` | `scope=global` returns both hosts' operations with `HostRefDto` names; `scope=global&host=nas` → 400; `scope=everywhere` → 400 |
| `every_past_operation_response_carries_the_label` | summary-adjacent, similar and search bodies have `label == "historical evidence, not authorization"` |
| `operation_list_rows_carry_the_resolution_brief_and_survive_a_failed_read` | rows of finished operations have `resolution { outcome, verified, ended_at }`, open ones `null`; with `briefs` failing the page is still 200 with `null`s and a `warn` log |
| `the_openapi_document_lists_the_new_routes` | the existing "OpenAPI is current" test passes with the five operation ids |

### 12.8 Web (`web/src/api/hooks/experience.test.ts`; fixtures `web/src/test/fixtures/experience/{summary-ready,summary-drafting,similar,search-host,search-global}.json`)

`the_hooks_call_the_documented_paths_and_are_disabled_without_input` (fetch mock: URL, query string, `enabled`); `each_fixture_has_the_generated_type_and_the_label` (typecheck plus `label` equality); `a_frame_for_operation_summary_refreshes_the_summary_and_similar_queries` (call prompt 28's `applyJournal` with an `operation` frame of kind `operation.summary` and assert the summary and similar keys are invalidated through the `keys.operation(id)` prefix); `the_edit_mutation_invalidates_the_operation_prefix_and_does_not_retry`. Run `npm run lint`, `npm run typecheck` and the vitest files.

## 13. Acceptance criteria

1. Every operation that reaches a terminal state has exactly one `DraftResolution` effect created in its terminal commit, and after the effect runs `ExperienceRead::summary` returns its five code fields (`source code`) and, when the investigation produced a report, `problem`, `cause` and `fix` (`source model`), with no extra model call.
2. `resolution_summary_fields` is append-only (a raw `UPDATE` or `DELETE` fails), revisions are dense per `(operation, field)`, and `operation_resolutions` always equals the fold of the revisions (`the_projection_equals_the_fold_of_the_revisions`).
3. `code_owned_summary_fields_reject_user_edits`, `summary_edit_goes_through_operation_writer` and `stale_summary_edit_rejected` pass; a user edit writes its revision, `operation.summary` journal row and `operation.summary_edited` audit row in one commit, with no value in the journal or audit.
4. `search_experience` is the fourth tool in `MetaTool::ALL`, the seventh variant, in the RW-07 order; `model_meta_tools_are_closed` (core, and engine if it has a copy) pins seven names and passes; `ToolSet::Read` is exactly the first four tools; `model_sees_only_meta_tools` and the drill probe pass.
5. `search_experience` takes only `query`; an extra `scope`, `host` or `limit` argument is refused and nothing is searched (`the_model_cannot_widen_the_scope`).
6. `experience_search_is_host_scoped` passes at three levels: the shared store suite (`search_is_host_scoped`), the meta-tool loop and the HTTP endpoint. A host-scoped query never returns another host's operation, nor an operation whose host set is not exactly that host.
7. `experience_block_labelled_not_authorization` passes: the result block starts with `Historical evidence, not authorization.`, every item renders the origin `historical evidence, not authorization`, and every DTO of past operations has `label == "historical evidence, not authorization"`.
8. A model search returns at most 5 items of at most 1024 bytes each, each with operation id, `ended` date, `verified` flag and the source of every narrative field.
9. The experience `ContextSource` has `Origin::Historical` items only, passes the shared context contract suite, and holds only read ports.
10. A past approval or success changes nothing about what a new step needs (`past_success_does_not_change_what_a_new_step_needs`), and the gate `experience_never_reaches_authorization_or_dispatch` passes.
11. `ExperienceScope::Global` is built only where 11 allows and the three gates (`experience_global_scope_has_known_callers`, the hub one, `summary_revisions_have_one_writer`) pass; `global_for_owner_conversation` has no production caller unless prompt 36 has merged.
12. Every narrative text, tool result and DTO is redacted: a `ListRedactor` secret appears in no revision, projection column, FTS term, journal payload, audit row, tool result or response (`summary_text_is_redacted_everywhere`).
13. Summaries survive output and journal pruning (`summaries_survive_pruning_of_outputs_and_journal_rows`), no new table is in `PURGED_TABLES`, and the purge guard test passes unchanged.
14. The migration runs on a fresh database and on `n1-schema5.db`, keeps every existing row, leaves `schema_compat.min_reader` unchanged and passes `quick_check` and `foreign_key_check` (`migration_adds_the_summary_tables_and_keeps_existing_rows`).
15. The five HTTP routes behave as 6.9 states, call `snapshot_seq` before loading, and the edit goes through `OperationCommands::edit_summary_field`; `OperationSummaryDto.resolution` is filled and a failed brief read does not fail a page.
16. The web hooks, fixtures and tests exist; the summary and similar queries refresh through the existing `keys.operation(id)` invalidation with no change to `applyJournal`; `web/src/api/openapi.json` and `schema.d.ts` were regenerated, not edited.
17. A hub restart between the terminal commit and the draft loses nothing, and the backfill queues each finished operation without a summary exactly once.
18. No new dependency, no config key, no wire change, no arch-rule edit, no edit to a merged migration; `cargo xtask arch-check` passes; no committed file mentions prompts, the build plan, agents or AI assistance.

## 14. Migration and compatibility

- **Existing installs.** The migration is additive and runs at the next start (a pre-migrate backup is taken by the existing mechanism). Operations that finished before it have no summary; the start-up backfill queues a `DraftResolution` for each, drafted from the data that is still there (so the narrative may come from the `Assessment` and `Plan` steps when raw outputs were already pruned, or be absent). With `[orchestration] enabled = false` (the default until the legacy path is removed) there are no operations and nothing is written; every search is a known empty.
- **Legacy actions and incidents** never get a summary (they are never converted); `incident_history`, `notes_search` and the existing similar-incidents context keep covering them.
- **Agents.** No change; no protocol change; no agent upgrade.
- **Rollback.** An older hub binary ignores the new tables, does not register `draft_resolution` and does not know `search_experience`; the data stays and is used again after the upgrade.
- **Backups and deletion.** Summaries are retained like operations. They live in the database and therefore in backups until those rotate; the book says so.
- **Model providers.** The tool list sent to a tool-capable provider gains one entry. A provider with no tool calling is unaffected.

## 15. Documentation requirements

- **Book (`docs/book/src/`):** add `concepts/experience.md` and list it in `SUMMARY.md` under Concepts, in plain language and without internal ids: what a resolution summary is, which fields Domovoi fills and which the model drafts, how to confirm or edit a narrative field, why a summary can take a moment to appear, how similar operations and the search work (one host by default; global only when you ask), and that **a past approval or success never lets anything run without your approval**. If prompt 26 created `concepts/operations.md`, add a short link to the new page there.
- **`reference/data-model.md`:** rows for the five new tables and the index; in the Retention table (`:68`) add "Resolution summaries: retained with the operation; they hold no raw output"; state that deleting data does not remove it from backups until they rotate.
- **`reference/api.md`:** the five routes in one short paragraph; the OpenAPI document is the reference.
- **`ARCHITECTURE.md`:** add a `### 5.x Experience` section (the types, the one writer, the projection, the host-scoping rule, in six to ten lines; use the next free number), an `ExperienceRead` row in the §6 port table, the new tables in the §9 table list, the five routes in §10, and the seven-tool list and the experience source in §11. Do not change §8 (the wire).
- **Config reference:** run `cargo xtask docs-config`; it must report no change (no key was added). Say so in the report.
- **OpenAPI:** `cargo xtask openapi && cd web && npm run gen:api`; commit the regenerated files.
- **Links:** `cargo xtask docs-links` must pass. No ADR: no arch rule or crate edge changes. If one seems needed, stop and report.
- Do not mention prompts, prompt numbers, the build plan, agents or AI assistance in any committed file, commit message or PR description. Write "coming in a later release" rather than naming an internal id.

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
