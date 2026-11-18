# 34 — Knowledge store

| | |
|---|---|
| Branch | `knowledge-store` |
| Milestone | M5 |
| Depends on (must be merged) | 29 (use-case layer; see "Hub use-case layer" below), 32 (layered context: `TriageDeps.extra_context` and the `ContextSource` contract suite, for section 4.1), 31 (the `ContextSource` port), 11 (incident writes through the unit of work; the unit of work, journal and audit chain it builds on come from 08, 09, 10 and 11), 27 (Investigate operation; **this prompt imports no code from 27**: the dependency is the roadmap's, M5 follows M3, and 27 itself needs 26, 25 and 24, which are written but not merged. Merge them first, as the index orders). Also needs 57 (configurable retention, merged) |
| Can run in parallel with | 31, 32, 36 (order-flexible, never concurrent; 34 adds a migration, so it runs after any earlier migration-adding prompt has merged) |
| Produces (shared contracts) | The knowledge store and API: `domovoi_core::knowledge::*` (`SubjectRef`, `Predicate`, `Fact`, `Relation`, `Entity`, `KnowledgeStatus`, `FactSource`, `EvidenceRef`, `PathConfirmation`), the ports `KnowledgeRead` and `ManagementCoverage` (the typed hook prompt 47 implements), `StateWrite::Knowledge`, `domovoi_engine::knowledge::KnowledgeService`, `domovoi_core::canonical_cidr`, tables `metric_samples`, `facts`, `facts_fts`, `relations`, `entities`, `management_path_confirmations`, and the `/api/v1/knowledge/*` endpoints |
| Consumes | 08 `UnitOfWork`, `Transaction`, `StateWrite`, `JournalDraft`, `AuditDraft`, `Versioned`; 09 `Actor` and the chained audit; 57 `RetentionPolicy` and the `retracted_facts_days` key |
| Launch gate | none. Research item R-10 (helper attestation) is **not** resolved by this prompt and is not required for it: management-path confirmation is fail-closed until prompt 47 supplies a real `ManagementCoverage` |
| Suggested model | medium |


## Hub use-case layer (read first)

Prompt 29 (`use-case-layer`) runs before this prompt in the recommended order and changes how hub code is
organised. Check `crates/domovoi-hub/src/services/` on your branch. If it exists:

- Every command (anything that writes) is a method on a service in `crate::services`, taking `&Caller`.
  The HTTP handler in `api/` and any chat handler in `auth/interactions/` only parse the request, call that
  method and map the result. Plain reads may stay in `api/` handlers.
- Do not add an engine handle (`ActionHandle`, `OperationHandle` or a new service handle of yours) to
  `ApiDeps` or `ChatDeps`; give it to `Services` instead. The arch rules `MOD-1`…`MOD-4` reject the
  other form.
- A new top-level hub module must be listed in `ZONES` or `UNZONED_MODULES` in `xtask/src/arch/rules.rs`.
- Where this brief says "add `X` to `ApiDeps`" for a handle used by a command, read it as "add `X` to
  `Services`". Read-only dependencies may still go on `ApiDeps`.

If `services/` does not exist, prompt 29 is not merged: stop and report, because the order is wrong.

## 1. Objective

Give Domovoi a typed, provenance-carrying **knowledge store** that is separate from inventory and from metrics:

1. Rename the misleadingly named `digest_facts` table to `metric_samples` (it holds metric samples) and keep a view under the old name for one release.
2. Add `facts`, `relations` and `entities` with the lifecycle of `04` §10: source, status, freshness, supersession, disputes, retraction and a retention-driven content purge.
3. Back-fill the generated host-profile notes into `discovered` facts whose `evidence_ref` is `note:<id>`, keep the note, and label it superseded.
4. Store **management paths** (D-28) as facts with owner-confirmation provenance, and expose a typed, fail-closed hook (`ManagementCoverage`) where prompt 47 will plug in the verified helper attestation. **This prompt implements no attestation.**
5. Ship the knowledge HTTP API (list, search, assert, accept, retract, verify, confirm, relations, entities) and regenerate the OpenAPI document.

Out of scope: any web page (prompt 48), experience search and resolution summaries (prompt 35), and layered model context (prompt 32). See section 5.

## 2. Architectural context

Plain-language summary of the decisions this prompt implements. Everything essential is here.

- **RW-14 (knowledge over existing inventory).** Baselines (`baselines` table) remain the owner of observed containers, units and ports. A fact or relation *references* inventory through a typed subject ref (`host:<uuid>`, `container:<host uuid>/<name>`, `unit:<host uuid>/<name>`, `entity:<uuid>`) and never copies inventory rows. A small `entities` table exists only for subjects the owner invents (a "VPN", a "NAS pool"). A fact has a `source`, a `status`, `observed_at`, `valid_until`, `last_verified_at`, a `supersedes` link and an evidence reference. **There is no numeric confidence.** Predicates are a closed, Rust-validated enum.
- **04 §10 (lifecycle).**
  - Freshness: every predicate has a staleness period. Past it a fact is `stale`, computed on read and swept by a periodic job.
  - Conflicts: different values for a single-valued predicate from different sources make both facts `disputed` until the owner chooses. `inferred` never becomes `user` without an explicit owner action.
  - Safety-critical predicates (`management_path`, `trusted_cidr`) used by policy hard limits must be `current` **and** `user`-sourced.
  - Retraction: status `retracted`; the FTS row is removed in the same transaction; the value is cleared after the tombstone period; the tombstone stays.
  - Facts sent to a model carry source and status; `disputed` and `stale` are labelled; retracted facts are never sent.
- **D-21 / RW-28 (retention).** Retracted fact content is purged after 30 days (configurable key `retention.retracted_facts_days`, already declared by prompt 57 as "applies once knowledge facts exist"; this prompt makes it apply). Deletion removes data from the live database and from FTS. Backups keep it until they rotate out.
- **Ownership (04 §2).** The knowledge service owns facts, relations and entities. It is a request/response service, not a task with a channel. It commits through the same `UnitOfWork` as every writer, with optimistic versions, journal entries and (for owner actions) audit entries in the same transaction. Resolution-summary fields belong to the operation writer (RW-41, prompt 35), not to this service.
- **D-28 / RW-37 (management paths).** Unattended containment on a host needs owner-confirmed, current management paths. A path can be confirmed **only when a verified, fresh helper attestation shows it is covered**; the hub never trusts an agent-supplied description of helper state. The attestation, key pinning and report parsing are prompt 47 (H1). **This prompt stores management paths and the owner's confirmations, and defines the port through which coverage will be checked; the only implementation it ships always answers "cannot be verified", so no confirmation can succeed in production until prompt 47.** Monitoring and human-approved work are unaffected.
- **Trust boundaries.**
  - Only an authenticated human (`Actor::User`) may write a `source = user` fact. The service refuses it for every other actor. The HTTP API forces `source = user` and takes the actor from the session, never from the request body.
  - Model- or operation-derived knowledge is `inferred` and is written by a system actor. It becomes `user` only through the owner's `accept` action.
  - A value placed in a journal entry or audit entry would survive the 30-day purge forever, so **journal and audit payloads never contain a fact value or a relation's endpoints**, only ids, predicate and subject.
  - Past approvals or facts never satisfy policy by themselves: a fact is advisory input. Nothing in this prompt lets a fact authorize an action.

### Decisions this prompt makes (the docs list only "initial" predicates)

| Decision | Choice |
|---|---|
| Profile backfill predicate | The docs say the backfill writes `discovered` facts but the initial predicate list has none that fits a machine description. This prompt **adds the predicate `host_platform`** (value: hostname, os, kernel, arch, cpu count, memory bytes) and nothing else. Containers, ports and skills in a profile are inventory and are **not** copied (RW-14). |
| Rename mechanics | `CREATE TABLE metric_samples`, copy rows keeping ids, drop `digest_facts`, create a **view** `digest_facts` plus an `INSTEAD OF INSERT` trigger, so the previous release can still insert and read. `MIN_READER` is not raised. The Rust port `FactRepo` is renamed `MetricSampleRepo`; the wire message `digest_facts` and the type `DigestFact` keep their names (wire 1.2 is frozen, RW-42). |
| Entity kind | A validated lowercase slug (`[a-z0-9_-]{1,32}`), not a closed enum: the docs give no list. |
| Staleness periods | Fixed in section 6.3, one table. |
| Network-change staleness | The service method and its test exist. **Wiring an automatic trigger is not done here**: the hub keeps no history of agent addresses, and the authoritative signal (a change of the helper attestation's `generation`) belongs to prompt 47. Say so in the PR. |
| Relation conflicts | None. One live row per (from, kind, to); a repeated record refreshes it. Relations are never `disputed`. |

## 3. Repository context

Verified against the checkout (branch `overhaul-investigation`, migrations end at `0011_plan_binding.sql`). Re-verify every line in step 1.

**Digest facts (to be renamed)**

- Table `digest_facts` and its index: `crates/domovoi-store/migrations/0001_init.sql:237-246` (`id INTEGER PRIMARY KEY AUTOINCREMENT, host_id, key, label, value, unit, observed_at`, index `digest_facts_host_observed`). No later migration touches it.
- Port: `FactRepo` at `crates/domovoi-core/src/ports/repos/knowledge.rs:103` (`record`, `list(host, from, until)`), exported in `crates/domovoi-core/src/ports/repos/mod.rs:30`.
- SQLite impl: `impl FactRepo for Store` in `crates/domovoi-store/src/repos/knowledge.rs` (around line 260; the SQL names `digest_facts` twice).
- The domain type `DigestFact`: `crates/domovoi-core/src/digest.rs:21`. The wire message `DigestFacts` / kind `"digest_facts"`: `crates/domovoi-proto/src/messages/mod.rs:116-117`. **Leave both alone.**
- Users of `FactRepo` (all must be renamed to `MetricSampleRepo`): `crates/domovoi-hub/src/agent_server/mod.rs:35,75`, `crates/domovoi-hub/src/api/mod.rs:43,149`, `crates/domovoi-hub/src/digest.rs:8,21`, `crates/domovoi-hub/src/runtime/schedulers.rs` (the `FactRepo::list` call near line 461), `crates/domovoi-hub/tests/hub/api_admin.rs:3`, `crates/domovoi-hub/tests/hub/sessions.rs:3`, `crates/domovoi-testkit/src/contracts/knowledge.rs` (`fact_repo_contract`), `crates/domovoi-testkit/src/repos/knowledge.rs`, `crates/domovoi-store/tests/sqlite/contracts.rs` (`sqlite_fact_repo_satisfies_contract`). Field names `facts: Arc<dyn FactRepo>` in `AgentServerDeps`, `ApiDeps`, `DigestDeps` **keep their names** (dozens of constructors); only the type and the doc comments change.
- Docs that name the table: `ARCHITECTURE.md:262` (wire table, stays) and `docs/book/src/reference/data-model.md:35`.

**Host profile notes (to be back-filled)**

- `crates/domovoi-hub/src/profile/mod.rs`: `GENERATED_TAG = "generated"` (line 38), `PROFILE_TAG = "host-profile"` (41), `ProfileDeps` (140), `profile_key(host)` = `host.<id>.profile` (160), `stored(deps, host)` reads the JSON profile from `SettingsRepo`, `refresh(deps, host)` collects, renders, upserts the note through `NoteRepo` and saves the profile.
- `HostProfile` carries `facts: ProfileFacts { hostname, os, kernel, arch, cpu_count, mem_total_bytes, boot_time }`, `collected_at`, and `note: Option<NoteId>` (the generated note's id, reused on refresh). The note is built in `refresh` with tags `[PROFILE_TAG, GENERATED_TAG]` and title `Host profile: <host name>`.
- The note's first lines: `GENERATED_LINE` at `crates/domovoi-hub/src/profile/render.rs:30` (`"_Generated by Domovoi. It is replaced when the profile refreshes; write your own notes separately._"`), used at line 167. The test `crates/domovoi-hub/tests/hub/profile.rs:525` asserts `body.contains("_Generated by Domovoi.")`, so the new text must keep that prefix.
- `ProfileDeps` is constructed in `crates/domovoi-hub/src/runtime/mod.rs:294`, `crates/domovoi-hub/examples/dev_hub.rs:477`, `crates/domovoi-hub/tests/hub/api_harness.rs:437`, and `crates/domovoi-hub/tests/hub/profile.rs:143,354`.
- Notes and their FTS: `notes` and `notes_fts` (external-content FTS5 with triggers) in `0001_init.sql`; `notes.version` from `0006_unit_of_work.sql`.

**Unit of work (consumed from 08/09)**

- `crates/domovoi-core/src/ports/uow/transaction.rs`: `AggregateKind { Incident, Action, Note }` (line 15), `AggregateRef` (49) with `incident/action/note` constructors, `Expected { New, Version(u64) }` (93), `Versioned<T>` (151), the closed enum `StateWrite` (164), `Transaction` (209) with builders `write/journal/audit`, `CommitReceipt { journal_seqs, audit_seqs, versions }` (297), `CommitError { Conflict{aggregate, expected, found}, Constraint, StaleEffect, Backend }` (309).
- `crates/domovoi-core/src/ports/uow/journal.rs`: `JournalKind { IncidentTimeline, ActionTransition }` (15), `JournalDraft` (45), `AuditDraft::new(actor, at, kind, subject, detail)`.
- SQLite apply: `crates/domovoi-store/src/uow/writes.rs` (`apply` dispatch, 319 lines), `uow/mod.rs` (one `BEGIN IMMEDIATE` on the single writer), `uow/journal.rs`. In-memory twin: `crates/domovoi-testkit/src/repos/uow_apply.rs`, `uow.rs`, with state in `repos/mod.rs`.
- `crates/domovoi-core/src/actor.rs`: `Actor { User(UserId), System(String), Agent(HostId), Policy(String) }`, constants `SYSTEM_INCIDENTS`, `SYSTEM_ACTIONS`, `SYSTEM_AUDIT`; names match `[a-z0-9._-]{1,64}`.
- Ids are `define_id!` newtypes (UUIDv7) in `crates/domovoi-core/src/id.rs`.
- Address helpers in `crates/domovoi-core/src/addr.rs`: `valid_cidr`, `in_cidr` only. There is **no canonical-form function** (04b §1 defines the canonical CIDR: host bits zeroed, IPv4 dotted, IPv6 RFC 5952 lowercase). This prompt adds `canonical_cidr`.

**Store conventions**

- `Store` has `write` (single writer pool) and `read` pools (`crates/domovoi-store/src/db.rs:56-58`); foreign keys are on (`db.rs:100`).
- Codecs: `ms`, `from_ms`, `json`, `from_json`, `name`, `from_name`, `parse`, `Cursor` and `paginate` for keyset paging (`crates/domovoi-store/src/codec.rs`); FTS query building `fts::match_all` (`src/fts.rs`); note search with `bm25` and `snippet` (`src/repos/notes.rs:186-215`).
- Retention: `RetentionPolicy` (6 fields), `PurgeReport`, `Store::purge` (one transaction) in `crates/domovoi-store/src/purge.rs`; `PURGED_TABLES` and `FTS_CLEANUP` guard. The guard test `purged_tables_with_a_full_text_index_clean_it_in_the_same_transaction` in `crates/domovoi-store/tests/sqlite/purge.rs:~425` reads every virtual table's stored SQL and requires the text `content = '<table>'`; the new FTS table must be written in that exact form.
- Config: `RetentionSection` and `RETENTION_KEYS` in `crates/domovoi-config/src/hub/retention.rs`. `retracted_facts_days` exists (default 30) with `applies: Applies::Once("knowledge facts exist")` (line ~141) and a doc comment saying "Not applied yet". Tests there: `retention_registry_marks_the_unbuilt_keys` (expects three unbuilt keys). The hub maps the config in `crates/domovoi-hub/src/runtime/settings.rs:112-124` (`retention()`), and its test `declared_retention_keys_are_applied_to_nothing` (~line 225) lists `retracted_facts_days` among keys that change nothing. The purge runs in `DailyJob::Retention` at `crates/domovoi-hub/src/runtime/schedulers.rs:~373`.
- Migrations present: `0001_init.sql` … `0011_plan_binding.sql`. `MIN_READER` and its rule ("raise it for a drop, a rename or a changed meaning"): `crates/domovoi-store/src/compat.rs:19`.
- N-1 fixture: `crates/domovoi-store/tests/fixtures/n1-schema5.db` (migrations 0001-0005 only, cannot be regenerated); tests copy it, optionally write rows through a raw `sqlx` connection, then `Store::open` (`crates/domovoi-store/tests/sqlite/upgrade.rs`).

**Hub API conventions**

- Handlers use `State(state): State<ApiState>`, `Authed(current): Authed` (session; `current.user.id` is the `UserId`), `utoipa::path` annotations and `ApiError` (`crates/domovoi-hub/src/api/error.rs`: `BadRequest`, `Conflict`, `Forbidden`, `NotFound`, `TooMany`, `Unavailable`, `Internal`; `From<StoreError>` exists). Routes are listed in `routes()` in `crates/domovoi-hub/src/api/mod.rs:~320-390`; `ApiDeps` at line 135; `page()` helper at 266; `host_named` at ~275. DTOs live in `dto*.rs`.
- Step-up pattern for a password-protected change: `ConnectInfo<SocketAddr>` + `HeaderMap`, `proxy::client_ip(peer, &headers, &state.auth.web().trusted_proxies)` and `state.auth.confirm_password(ip, user_id, password)`; `AuthError::LockedOut{retry_after_secs}` maps to 429 and any other error to 403 (`crates/domovoi-hub/src/api/settings_write.rs:171-195`, `crates/domovoi-hub/src/settings/service.rs:230-245`).
- `ApiDeps` is constructed in `crates/domovoi-hub/src/runtime/mod.rs:306`, `crates/domovoi-hub/examples/dev_hub.rs:501` and `crates/domovoi-hub/tests/hub/api_harness.rs:413`.
- Test harness: `api()` returns `Api` with `get(uri, &login)`, `post(uri, &login, body)`, `send(method, uri, login, body)`, `admin()` (creates the admin and returns a `Login`), `audit_events(kind)`, `clock`, `store`, and `PASSWORD`/`USERNAME` constants (`crates/domovoi-hub/tests/hub/api_harness.rs`; verify `PASSWORD` is reachable). Test binaries register modules in `crates/domovoi-hub/tests/hub/main.rs`, `crates/domovoi-engine/tests/services/main.rs` and `crates/domovoi-store/tests/sqlite/main.rs`.
- Periodic jobs: `every(clock, period, shutdown, job)` in `crates/domovoi-hub/src/runtime/every.rs` (runs now, then every `period` from the start of each run).

## 4. Exact scope

Build, in this order of dependency:

**A. `domovoi-core` (pure, no I/O)**

| Path | Change |
|---|---|
| `crates/domovoi-core/src/knowledge/mod.rs` | **(new)** module root and re-exports |
| `crates/domovoi-core/src/knowledge/subject.rs` | **(new)** `SubjectRef`, `SubjectError` |
| `crates/domovoi-core/src/knowledge/predicate.rs` | **(new)** `Predicate`, `Cardinality`, per-predicate subject rules, staleness table |
| `crates/domovoi-core/src/knowledge/value.rs` | **(new)** value validation and canonicalization, `canonical_json`, `search_text`, `ValueError` |
| `crates/domovoi-core/src/knowledge/fact.rs` | **(new)** `Fact`, `FactSource`, `KnowledgeStatus`, `EvidenceRef`, `effective_status`, `fresh_until`, `usable_for_safety`, actor-source rule |
| `crates/domovoi-core/src/knowledge/graph.rs` | **(new)** `Relation`, `RelationKind`, `Entity` |
| `crates/domovoi-core/src/knowledge/paths.rs` | **(new)** `PathConfirmation`, `PathState`, `path_state`, `CoverageVerdict`, `CoverageEvidence`, `UnverifiableReason` |
| `crates/domovoi-core/src/knowledge/write.rs` | **(new)** `KnowledgeWrite`, `FactUpdate`, `RelationUpdate` |
| `crates/domovoi-core/src/ports/knowledge.rs` | **(new)** `KnowledgeRead`, `ManagementCoverage`, filters, `FactHit` |
| `crates/domovoi-core/src/ports/uow/transaction.rs` | add `AggregateKind::{Fact, Relation, Entity}`, matching `AggregateRef` constructors, `StateWrite::Knowledge(KnowledgeWrite)` |
| `crates/domovoi-core/src/ports/uow/journal.rs` | add `JournalKind::{FactChange, RelationChange, EntityChange}` |
| `crates/domovoi-core/src/ports/repos/knowledge.rs`, `repos/mod.rs` | rename `FactRepo` to `MetricSampleRepo` |
| `crates/domovoi-core/src/id.rs`, `actor.rs`, `addr.rs`, `lib.rs`, `ports/mod.rs` | `FactRecordId`, `RelationId`, `EntityId`; `SYSTEM_KNOWLEDGE`, `SYSTEM_PROFILE`; `canonical_cidr`; exports |

**B. `domovoi-store`**

| Path | Change |
|---|---|
| `crates/domovoi-store/migrations/<next free number>_knowledge_store.sql` | **(new)** section 8 |
| `crates/domovoi-store/src/facts/{mod,apply_facts,apply_graph,read,rows}.rs` | **(new)** applying `KnowledgeWrite`, implementing `KnowledgeRead`, row codecs |
| `crates/domovoi-store/src/uow/writes.rs` | one match arm: `StateWrite::Knowledge(w) => crate::facts::apply(...)` |
| `crates/domovoi-store/src/repos/knowledge.rs` | `FactRepo` impl renamed and pointed at `metric_samples` |
| `crates/domovoi-store/src/purge.rs`, `lib.rs` | retracted-content purge, `RetentionPolicy::retracted_facts`, `PurgeReport::retracted_facts` |

**C. `domovoi-config`**: `retracted_facts_days` becomes `Applies::Now`; doc comment and registry test updated.

**D. `domovoi-engine`**: new module `crates/domovoi-engine/src/knowledge/{mod,service,decide,owner,paths,graph,sweep,coverage,error}.rs` (pure decision functions in `decide.rs`; `UnavailableCoverage` in `coverage.rs`).

**E. `domovoi-hub`**

| Path | Change |
|---|---|
| `crates/domovoi-hub/src/api/knowledge.rs`, `knowledge_graph.rs`, `dto_knowledge.rs` | **(new)** handlers and DTOs |
| `crates/domovoi-hub/src/api/mod.rs` | modules, `ApiDeps::knowledge`, routes, `knowledge` tag |
| `crates/domovoi-hub/src/knowledge/mod.rs` | **(new)** `spawn_sweeper` |
| `crates/domovoi-hub/src/profile/backfill.rs` | **(new)** backfill and platform observation |
| `crates/domovoi-hub/src/profile/mod.rs`, `render.rs` | `ProfileDeps::knowledge`, observe after refresh, superseded label |
| `crates/domovoi-hub/src/runtime/mod.rs`, `runtime/settings.rs` | build the service, wire deps, run backfill, spawn sweeper, map the retention key |
| `crates/domovoi-hub/tests/hub/snapshots/hub__api_routes__openapi.snap` | the OpenAPI `insta` snapshot changes with the new routes: review it and accept with `cargo insta review` (test `openapi_snapshot`, `tests/hub/api_routes.rs:529`) |
| `crates/domovoi-hub/examples/dev_hub.rs`, `tests/hub/api_harness.rs`, `tests/hub/profile.rs` | constructors gain the new fields; the harness `Api` also gains `coverage: Arc<FakeCoverage>` (unavailable by default), used by the API tests |

**F. `domovoi-testkit`**: in-memory `KnowledgeRead` and `KnowledgeWrite` support in `InMemoryStore`, a `FakeCoverage` **(new)** (`crates/domovoi-testkit/src/coverage.rs`), the renamed metric-sample contract, and a new contract suite `contracts/knowledge_store.rs`.

**G. Generated and docs**: `web/src/api/openapi.json` and `web/src/api/schema.d.ts` (regenerated, never hand-edited), config reference, book pages, `ARCHITECTURE.md` (the edits are listed in section 15 of this brief).


### 4.1 Knowledge-layer context source

Prompt 32 defined the contract for this source (its section 6.9) and built nothing; this prompt builds it.
Copy of the contract, **fixed**:

- Exactly one source, `KnowledgeSource` **(new)**, in `domovoi_engine::knowledge` (or a `context`
  submodule if the merged layout of 32 prefers it), wrapping the `KnowledgeRead` port and a `Redactor`,
  nothing else. Register it by pushing it onto `TriageDeps.extra_context` where the hub builds
  `TriageDeps` (`crates/domovoi-hub/src/runtime/triage_build.rs`). Edit no aggregator, cap, layout or
  rendering code and add no `ContextLayer` variant.
- Id `knowledge`, layer `Knowledge`. Supports `Overview` (the live facts and relations of the scope's
  host, at most 20: user-sourced and current first, then by predicate, newest first) and `Search { text }`
  (`KnowledgeRead::search_facts(text, Some(host), limit)`), never `Read`.
- Each fact or relation becomes one `ContextItem`: id key `fact:<id>` or `relation:<id>`; a title naming
  the predicate or relation kind and the subject; a redacted body with the value's canonical text, bounded
  by `max_item_bytes`, with `truncated` set when cut; status from `effective_status(now)`: `Current` →
  `Known`, `Stale` → `Stale { observed_at: last_verified_at or observed_at }`, `Disputed` →
  `Disputed { with: <id of one other live fact of the same subject and predicate> }`. `Superseded` and
  `Retracted` facts are never returned. Provenance origin by source: `User` and `Imported` → `Declared`,
  `Discovered` and `Integration` → `Observed`, `Inferred` → `Inferred`; reference `fact:<id>`,
  `observed_at`, and the actor's display.
- A store error is a gap with a short fixed reason; no live fact is an empty batch (a known empty).
- `trusted_cidr` and `management_path` facts are shown like any other: evidence, never permission.
  Nothing in an item can express that a fact is usable for safety.
- Tests: the source passes 31's `contracts::context` suite (`knowledge_source_meets_the_context_contract`),
  `retracted_and_superseded_facts_never_reach_the_context`, `knowledge_source_store_error_is_a_gap`, and an
  `insta` snapshot of one rendered knowledge section (`knowledge_section_renders_statuses`).

## 5. Explicit non-goals

- **No web pages** and no change under `web/src` except the two generated API files (prompt 48 builds the Knowledge browser, proposals and the management-path view).
- **No experience search, resolution-summary fields, `search_experience` or `ExperienceSearch` port** (prompt 35). The `facts_fts` table is for fact search only.
- **No aggregator, layer, cap or rendering change, no meta-tools, no change to `ReadTool`** (prompts 31 and 32). The one context change here is the `KnowledgeSource` of section 4.1, which follows prompt 32's contract exactly.
- **No helper, attestation, key pinning, `SafetyIdentity`, `ExpectedSafety`, nonce or `attest_safety` handling** (prompt 47, research R-10 still open). `CoverageEvidence` is an opaque string here; do not parse it.
- **No policy evaluator integration** (prompts 39 and 40). Do not make any existing check read these tables. The only rules this prompt enforces about safety-critical facts are storage-side (who can write them, when they are usable).
- **No wire change.** Do not rename `DigestFacts`, `DigestFact` or the `digest_facts` message kind.
- **No migration of `maintenance_windows`, `ack_rules`, `baselines` or `notes` into facts.** The `maintenance_window` predicate exists but nothing writes it yet.
- **No new advisory kind, no new effect kind, no new settings key** (the retention key already exists).
- **No automatic network-change trigger** (see section 2).
- Do not edit a merged migration. Do not touch `crates/domovoi-agent`, `crates/domovoi-proto`, `crates/domovoi-runner`, `crates/domovoi-llm`.
- Do not add dependencies. The code needs none that the workspace lacks.

## 6. Required interfaces and contracts

Items marked **fixed** must match exactly (names, strings, JSON, SQL). Items marked **convention** follow repository style. Every item here is **(new)** unless it says otherwise. Every public item needs a doc comment saying what it is for.

### 6.1 Ids, subjects and evidence (core) — fixed

```rust
// id.rs: three more define_id! newtypes (UUIDv7), same derives as the others
FactRecordId, RelationId, EntityId
// `FactRecordId` names a stored fact row. It is not `domovoi_core::capability::FactId` (prompt 24),
// which names a safety-critical fact the policy evaluator reads (for example `host.not_in_maintenance`).

// actor.rs
pub const SYSTEM_KNOWLEDGE: &str = "knowledge"; // sweeps and service-internal changes
pub const SYSTEM_PROFILE: &str = "profile";     // the host profile task

// knowledge/subject.rs
pub enum SubjectRef {
    Host(HostId),
    Container { host: HostId, name: String },
    Unit { host: HostId, name: String },
    Entity(EntityId),
}
impl SubjectRef { pub fn host(&self) -> Option<HostId>; }   // None only for Entity
// Display / FromStr, serde as the string form:
//   host:<uuid>   container:<host uuid>/<name>   unit:<host uuid>/<name>   entity:<uuid>
```

- `FromStr` is strict: lowercase kind, hyphenated lowercase UUID. A container name is 1 to 128 characters of `[A-Za-z0-9_.-]` starting with `[A-Za-z0-9]`. A unit name is 1 to 256 characters of `[A-Za-z0-9:_.@-]`. Anything else is `SubjectError::{UnknownKind, BadId, BadName}`.
- The ref is by **name**, not by the runtime identity prompts 24/25 bind to plans: a fact must survive a container being recreated. Do not import anything from prompt 25.
- `EvidenceRef` (in `fact.rs`): text `<kind>:<uuid>` with `kind` in `note | incident | signal | action | operation | step`, at most 64 characters. Validation is **syntactic only**: do not check that the target exists (`operation` and `step` come from prompt 26, unmerged). `Display`/`FromStr`, serde as string.

### 6.2 Predicates — fixed

```rust
pub enum Predicate {
    Role, Criticality, OwnerNote, DependsOn, ManagementPath,
    MaintenanceWindow, BackupLocation, NeverRestart, TrustedCidr, HostPlatform,
}
// as_str / parse (serde names): "role" "criticality" "owner_note" "depends_on"
//   "management_path" "maintenance_window" "backup_location" "never_restart"
//   "trusted_cidr" "host_platform"
pub enum Cardinality { One, Many }
impl Predicate {
    pub const ALL: [Predicate; 10];
    pub fn cardinality(self) -> Cardinality;
    pub fn is_safety_critical(self) -> bool;                    // ManagementPath, TrustedCidr only
    pub fn staleness(self) -> jiff::SignedDuration;
    pub fn allows_subject(self, subject: &SubjectRef) -> bool;
    pub fn validate(self, subject: &SubjectRef, value: &serde_json::Value)
        -> Result<serde_json::Value, ValueError>;               // returns the canonical value
}
```

| Predicate | Cardinality | Subject kinds | Value (JSON, strict: unknown keys refused) | Staleness |
|---|---|---|---|---|
| `role` | One | host, container, unit, entity | string, trimmed, 1 to 80 characters | 180 d |
| `criticality` | One | host, container, unit, entity | `"critical"` or `"warning"` (the existing baseline `Criticality` words) | 180 d |
| `owner_note` | Many | any | string, trimmed, 1 to 1000 characters | 365 d |
| `depends_on` | Many | any | string holding a canonical `SubjectRef` different from the subject | 180 d |
| `management_path` | Many | host only | `{"cidr": "<canonical CIDR>"}` | 30 d |
| `maintenance_window` | Many | any | `{"starts_at": "<RFC 3339 UTC>", "ends_at": "<RFC 3339 UTC>", "note": "<optional string, 1 to 200>"}` with `ends_at > starts_at`; the fact's `valid_until` is forced to `ends_at` | 7 d |
| `backup_location` | Many | host, container, unit, entity | string, trimmed, 1 to 200 characters | 90 d |
| `never_restart` | One | container, unit | the boolean `true` (retract the fact to remove the rule) | 365 d |
| `trusted_cidr` | Many | host only | `{"cidr": "<canonical CIDR>"}` | 30 d |
| `host_platform` | One | host only | `{"hostname": s, "os": s, "kernel": s, "arch": s, "cpu_count": u32, "mem_total_bytes": u64}`, every field required, strings 1 to 200 characters | 14 d |

- `canonical_cidr(text: &str) -> Option<String>` goes in `crates/domovoi-core/src/addr.rs` and is re-exported from `lib.rs` next to `valid_cidr`. It parses with the module's existing parser and returns the 04b canonical form: host bits zeroed, then `/prefix`; IPv4 dotted decimal; IPv6 lowercase RFC 5952 (longest run of zero groups compressed, no leading zeros, a single zero group is not compressed). A bare address means `/32` or `/128`. As in `in_cidr`, an IPv4-mapped IPv6 address reads as the IPv4 address it maps, so `::ffff:10.0.0.7/104` is refused (prefix wider than the IPv4 family) and `::ffff:10.0.0.7` becomes `10.0.0.7/32`. Prompt 47 must reuse this function for report comparison; it must not write a second one.
- `ValueError` (thiserror): `WrongType`, `OutOfRange`, `UnknownKey`, `MissingKey`, `BadCidr`, `BadTimestamp`, `SubjectNotAllowed`.
- `canonical_json(value: &Value) -> String` in `value.rs`: compact JSON with object keys sorted bytewise, recursively. **Store the `value` column with this function, never with `serde_json::to_string`**, so equality of the text column means equality of the value regardless of the `serde_json` feature set. `search_text(predicate, value) -> String`: every string, number and boolean leaf of the value joined by a space, keys in sorted order.

### 6.3 Facts, relations, entities (core) — fixed

```rust
pub enum FactSource { User, Discovered, Integration, Inferred, Imported }   // snake_case
pub enum KnowledgeStatus { Current, Stale, Disputed, Superseded, Retracted } // snake_case
impl KnowledgeStatus { pub fn is_live(self) -> bool }   // Current | Stale | Disputed

pub struct Fact {
    pub id: FactRecordId,
    pub subject: SubjectRef,
    pub predicate: Predicate,
    pub value: Option<serde_json::Value>,   // None only after the content purge
    pub source: FactSource,
    pub status: KnowledgeStatus,            // the stored status
    pub observed_at: Timestamp,
    pub valid_until: Option<Timestamp>,
    pub last_verified_at: Option<Timestamp>,
    pub supersedes: Option<FactRecordId>,
    pub evidence: Option<EvidenceRef>,
    pub actor: Actor,
    pub created_at: Timestamp,
    pub retracted_at: Option<Timestamp>,
    pub retracted_by: Option<Actor>,
    pub content_purged_at: Option<Timestamp>,
}
impl Fact {
    pub fn fresh_until(&self) -> Option<Timestamp>;
    pub fn effective_status(&self, now: Timestamp) -> KnowledgeStatus;
    pub fn usable_for_safety(&self, now: Timestamp) -> bool;
}
/// Which actors may write which source. Fixed table:
pub fn source_allows_actor(source: FactSource, actor: &Actor) -> bool;
```

- `fresh_until` = the earlier of `valid_until` and `(last_verified_at or observed_at) + predicate.staleness()`.
- `effective_status(now)`: if the stored status is `Current` and `now >= fresh_until`, `Stale`; every other stored status is returned unchanged.
- `usable_for_safety(now)`: `predicate.is_safety_critical() && source == User && effective_status(now) == Current && value.is_some()`. (For `management_path` the confirmation is a further requirement, see `path_state`.)
- `source_allows_actor`: `User` needs `Actor::User(_)`; `Discovered` needs `System` or `Agent`; `Integration` and `Inferred` need `System`; `Imported` needs `System` or `User`. `Actor::Policy(_)` is never allowed to write a fact. `host_platform` additionally accepts only `Discovered` or `Integration` (checked by the service).

```rust
pub enum RelationKind { DependsOn, RunsOn, RoutesTo, StoresOn, BacksUpTo, ManagedFrom } // snake_case
pub struct Relation {
    pub id: RelationId,
    pub from: Option<SubjectRef>,   // None only after the content purge
    pub kind: RelationKind,
    pub to: Option<SubjectRef>,     // None only after the content purge
    pub source: FactSource,
    pub status: KnowledgeStatus,    // only Current, Stale or Retracted are ever stored
    pub observed_at: Timestamp,
    pub evidence: Option<EvidenceRef>,
    pub actor: Actor,
    pub created_at: Timestamp,
    pub retracted_at: Option<Timestamp>,
    pub retracted_by: Option<Actor>,
    pub content_purged_at: Option<Timestamp>,
}
pub struct Entity {
    pub id: EntityId,
    pub kind: String,        // slug [a-z0-9_-]{1,32}
    pub name: String,        // trimmed, 1 to 80 characters, unique ignoring case
    pub created_by: Actor,   // always Actor::User
    pub created_at: Timestamp,
}
```

A relation is stale 180 days after `observed_at` (the same effective-status rule, no `last_verified_at`).

### 6.4 Management paths and the coverage hook — fixed

```rust
pub struct PathConfirmation {
    pub fact: FactRecordId,
    pub host: HostId,
    pub confirmed_by: Actor,         // Actor::User
    pub confirmed_at: Timestamp,
    pub evidence: CoverageEvidence,
}
/// Opaque in this prompt: prompt 47 will store the verified safety identity here.
/// 34 stores it and compares it for equality only; never parse it.
pub struct CoverageEvidence(String);   // 1 to 2048 characters

pub enum CoverageVerdict {
    Covered { evidence: CoverageEvidence },
    Uncovered,
    Unverifiable { reason: UnverifiableReason },
}
pub enum UnverifiableReason {          // RW-37's `unverified` reasons, plus the "no attestor wired" case
    NotAvailable, KeyNotPinned, KeyMismatch, AttestationMissing, AttestationStale,
    SignatureInvalid, ReportIncomplete, GenerationRegressed,
}
pub enum PathState { Candidate, Unconfirmed, Confirmed, Stale }
/// Pure, evaluated in this order: Candidate when the fact's source is not User; Stale when its
/// effective status is Stale; Unconfirmed when it has no confirmation; otherwise Confirmed. A
/// confirmation belongs to one fact id, so replacing or retracting the fact ends it. Only live
/// facts are ever listed as paths.
pub fn path_state(fact: &Fact, confirmation: Option<&PathConfirmation>, now: Timestamp) -> PathState;

#[async_trait]
pub trait ManagementCoverage: Send + Sync {
    /// Whether a coverage check can currently succeed at all (false until prompt 47).
    fn is_available(&self) -> bool;
    /// Whether the verified helper attestation shows `cidr` (canonical) covered on `host`.
    async fn verdict(&self, host: HostId, cidr: &str) -> CoverageVerdict;
}
```

`domovoi_engine::knowledge::UnavailableCoverage` implements it: `is_available() == false`, `verdict` always `Unverifiable { reason: NotAvailable }`. It is the only production implementation in this prompt. Doc comment: "prompt 47 replaces this wiring with the attestation-backed implementation".

### 6.5 Ports — fixed names, convention for the rest

```rust
// ports/knowledge.rs
pub struct FactFilter {
    pub host: Option<HostId>,
    pub subject: Option<SubjectRef>,
    pub predicate: Option<Predicate>,
    pub source: Option<FactSource>,
    /// None = live statuses only (current, stale, disputed). Superseded and retracted rows
    /// appear only when listed here.
    pub statuses: Option<Vec<KnowledgeStatus>>,
}
pub struct RelationFilter {
    pub subject: Option<SubjectRef>,          // matches either end
    pub kind: Option<RelationKind>,
    pub include_retracted: bool,              // default false
}
pub struct FactHit { pub fact: Fact, pub score: f64 }

#[async_trait]
pub trait KnowledgeRead: Send + Sync {
    async fn fact(&self, id: FactRecordId) -> Result<Option<Versioned<Fact>>, StoreError>;
    async fn facts(&self, filter: &FactFilter, page: &Page) -> Result<Paged<Fact>, StoreError>;
    /// Live facts of one subject and predicate, oldest first.
    async fn live_facts(&self, subject: &SubjectRef, predicate: Predicate)
        -> Result<Vec<Versioned<Fact>>, StoreError>;
    /// Every live fact, oldest first, at most `limit` (the sweep reads these).
    async fn all_live_facts(&self, limit: u32) -> Result<Vec<Versioned<Fact>>, StoreError>;
    /// Full-text search over live facts only; retracted and superseded facts never match.
    async fn search_facts(&self, query: &str, host: Option<HostId>, limit: u16)
        -> Result<Vec<FactHit>, StoreError>;
    async fn confirmation(&self, fact: FactRecordId) -> Result<Option<PathConfirmation>, StoreError>;
    async fn relation(&self, id: RelationId) -> Result<Option<Versioned<Relation>>, StoreError>;
    async fn relations(&self, filter: &RelationFilter, page: &Page) -> Result<Paged<Relation>, StoreError>;
    async fn live_relation(&self, from: &SubjectRef, kind: RelationKind, to: &SubjectRef)
        -> Result<Option<Versioned<Relation>>, StoreError>;
    async fn entity(&self, id: EntityId) -> Result<Option<Versioned<Entity>>, StoreError>;
    async fn entities(&self) -> Result<Vec<Entity>, StoreError>;
    /// True when a live fact or relation names `entity:<id>` as a subject or endpoint, or a live
    /// `depends_on` fact's value is that ref.
    async fn entity_in_use(&self, id: EntityId) -> Result<bool, StoreError>;
}
```

Ordering of `facts`/`relations` pages: `created_at DESC, id DESC` (keyset cursor, like `ActionRepo`). `search_facts` is `-bm25` descending, then `created_at DESC`; each word of the query must match as a whole word (reuse `fts::match_all`); an empty query matches nothing; host filter keeps facts whose `host_id` equals `host` **and** facts with no host (entity subjects).

### 6.6 Writes through the unit of work — fixed

```rust
// ports/uow/transaction.rs
AggregateKind::{Fact, Relation, Entity}   // as_str "fact" | "relation" | "entity", parse round-trips
AggregateRef::{fact(FactRecordId), relation(RelationId), entity(EntityId)}
StateWrite::Knowledge(KnowledgeWrite)

// knowledge/write.rs
pub enum KnowledgeWrite {
    /// New row, version 1. The store derives host_id, search_text and live from the row.
    FactInsert(Fact),
    /// Changes only the mutable columns below; refused unless the stored row is live.
    FactUpdate { id: FactRecordId, expected: u64, update: FactUpdate },
    RelationInsert(Relation),
    RelationUpdate { id: RelationId, expected: u64, update: RelationUpdate },
    EntityInsert(Entity),
    EntityRename { id: EntityId, expected: u64, name: String },
    EntityDelete { id: EntityId, expected: u64 },
    /// Insert or replace the confirmation of one fact.
    PathConfirmation(PathConfirmation),
}
pub struct FactUpdate {
    pub status: Option<KnowledgeStatus>,
    pub observed_at: Option<Timestamp>,
    pub last_verified_at: Option<Timestamp>,
    pub retraction: Option<(Timestamp, Actor)>,   // sets retracted_at and retracted_by; status must become Retracted
}
pub struct RelationUpdate {
    pub status: Option<KnowledgeStatus>,
    pub observed_at: Option<Timestamp>,
    pub retraction: Option<(Timestamp, Actor)>,
}

// ports/uow/journal.rs
JournalKind::FactChange     // "fact.changed"
JournalKind::RelationChange // "relation.changed"
JournalKind::EntityChange   // "entity.changed"
```

Journal payloads, `payload_v = 1`, actor = the writing actor, aggregate = the fact, relation or entity. **No value, no endpoints, no entity name.**

```json
{"change":"recorded","predicate":"role","subject":"host:0190…","status":"current","source":"user","supersedes":null,"evidence":null}
{"change":"retracted","kind":"depends_on"}
{"change":"renamed","kind":"vpn"}
```

`change` for facts is one of `recorded`, `refreshed`, `superseded`, `disputed`, `resolved`, `stale`, `verified`, `retracted`, `confirmed`; for relations `recorded`, `refreshed`, `stale`, `retracted`; for entities `created`, `renamed`, `deleted`.

Audit entries (owner actions only; never for discovered, integration, inferred, imported recordings or sweeps): kind and subject, `detail` never contains a value:

| Kind | Subject | Detail |
|---|---|---|
| `knowledge.fact.asserted` | `fact:<id>` | `{"predicate":…,"subject":…}` |
| `knowledge.fact.accepted` | `fact:<id>` (the new row) | `{"predicate":…,"subject":…,"from":"<old fact id>"}` |
| `knowledge.fact.retracted` | `fact:<id>` | `{"predicate":…,"subject":…}` |
| `knowledge.fact.verified` | `fact:<id>` | `{"predicate":…,"subject":…}` |
| `knowledge.management_path.confirmed` | `fact:<id>` | `{"host":"<host id>"}` |
| `knowledge.management_path.stale` | `fact:<id>` | `{"host":…,"cause":"<NetworkChangeCause>"}` (system actor; written because the change is safety-relevant) |
| `knowledge.relation.asserted` / `.retracted` | `relation:<id>` | `{"kind":…}` |
| `knowledge.entity.created` / `.renamed` / `.deleted` | `entity:<id>` | `{"kind":…}` |

### 6.7 The knowledge service (engine) — fixed names, convention for bodies

```rust
// crates/domovoi-engine/src/knowledge/service.rs
pub struct KnowledgeDeps {
    pub uow: Arc<dyn UnitOfWork>,
    pub read: Arc<dyn KnowledgeRead>,
    pub coverage: Arc<dyn ManagementCoverage>,
    pub clock: Arc<dyn Clock>,
}
pub struct KnowledgeService { /* deps + a tokio::sync::Mutex<()> serializing every mutating call + last sweep */ }

pub struct FactDraft {
    pub subject: SubjectRef,
    pub predicate: Predicate,
    pub value: serde_json::Value,
    pub source: FactSource,
    pub observed_at: Option<Timestamp>,     // default: now
    pub valid_until: Option<Timestamp>,
    pub evidence: Option<EvidenceRef>,
}
pub enum RecordEffect { Created, Refreshed, Superseded, Disputed, Unchanged }
pub struct Recorded { pub fact: Versioned<Fact>, pub effect: RecordEffect }

impl KnowledgeService {
    pub fn new(deps: KnowledgeDeps) -> Self;
    pub fn coverage_available(&self) -> bool;

    // System writers (discovered, integration, inferred, imported). The actor is checked
    // against `source_allows_actor`; `source = user` is refused here for every actor.
    pub async fn record(&self, draft: FactDraft, actor: &Actor) -> Result<Recorded, KnowledgeError>;

    // Owner actions. `user` is the authenticated session's UserId; the source is user.
    pub async fn assert_fact(&self, draft: FactDraft, user: UserId) -> Result<Recorded, KnowledgeError>;
    pub async fn accept(&self, id: FactRecordId, expected: u64, user: UserId) -> Result<Recorded, KnowledgeError>;
    pub async fn retract(&self, id: FactRecordId, expected: u64, user: UserId) -> Result<Versioned<Fact>, KnowledgeError>;
    pub async fn verify(&self, id: FactRecordId, expected: u64, user: UserId) -> Result<Versioned<Fact>, KnowledgeError>;
    pub async fn confirm_management_path(&self, id: FactRecordId, expected: u64, user: UserId)
        -> Result<ManagementPathView, KnowledgeError>;

    // Read side used by the API
    pub async fn facts(&self, filter: &FactFilter, page: &Page) -> Result<Paged<Fact>, KnowledgeError>;
    pub async fn search(&self, query: &str, host: Option<HostId>, limit: u16) -> Result<Vec<FactHit>, KnowledgeError>;
    pub async fn management_paths(&self, host: HostId) -> Result<Vec<ManagementPathView>, KnowledgeError>;

    // Staleness
    pub async fn note_network_change(&self, host: HostId, cause: NetworkChangeCause)
        -> Result<Vec<FactRecordId>, KnowledgeError>;
    pub async fn sweep(&self) -> Result<SweepReport, KnowledgeError>;

    // Graph
    pub async fn record_relation(&self, draft: RelationDraft, actor: &Actor) -> Result<Versioned<Relation>, KnowledgeError>;
    pub async fn assert_relation(&self, draft: RelationDraft, user: UserId) -> Result<Versioned<Relation>, KnowledgeError>;
    pub async fn retract_relation(&self, id: RelationId, expected: u64, user: UserId) -> Result<Versioned<Relation>, KnowledgeError>;
    pub async fn create_entity(&self, kind: &str, name: &str, user: UserId) -> Result<Versioned<Entity>, KnowledgeError>;
    pub async fn rename_entity(&self, id: EntityId, expected: u64, name: &str, user: UserId) -> Result<Versioned<Entity>, KnowledgeError>;
    pub async fn delete_entity(&self, id: EntityId, expected: u64, user: UserId) -> Result<(), KnowledgeError>;
}
pub struct RelationDraft { pub from: SubjectRef, pub kind: RelationKind, pub to: SubjectRef,
                           pub source: FactSource, pub observed_at: Option<Timestamp>, pub evidence: Option<EvidenceRef> }
pub struct ManagementPathView { pub fact: Versioned<Fact>, pub confirmation: Option<PathConfirmation>, pub state: PathState }
pub enum NetworkChangeCause { PeerAddressChanged, HelperSafetySetChanged, Other }   // snake_case in audit
pub struct SweepReport { pub marked_stale: u32, pub relations_marked_stale: u32 }
```

`facts()` and `search()` call `sweep_if_due` first (at most one sweep per 5 minutes of `Clock` time), so a `stale` filter does not trail the clock by a day.

`KnowledgeError` (thiserror, `crates/domovoi-engine/src/knowledge/error.rs`) — fixed variants:

```rust
InvalidSubject(SubjectError), InvalidValue(ValueError), InvalidInput(String),
SubjectNotAllowed { predicate: Predicate }, ActorNotAllowed { source: FactSource },
NotFound, Conflict,                      // Conflict: a version check failed; reload and retry
NotLive { status: KnowledgeStatus },     // retract/verify/accept on superseded or retracted
NotOwnerFact,                            // confirm on a fact whose source is not user
UseConfirmation,                         // verify on a management_path
PathUncovered,                           // verdict Uncovered
CoverageUnverifiable(UnverifiableReason),
DuplicateEntity, EntityInUse,
Store(StoreError)
```

A `CommitError::Conflict` maps to `Conflict`; `CommitError::Constraint` of the unique-live-value index maps to `Conflict` too (a concurrent duplicate); other commit errors map to `Store(StoreError::Backend(text))`.

### 6.8 The decision functions (pure, `decide.rs`) — fixed behaviour

All of the lifecycle rules live in pure functions with no I/O, tested directly.

```rust
pub struct RecordPlan { pub inserts: Vec<Fact>, pub updates: Vec<PlannedUpdate>, pub effect: RecordEffect, pub result: FactRecordId }
pub struct PlannedUpdate { pub id: FactRecordId, pub expected: u64, pub update: FactUpdate, pub change: &'static str }
pub fn plan_record(live: &[Versioned<Fact>], draft: &ValidDraft, actor: &Actor, now: Timestamp, new_id: FactRecordId) -> RecordPlan;
```

`ValidDraft` is a `FactDraft` after validation: canonical value, `valid_until` forced for `maintenance_window`, `observed_at` defaulted. `live` is `KnowledgeRead::live_facts(subject, predicate)`.

Pseudocode (the whole lifecycle):

```
same = the live fact whose canonical value equals the draft's, if any

if predicate.cardinality() == Many:
    if same:  return refresh_or_upgrade(same)
    else:     insert new fact (Current)               -> Created

# One:
mine   = live facts with source == draft.source and a different value
others = live facts with another source and a different value
supersede every fact in `mine`                          (status Superseded, change "superseded")
if draft.source == User: supersede every fact in `others` (the owner wins)
if same:  refresh_or_upgrade(same)
else:     insert new fact (supersedes = the newest superseded fact's id, if any)
settle(live facts still live after the above, including the new one)

refresh_or_upgrade(same):
    if predicate.is_safety_critical():
        # observation and re-assertion never revive or extend a safety-critical fact
        return Unchanged(same)
    if draft.source == same.source:
        update same: observed_at = max(old, draft.observed_at), last_verified_at = now,
                     status Stale -> Current                     -> Refreshed
    elif draft.source == User:                                   # the owner asserts a value another source holds
        supersede same; insert a user fact with the same value, supersedes = same.id   -> Superseded
    else:                                                        # corroboration by another source
        update same: last_verified_at = now, status Stale -> Current                  -> Refreshed

settle(rows):   # One-cardinality predicates only
    values = distinct canonical values among the live rows
    if len(values) > 1: every live row -> Disputed (change "disputed") when not already
    else:               every Disputed row -> Current (change "resolved")
```

Notes the executor must keep:

- A `Disputed` fact whose competing row is superseded returns to `Current`; a `Stale` row never returns to `Current` by `settle`, only by refresh.
- `Superseded` rows keep their value (history). Only `Retracted` rows are purged.
- `accept(id)` loads the fact, requires it live (`NotLive` otherwise) and a present value, builds a `FactDraft` with the same subject, predicate, value, `valid_until` and evidence, `source = User`, and runs `assert_fact`. The audit kind is `knowledge.fact.accepted` with `from = id`. This is the only way a non-owner fact becomes `user`-sourced.
- `confirm_management_path(id, expected, user)`: the fact must be a `management_path` (else `InvalidInput`), `source == User` (else `NotOwnerFact`), with effective status `Current` or `Stale` (else `NotLive`). Then `coverage.verdict(host, cidr)`: `Covered{evidence}` commits `KnowledgeWrite::PathConfirmation` (replacing any earlier one) plus `FactUpdate { status: Current, last_verified_at: now }` plus journal `confirmed` plus audit `knowledge.management_path.confirmed`; `Uncovered` returns `PathUncovered`; `Unverifiable{reason}` returns `CoverageUnverifiable(reason)`. Nothing is written in the last two cases.
- `verify(id, ...)`: refused for `management_path` with `UseConfirmation`; for any other live fact sets `last_verified_at = now` and `Stale -> Current`; audit `knowledge.fact.verified`.
- `retract`: allowed on any live fact for any predicate (it only removes trust); sets status `Retracted`, `retracted_at`, `retracted_by`; the store clears the FTS row in the same transaction.
- `note_network_change(host, cause)`: for every live `management_path` fact of the host whose stored status is `Current`, `FactUpdate { status: Stale }`, journal `stale`, and one audit `knowledge.management_path.stale` with the system actor `system:knowledge`. Returns the ids changed. It never touches confirmations (a stale path is simply not `Confirmed`; `path_state` reports `Stale`).
- `sweep()`: reads `all_live_facts(10_000)`, and for every stored-`Current` fact whose `effective_status(now)` is `Stale` commits `FactUpdate { status: Stale }` with a journal `stale` (system actor `system:knowledge`), in transactions of at most 100 facts. For safety-critical `management_path` facts it also writes the audit entry above with cause `other`. Relations are swept the same way (`relations_marked_stale`). `sweep` is idempotent.
- `management_paths(host)`: the host's live `management_path` facts with their confirmations and `path_state`, ordered by canonical cidr. A `Disputed` row cannot occur for `Many` predicates.

### 6.9 HTTP API — fixed paths, methods and status codes

All under `/api/v1/knowledge`, all need a session (`Authed`), all unsafe methods need the CSRF token as everywhere else. Tag `knowledge` (add it to the `ApiDoc` tag list: `("knowledge", description = "Facts, relations and entities the hub knows")`). `{id}` is a UUID; a malformed id is 404.

| Method and path | operation_id | Request | Success | Errors |
|---|---|---|---|---|
| `GET /facts` | `list_facts` | query: `host` (name), `subject`, `predicate`, `source`, `status` (comma list of statuses), `q`, `limit` (default 50, max 200), `cursor` | 200 `KnowledgeFactPageDto` | 400 bad filter value; 404 unknown host |
| `GET /facts/{id}` | `get_fact` | | 200 `KnowledgeFactDto` | 404 |
| `POST /facts` | `assert_fact` | `KnowledgeFactInput` | 201 `KnowledgeFactDto` | 400 invalid; 403 password; 429 |
| `POST /facts/{id}/accept` | `accept_fact` | `KnowledgeFactAction` | 200 `KnowledgeFactDto` | 404; 409 not live or stale version; 403/429 |
| `POST /facts/{id}/retract` | `retract_fact` | `KnowledgeFactAction` | 200 `KnowledgeFactDto` | 404; 409 |
| `POST /facts/{id}/verify` | `verify_fact` | `KnowledgeFactAction` | 200 `KnowledgeFactDto` | 404; 409 (also for a management path: "confirm it instead"); 403/429 |
| `GET /hosts/{name}/management-paths` | `list_management_paths` | | 200 `ManagementPathsDto` | 404 unknown host |
| `POST /facts/{id}/confirm` | `confirm_management_path` | `KnowledgeFactAction` with `password` required | 200 `ManagementPathDto` | 404; 409 not live, not owner fact, uncovered, unverifiable, stale version; 403/429 |
| `GET /relations` | `list_relations` | query: `subject`, `kind`, `include_retracted`, `limit`, `cursor` | 200 `RelationPageDto` | 400 |
| `POST /relations` | `assert_relation` | `RelationInput` | 201 `RelationDto` | 400 |
| `POST /relations/{id}/retract` | `retract_relation` | `{expected_version}` | 200 `RelationDto` | 404; 409 |
| `GET /entities` | `list_entities` | | 200 `Vec<EntityDto>` | |
| `POST /entities` | `create_entity` | `{kind, name}` | 201 `EntityDto` | 400; 409 duplicate name |
| `PUT /entities/{id}` | `rename_entity` | `{name, expected_version}` | 200 `EntityDto` | 404; 409 duplicate or stale |
| `DELETE /entities/{id}` | `delete_entity` | query `expected_version` | 204 | 404; 409 in use or stale |

Step-up: **a password is required (`403 forbidden` "this change needs your password" when absent, `403` when wrong, `429` while locked out) for `POST /facts`, `accept`, `verify` and `confirm` when the fact's predicate `is_safety_critical()`**, and always for `confirm`. `retract` never needs it (it only removes trust). The password goes in the request body field `password`.

Wire shapes (JSON, `snake_case`; `value` is any JSON, so the schema annotation is `serde_json::Value`, verify what the generated TypeScript says and use `unknown` there):

```json
// KnowledgeFactInput  (source is NOT accepted: the API forces "user")
{ "subject": "host:0190…", "predicate": "management_path", "value": {"cidr": "192.168.1.0/24"},
  "evidence": null, "valid_until": null, "password": "…" }

// KnowledgeFactAction
{ "expected_version": 3, "password": null }

// KnowledgeFactDto (not `FactDto`: that name is the existing digest-fact DTO in `api/dto_admin.rs`; keep it unchanged)
{ "id": "…", "subject": "host:…", "predicate": "role", "value": "media server",
  "source": "user", "status": "current", "stored_status": "current", "version": 1,
  "observed_at": "2026-11-18T10:00:00Z", "valid_until": null, "last_verified_at": null,
  "fresh_until": "2027-05-17T10:00:00Z", "supersedes": null, "evidence": "note:…",
  "actor": "user:…", "created_at": "…", "retracted_at": null, "content_purged": false }

// ManagementPathDto
{ "fact": <KnowledgeFactDto>, "state": "unconfirmed",
  "confirmation": { "confirmed_by": "user:…", "confirmed_at": "…" } | null }

// ManagementPathsDto
{ "host": "nas", "coverage_available": false, "paths": [<ManagementPathDto>] }

// KnowledgeFactPageDto
{ "items": [<KnowledgeFactDto>], "next_cursor": null }

// RelationInput (source forced to "user")      RelationDto
{ "from": "container:0190…/plex", "kind": "runs_on", "to": "host:0190…", "evidence": null }
{ "id": "…", "from": "container:…/plex", "kind": "runs_on", "to": "host:…", "source": "user",
  "status": "current", "stored_status": "current", "version": 1, "observed_at": "…",
  "evidence": null, "actor": "user:…", "created_at": "…", "retracted_at": null, "content_purged": false }
// after the content purge "from" and "to" are null

// EntityDto                                    entity create / rename bodies
{ "id": "…", "kind": "vpn", "name": "Home VPN", "created_by": "user:…", "created_at": "…", "version": 1 }
{ "kind": "vpn", "name": "Home VPN" }     { "name": "Home VPN 2", "expected_version": 1 }
```

`RelationPageDto` is `{ "items": [<RelationDto>], "next_cursor": null }`. `subject` and `from`/`to` are subject-ref strings; a malformed one is 400. `KnowledgeFactInput.evidence` is an evidence-ref string or null.

- `status` in `KnowledgeFactDto` is the **effective** status at request time; `stored_status` is the row's. After the content purge `value` is `null` and `content_purged` is `true`.
- `evidence` of a confirmation (`CoverageEvidence`) is **not** returned by the API.
- With `q` present the list runs `search`, ignores `cursor`, returns at most `limit` (max 50) and never sets `next_cursor`; the other filters still narrow it.
- A fact with `status=retracted` listed explicitly shows its value until purged. The default list, `q`, and every service read used for model context never include retracted or superseded facts.
- Error body is the existing `ErrorBody`. Do not add error codes or strings the web layer would show raw; messages are short and plain (for example "someone changed it meanwhile; reload and try again").

## 7. Ordered implementation steps

Work on branch `knowledge-store`, cut from the latest `overhaul-investigation`. Commit after each numbered step that leaves the scoped checks green (Conventional Commits, one scope, one line, for example `refactor(store): rename the digest fact repository`, `feat(core): knowledge types and ports`, `feat(store): facts, relations and entities`, `feat(engine): knowledge service`, `feat(hub): knowledge api`). PR title has one scope too.

1. **Inspect and verify the repository context; report discrepancies.** List `crates/domovoi-store/migrations/` and note the next free number (expected `0012`, but read the directory). Open every file and symbol in section 3 and confirm it exists as described. Confirm prompts 08, 09, 10, 11 and 57 are merged (`UnitOfWork`, `StateWrite`, chained audit, `RetentionPolicy`, `retracted_facts_days`). Check whether prompts 26/27 have merged and, if so, whether they added `AggregateKind` or `JournalKind` variants or a `StateWrite` arm (keep theirs; add yours beside them). Confirm `tokio::sync::Mutex` is available in `domovoi-engine` (it is: the `sync` feature is on). Run `graphify query "StateWrite"` only as a map; trust the files. If anything differs materially, write it down before coding, and stop if it changes what you would build.
2. **Rename the digest-fact port (mechanical).** `FactRepo` becomes `MetricSampleRepo` everywhere listed in section 3 (type and imports and doc comments only; field names `facts` stay). Rename `fact_repo_contract` to `metric_sample_repo_contract` and the SQLite test `sqlite_fact_repo_satisfies_contract` to `sqlite_metric_sample_repo_satisfies_contract`. After this, `grep -rn "FactRepo" crates` must find only `MetricSampleRepo`. Do not touch `DigestFact`, `DigestFacts` or the wire. Run clippy and the tests of `domovoi-core`, `domovoi-store`, `domovoi-testkit`, `domovoi-hub` for the changed files. Commit.
3. **Core, part 1.** `FactRecordId`, `RelationId`, `EntityId`; `SYSTEM_KNOWLEDGE`, `SYSTEM_PROFILE`; `canonical_cidr` with its unit tests (vectors in section 12.1). Commit.
4. **Core, part 2.** `knowledge/` module: subject, predicate, value, fact, graph, paths, write; `ports/knowledge.rs`; the `AggregateKind`, `AggregateRef`, `StateWrite::Knowledge` and `JournalKind` additions with their `as_str`/`parse`. Fix every exhaustive `match` on the extended enums (grep `AggregateKind::`, `JournalKind::`, `StateWrite::`). Write the core unit tests. Keep each file under 500 lines (split `fact.rs` if it grows). Commit.
5. **Migration and store.** Add the migration (section 8). Implement `facts/` in the store: `apply` for each `KnowledgeWrite` (semantics in section 8.3), `KnowledgeRead`, row codecs; add the one `StateWrite::Knowledge` arm in `uow/writes.rs`; port the `MetricSampleRepo` SQL to `metric_samples`; extend `Store::purge` (section 8.4). Add the SQLite tests of section 12.3. Commit.
6. **Testkit.** In-memory `KnowledgeWrite` application and `KnowledgeRead` in `InMemoryStore` (new files beside `uow_apply.rs`; keep the same conflict semantics and the same uniqueness and liveness rules as SQLite); `FakeCoverage`; the `KnowledgeSubject` trait and the contract suite `contracts/knowledge_store.rs`; run the suite against both `InMemoryStore` and SQLite. Commit.
7. **Config and retention wiring.** `retracted_facts_days` becomes `Applies::Now`; update its doc comment, the registry test and the hub's `retention()` mapping and tests (section 8.4). Commit.
8. **Engine.** `decide.rs` first, with its unit tests, then `service.rs`, `owner.rs`, `paths.rs`, `graph.rs`, `sweep.rs`, `coverage.rs`, `error.rs`. Add the service tests of section 12.2 (register the new modules in `tests/services/main.rs`). Commit.
9. **Hub API.** DTOs, handlers, routes, `ApiDeps::knowledge`, tag; the step-up helper. Wire `KnowledgeService` in `runtime/mod.rs` with `UnavailableCoverage`; update the three `ApiDeps` constructors; add the API tests (section 12.4). Commit.
10. **Sweeper and profile.** `knowledge::spawn_sweeper` (hourly through `every`, runs once at start); `ProfileDeps::knowledge`; observation after `refresh`; the superseded label; `backfill.rs`; run the backfill once during `assemble`, before the profile task is spawned. Update the five `ProfileDeps` constructors. Add the tests of section 12.5. Commit.
11. **Generated files and documentation.** `cargo xtask openapi && cd web && npm run gen:api`; `cargo xtask docs-config`; book and `ARCHITECTURE.md` edits (the list in section 15 of this brief); `cargo xtask docs-links`. Commit.
12. **Checks and the completion report** (the commands are in the Repository rules block; add the ones section 12.6 lists).

## 8. Database and persistence

### 8.1 Migration

File: `crates/domovoi-store/migrations/<next free number>_knowledge_store.sql`. Take the number by listing the directory on your branch; today the next is `0012`. Describe the migration by its contents, never by number, in code comments and docs. One file, run by sqlx in one transaction.

```sql
-- digest_facts holds metric samples, not knowledge facts. Copy them under the right name,
-- and leave a view under the old name for one release so the previous binary keeps working.
CREATE TABLE metric_samples (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    host_id     TEXT NOT NULL,
    key         TEXT NOT NULL,
    label       TEXT NOT NULL,
    value       TEXT NOT NULL,
    unit        TEXT,
    observed_at INTEGER NOT NULL
) STRICT;
INSERT INTO metric_samples (id, host_id, key, label, value, unit, observed_at)
    SELECT id, host_id, key, label, value, unit, observed_at FROM digest_facts ORDER BY id;
CREATE INDEX metric_samples_host_observed ON metric_samples (host_id, observed_at, id);
DROP TABLE digest_facts;
CREATE VIEW digest_facts AS
    SELECT id, host_id, key, label, value, unit, observed_at FROM metric_samples;
CREATE TRIGGER digest_facts_insert INSTEAD OF INSERT ON digest_facts BEGIN
    INSERT INTO metric_samples (host_id, key, label, value, unit, observed_at)
    VALUES (new.host_id, new.key, new.label, new.value, new.unit, new.observed_at);
END;

-- Subjects the owner invents ("VPN", "NAS pool"). Never inventory.
CREATE TABLE entities (
    id         TEXT PRIMARY KEY,
    kind       TEXT NOT NULL,
    name       TEXT NOT NULL,
    created_by TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    version    INTEGER NOT NULL DEFAULT 1
) STRICT;
CREATE UNIQUE INDEX entities_name ON entities (lower(name));

-- Typed facts. Enums (predicate, source, status) are validated in Rust, never with CHECK.
-- `live` is derived in Rust (status is current, stale or disputed) so SQL never lists statuses
-- except through it. `value` and `search_text` are NULL after the retention purge.
CREATE TABLE facts (
    id                TEXT PRIMARY KEY,
    subject_ref       TEXT NOT NULL,
    host_id           TEXT,
    predicate         TEXT NOT NULL,
    value             TEXT,
    search_text       TEXT,
    live              INTEGER NOT NULL,
    source            TEXT NOT NULL,
    status            TEXT NOT NULL,
    observed_at       INTEGER NOT NULL,
    valid_until       INTEGER,
    last_verified_at  INTEGER,
    supersedes_id     TEXT REFERENCES facts (id),
    evidence_ref      TEXT,
    actor             TEXT NOT NULL,
    created_at        INTEGER NOT NULL,
    retracted_at      INTEGER,
    retracted_by      TEXT,
    content_purged_at INTEGER,
    version           INTEGER NOT NULL DEFAULT 1
) STRICT;
CREATE INDEX facts_subject ON facts (subject_ref, predicate, live);
CREATE INDEX facts_host ON facts (host_id, predicate, live);
CREATE INDEX facts_retracted ON facts (status, retracted_at);
CREATE INDEX facts_created ON facts (created_at DESC, id DESC);
-- One live row per subject, predicate and canonical value (race protection for the service).
CREATE UNIQUE INDEX facts_one_live_value ON facts (subject_ref, predicate, value)
    WHERE live = 1 AND value IS NOT NULL;

CREATE VIRTUAL TABLE facts_fts USING fts5 (
    subject_ref, predicate, search_text,
    content = 'facts', content_rowid = 'rowid',
    tokenize = 'unicode61 remove_diacritics 0'
);
CREATE TRIGGER facts_fts_insert AFTER INSERT ON facts BEGIN
    INSERT INTO facts_fts (rowid, subject_ref, predicate, search_text)
    SELECT new.rowid, new.subject_ref, new.predicate, new.search_text WHERE new.live = 1;
END;
CREATE TRIGGER facts_fts_update AFTER UPDATE ON facts BEGIN
    INSERT INTO facts_fts (facts_fts, rowid, subject_ref, predicate, search_text)
    SELECT 'delete', old.rowid, old.subject_ref, old.predicate, old.search_text WHERE old.live = 1;
    INSERT INTO facts_fts (rowid, subject_ref, predicate, search_text)
    SELECT new.rowid, new.subject_ref, new.predicate, new.search_text WHERE new.live = 1;
END;
CREATE TRIGGER facts_fts_delete AFTER DELETE ON facts BEGIN
    INSERT INTO facts_fts (facts_fts, rowid, subject_ref, predicate, search_text)
    SELECT 'delete', old.rowid, old.subject_ref, old.predicate, old.search_text WHERE old.live = 1;
END;

CREATE TABLE relations (
    id                TEXT PRIMARY KEY,
    from_ref          TEXT,
    to_ref            TEXT,
    kind              TEXT NOT NULL,
    source            TEXT NOT NULL,
    status            TEXT NOT NULL,
    live              INTEGER NOT NULL,
    observed_at       INTEGER NOT NULL,
    evidence_ref      TEXT,
    actor             TEXT NOT NULL,
    created_at        INTEGER NOT NULL,
    retracted_at      INTEGER,
    retracted_by      TEXT,
    content_purged_at INTEGER,
    version           INTEGER NOT NULL DEFAULT 1
) STRICT;
CREATE UNIQUE INDEX relations_one_live_edge ON relations (from_ref, kind, to_ref) WHERE live = 1;
CREATE INDEX relations_from ON relations (from_ref, kind);
CREATE INDEX relations_to ON relations (to_ref, kind);
CREATE INDEX relations_retracted ON relations (status, retracted_at);
CREATE INDEX relations_created ON relations (created_at DESC, id DESC);

-- The owner's confirmation of a management path. `evidence` is opaque here; the helper
-- attestation work stores the verified safety identity in it.
CREATE TABLE management_path_confirmations (
    fact_id      TEXT PRIMARY KEY REFERENCES facts (id),
    host_id      TEXT NOT NULL,
    confirmed_by TEXT NOT NULL,
    confirmed_at INTEGER NOT NULL,
    evidence     TEXT NOT NULL
) STRICT;
CREATE INDEX management_path_confirmations_host ON management_path_confirmations (host_id);
```

Checks on this DDL that the executor must make: the FTS definition keeps the literal text `content = 'facts'` (the existing guard test parses it); the migration contains no `CHECK` for an enum; `Store::open` runs it on a fresh database and on the N-1 fixture; `PRAGMA quick_check` stays `ok`.

`MIN_READER` is **not** raised. The migration drops a table and recreates its name as a view, which the rule in `compat.rs` would normally call a rename; here the view and its `INSTEAD OF INSERT` trigger keep the previous binary's `INSERT INTO digest_facts` and `SELECT ... FROM digest_facts ORDER BY observed_at, id` working, which a test proves (section 12.3). Remove the view and trigger in a later release (not here), and raise `MIN_READER` then.

### 8.2 Transaction boundaries

- Every service operation builds **one** `Transaction`: its `writes` (`StateWrite::Knowledge`), the journal drafts of section 6.6, and, for owner actions, the audit drafts. One `UnitOfWork::commit` applies all of it or none (`BEGIN IMMEDIATE` on the single writer). A failed commit changes nothing and writes no journal or audit row.
- Writes inside one transaction run in the order given. The unique live-value index applies to live rows, so when a new row has the same canonical value as an old row it replaces (the upgrade case in `refresh_or_upgrade`), emit `FactUpdate(old -> Superseded)` **first** and `FactInsert(new)` after it. In every other case emit the updates first and the insert last, so a `supersedes_id` always names a row that exists. `PathConfirmation` comes after the `FactUpdate` of its fact.
- The service holds its mutation mutex across read, decision and commit. Reads (`KnowledgeRead`) never take it.
- The retention purge and the sweep are separate transactions (the purge through `Store::purge`, the sweep through `UnitOfWork`).

### 8.3 Semantics of each `KnowledgeWrite` in the SQLite store and the in-memory twin

| Write | Behaviour | Errors |
|---|---|---|
| `FactInsert(f)` | requires `f.value` present and `f.status.is_live()`; stores `value = canonical_json(value)`, `search_text = search_text(predicate, value)`, `host_id = subject.host()`, `live = 1`, `version = 1`; pushes `(AggregateRef::fact(id), 1)` to `receipt.versions` | existing id: `Conflict{expected: New, found: Version}`; the unique live-value index: `Constraint`; missing or non-live: `Constraint` |
| `FactUpdate{id, expected, update}` | reads the row; stored row must be live; applies only the given columns; `status = Retracted` requires `retraction`, and `retraction` requires `status = Retracted`; `live` recomputed; `version + 1`; pushes the new version | missing: `Conflict{.., found: Missing}`; wrong version: `Conflict{.., found: Version}`; row not live: `Constraint("the fact is <status>")` |
| `RelationInsert`, `RelationUpdate` | the same rules for relations (`live` is `Current` or `Stale`); the unique live-edge index | as above |
| `EntityInsert` | version 1; name unique ignoring case | duplicate: `Constraint` |
| `EntityRename`, `EntityDelete` | version checked; delete removes the row | missing or wrong version: `Conflict` |
| `PathConfirmation(c)` | the fact must exist, be a `management_path` and have `host_id == c.host`; insert or replace by `fact_id` | otherwise `Constraint` |

Version numbers: a row written by `FactInsert` is version 1, and every update adds 1. Entities have `version` too.

Allowed `FactUpdate` status moves: `Current -> Stale`, `Stale -> Current`, any live status `-> Disputed`, `Disputed -> Current`, any live status `-> Superseded`, any live status `-> Retracted`. The store enforces only "the stored row is live"; `decide.rs` produces only the moves above.

### 8.4 Retention purge and its configuration

- `RetentionPolicy` gains `pub retracted_facts: SignedDuration` (default 30 days). `PurgeReport` gains `pub retracted_facts: u64` (facts plus relations cleared). Inside the existing `Store::purge` transaction add, before the commit:

```sql
UPDATE facts SET value = NULL, search_text = NULL, live = 0, content_purged_at = ?1,
       version = version + 1
 WHERE status = 'retracted' AND retracted_at < ?2 AND content_purged_at IS NULL;
UPDATE relations SET from_ref = NULL, to_ref = NULL, live = 0, content_purged_at = ?1,
       version = version + 1
 WHERE status = 'retracted' AND retracted_at < ?2 AND content_purged_at IS NULL;
```

  `?1` is `now` in milliseconds and `?2` is `now - policy.retracted_facts`. The rows stay (the tombstone: id, subject, predicate, source, status, actor, timestamps, supersedes link, evidence reference). The FTS row is already gone because retraction set `live = 0`; the update trigger runs with `old.live = 0` and does nothing. `facts` is **not** added to `PURGED_TABLES` (rows are not deleted) and needs no `FTS_CLEANUP` entry; keep the guard test green.
- `domovoi-config`: `retracted_facts_days` becomes `applies: Applies::Now`; change its doc comment on `RetentionSection::retracted_facts_days` to "Days the content of a retracted fact or relation is kept; its tombstone stays. Default 30."; update `retention_registry_marks_the_unbuilt_keys` to expect only `operation_journal_days` and `execution_outputs_days`.
- Hub `runtime/settings.rs::retention()` maps `config.retention.retracted_facts_days` into `retracted_facts` and its doc comment says "The two keys that cover data not built yet are not read." Update `declared_retention_keys_are_applied_to_nothing` to declare only the two remaining keys and add `retracted_facts_days_reaches_the_purge_policy`.
- Update `policy_days` and the other `RetentionPolicy { .. }` literals in `crates/domovoi-store/tests/sqlite/purge.rs` (lines ~23 and ~257) with the new field.

### 8.5 N-1 upgrade test

Add `crates/domovoi-store/tests/sqlite/metric_samples.rs` (register in `main.rs`). Copy `tests/fixtures/n1-schema5.db` into a temporary directory, open it with a raw `sqlx` connection **before** `Store::open`, insert three `digest_facts` rows with explicit ids 7, 9 and 12 (the table still exists at schema 5), close, then `Store::open` and assert section 12.3's `upgrade_copies_digest_facts_into_metric_samples_keeping_ids`, `digest_facts_view_serves_the_previous_release` and `knowledge_migration_leaves_existing_tables_untouched`. Do not regenerate or modify the fixture.

## 9. Protocol and compatibility

- **No wire change.** The agent's `digest_facts` message and the `DigestFact`/`DigestFacts` types are unchanged; the hub still handles `Message::DigestFacts` at `crates/domovoi-hub/src/agent_server/session.rs:190-192` and records through the renamed port into `metric_samples`.
- **Previous release against the new schema.** It keeps working: it inserts and reads through the `digest_facts` view; it ignores the new tables (`schema_compat` rule); it never writes a `StateWrite::Knowledge`. Proven by the view test above.
- **Current release against an old agent.** No change: nothing here depends on the agent.
- **HTTP API.** New endpoints only; no existing endpoint changes. The API change is additive, so existing web code is unaffected.
- **Journal and SSE.** New journal kinds (`fact.changed`, `relation.changed`, `entity.changed`) appear in `journal`. Readers must already tolerate kinds they do not know (`JournalEntry::kind` is a string). Do not add them to any SSE topic.

## 10. Error handling and recovery

| Situation | Exact behaviour |
|---|---|
| `FactDraft` with an unknown predicate string through the API | 400 `bad_request`, nothing written |
| Value violating the predicate's schema, subject kind not allowed, bad cidr | `KnowledgeError::InvalidValue` / `SubjectNotAllowed`; API 400; nothing written |
| `record` with `source = user`, or a source the actor may not write | `ActorNotAllowed`; nothing written; no audit row |
| `host_platform` with a source other than `discovered` or `integration` | `InvalidInput("host_platform is observed, not asserted")`; nothing written |
| Version mismatch on any update | `Conflict`; API 409 "someone changed it meanwhile; reload and try again"; no journal or audit row |
| `retract`, `verify` or `accept` on a superseded or retracted fact | `NotLive`; API 409 |
| `verify` on a `management_path` | `UseConfirmation`; API 409 |
| `confirm` on a fact that is not a `management_path` | `InvalidInput`; API 400 |
| `confirm` on a `discovered` or `inferred` path | `NotOwnerFact`; API 409 "accept the path first" |
| `confirm` while `UnavailableCoverage` is wired | `CoverageUnverifiable(NotAvailable)`; API 409 "the helper's coverage cannot be verified yet"; **no confirmation row, no journal row, no audit row** |
| `confirm` when the verdict is `Uncovered` | `PathUncovered`; API 409 "the helper does not cover this path"; nothing written |
| Wrong or missing password where step-up applies | API 403, nothing written; the lockout shares the login limiter (429) |
| Two writers race on the same live value | the unique live-value index refuses the second; mapped to `Conflict` |
| The retention purge runs while a fact is being retracted | each is its own transaction on the single writer; the purge only touches rows already `retracted` |
| `entity` delete while referenced | `EntityInUse`; API 409 |
| Entity name taken (ignoring case) | `DuplicateEntity`; API 409 |
| Relation from a subject to itself, unknown kind | `InvalidInput`; API 400 |
| `sweep` fails midway | transactions already committed stay (each is a valid state); the error is returned and logged `warn`; the next run continues; hub start never fails because of the sweep |
| Backfill or platform observation fails | logged `warn`; the profile refresh and hub start still succeed; the next refresh retries (idempotent) |
| Store read fails while listing | `ApiError` from `StoreError` (existing mapping); never rendered as an empty list |

## 11. Security requirements

- **Process identity.** Nothing in this prompt runs a process or touches an agent, the helper or a skill. The knowledge service runs inside the hub.
- **Who writes `user` facts.** `source_allows_actor` is the only gate and it is checked inside the service for every write. The HTTP handlers build the draft with `source = FactSource::User` themselves and pass `current.user.id`; the request types have no `source` or `actor` field, and `KnowledgeFactInput`, `KnowledgeFactAction` and `RelationInput` carry `#[serde(deny_unknown_fields)]`, so a smuggled `source` or `actor` is a 400 and never a silent no-op.
- **Inferred never becomes user silently.** The only code path that produces a `user` row from a non-user row is `accept`, which takes a `UserId` (an authenticated session) and writes an audit entry. A system actor can neither pass `source = user` nor call `accept`.
- **Safety-critical facts.** `usable_for_safety` and `path_state` are the only definitions of "usable"; both require `source == User`, effective status `Current` and, for a management path, a confirmation. Observation and re-assertion cannot revive a stale safety-critical fact (`refresh_or_upgrade`). Only `confirm` (management paths, with step-up and a `Covered` verdict) or the owner's `verify` with step-up (`trusted_cidr`) can.
- **No attestation here, fail closed.** The coverage hook has one shipped implementation, `UnavailableCoverage`, which never reports `Covered`. A path therefore cannot reach `Confirmed` in a production hub until prompt 47 replaces the wiring. Do not add any other way to create a `PathConfirmation` (the store refuses a confirmation whose fact is not a `management_path` of the same host; the service creates it only on `Covered`). The hub never trusts an agent-supplied description of the helper; nothing in this prompt reads agent data into a confirmation.
- **Step-up.** Writes touching a safety-critical predicate (`POST /facts`, `accept`, `verify`) and every `confirm` need the password through `confirm_password`. Retraction never does.
- **Validation.** Subjects, evidence references, predicates, values, cidrs, entity names and kinds are validated in core before anything is stored; `value` is stored canonically; ids from the path are parsed or 404. SQL always binds parameters (no string-built SQL with user text); FTS queries go through `fts::match_all` so no FTS operator can come from user input.
- **Audit and journal hygiene.** Journal and audit payloads carry ids, predicate, subject, kind, host id, cause, never a fact value, a relation endpoint or an entity name (section 6.6), because they outlive the 30-day purge. A test checks this.
- **Secrets.** Facts are not a secret store. The notes the owner writes may contain anything; this prompt adds no redaction and no model context (prompt 32 redacts at context time). No fact value is logged: `tracing` calls name ids only. Do not derive `Debug` output that prints a value into a log line.
- **Retraction and purge.** Retraction removes the fact from FTS, from default listings and from every read used for model context immediately; the value stays visible to the owner only when the owner asks for retracted rows explicitly, until the purge clears it. The purge clears `value`, `search_text` and relation endpoints and keeps the tombstone. Backups keep purged content until they rotate (state this in the book page).
- **Approvals.** `HumanApproval` is untouched. Nothing here approves an action or mints `Authorization`, `PolicyPermit` or a plan hash. The closed `ReadTool` catalogue is untouched.

## 12. Testing requirements

Name tests after the behaviour. Tests never sleep: time comes from `FakeClock` (`domovoi_testkit::FakeClock`, `advance`). Use `InMemoryStore` for engine tests and a real `Store` (`common::Fresh::new()` in the store tests) for SQLite behaviour. The new fake is `FakeCoverage` **(new, `crates/domovoi-testkit/src/coverage.rs`, exported from `lib.rs`)**: `FakeCoverage::unavailable()` and `FakeCoverage::covering(&["192.168.1.0/24"])` construct it; the mode can change at run time through interior mutability: `.cover(&[..])` (answers `Covered` with evidence `"fake-evidence"` for listed cidrs and `Uncovered` for all others), `.set_unverifiable(UnverifiableReason)`, `.make_unavailable()`, and `.calls()` returns the `(host, cidr)` pairs it was asked about. `is_available()` is false only in the unavailable mode; in that mode `verdict` answers `Unverifiable { NotAvailable }`. Test builders for ids and hosts come from `domovoi_testkit::build` (`at`, `T0`, `host`, ...; verify their names).

### 12.1 Core unit tests (in-file `#[cfg(test)]`)

| Test | Setup, action, expected |
|---|---|
| `canonical_cidr_zeroes_host_bits_and_compresses_ipv6` | `192.168.1.7/24` gives `192.168.1.0/24`; `10.0.0.5` gives `10.0.0.5/32`; `2001:DB8:0:0:0:0:0:0/32` gives `2001:db8::/32`; `fd00::1` gives `fd00::1/128`; `1:0:0:2:0:0:0:3` gives `1:0:0:2::3/128` (longest zero run); `1:0:3:4:5:6:7:8` stays uncompressed (a single zero group is not compressed); `::ffff:10.0.0.7` gives `10.0.0.7/32`; `::/0` gives `::/0`; `10.0.0.0/33`, `::ffff:10.0.0.7/104`, `x`, `` give `None` |
| `subject_ref_round_trips_and_rejects_bad_text` | the four forms round-trip through `Display`/`FromStr` and serde; uppercase kind, bad uuid, a container name with `/` or space, an empty name are refused |
| `canonical_json_sorts_keys_recursively` | two objects with different key order give identical text |
| `every_predicate_validates_its_documented_values_and_rejects_others` | table-driven over section 6.2: accepted values come back canonical (`{"cidr":"192.168.1.7/24"}` becomes `{"cidr":"192.168.1.0/24"}`; strings trimmed), wrong type, extra key, missing key, out-of-range length, `ends_at <= starts_at`, wrong subject kind (`management_path` on a container) are refused |
| `effective_status_turns_stale_after_the_staleness_period` | a `role` fact current at observed T0 is `Current` at T0+179 d and `Stale` at T0+180 d; `last_verified_at` moves the deadline; stored `Disputed` stays `Disputed` |
| `valid_until_shortens_freshness` | `valid_until` before the staleness deadline wins |
| `usable_for_safety_needs_user_source_current_and_safety_critical` | true only for a `User` `management_path`/`trusted_cidr` that is effectively `Current` with a value; false for `Discovered`, `Inferred`, a `role` fact, a stale fact, a purged fact |
| `source_allows_actor_table` | the table of section 6.3 for all four actor kinds and five sources |
| `path_state_table` | candidate (discovered), unconfirmed (user, no confirmation), confirmed, stale (even with a confirmation) |
| `aggregate_and_journal_kinds_round_trip` | `as_str`/`parse` for the new variants |

### 12.2 Engine tests

`decide.rs` has unit tests for `plan_record` and `settle`; `crates/domovoi-engine/tests/services/knowledge.rs`, `knowledge_paths.rs` and `knowledge_graph.rs` (register in `tests/services/main.rs`) drive `KnowledgeService` over `InMemoryStore` with `FakeClock` and `FakeCoverage`. A helper builds the service and returns `(service, store, clock, coverage)`.

| Test | Setup, action, expected |
|---|---|
| `inferred_fact_never_becomes_confirmed_without_user` (roadmap) | `record` an `inferred` `role` fact with `system:knowledge`; assert it is `Current`, source `Inferred`, and `usable_for_safety` false; every attempt to write `source = user` with a `System` or `Agent` actor returns `ActorNotAllowed`; only `accept(id, v, user)` produces a `User` row, which supersedes the inferred one |
| `retracted_fact_not_retrievable` (roadmap) | assert a fact, `retract` it; `search`, the default `facts()` filter and `live_facts` do not return it; `facts()` with `statuses = [Retracted]` does |
| `conflicting_sources_kept_as_disputed` (roadmap) | `record` `role = "nas"` as `discovered` and `role = "backup"` as `integration`; both are `Disputed`; `all_live_facts` returns both; neither `usable_for_safety` |
| `network_change_marks_management_paths_stale` (roadmap) | a confirmed management path (via `FakeCoverage::covering`); `note_network_change(host, PeerAddressChanged)` returns its id; stored status `Stale`, `path_state` is `Stale`, the confirmation row remains; one audit `knowledge.management_path.stale`; a second call changes nothing |
| `only_a_human_actor_can_write_a_user_sourced_fact` | for each of `System`, `Agent`, `Policy` actors, `record` with `source = User` returns `ActorNotAllowed` and the store is unchanged (no fact, no journal, no audit); with `Actor::User` through `assert_fact` it works |
| `owner_accept_makes_an_inferred_fact_user_sourced_and_supersedes_it` | accept an inferred `owner_note`; the old row is `Superseded`, the new row is `User`/`Current` with `supersedes = old`, evidence copied; audit `knowledge.fact.accepted` has `from` |
| `same_source_new_value_supersedes_the_old_fact` | two `discovered` `host_platform` values; the first is `Superseded`, the second `Current` with `supersedes` set, no dispute |
| `owner_assertion_resolves_a_dispute` | disputed pair, then `assert_fact` of one value as the owner; the other row is `Superseded`, the winner is `Current` |
| `agreement_of_sources_resolves_a_dispute` | `discovered` X and `user` Y disputed; `record` `discovered` Y: X is `Superseded`, Y(user) is `Current`, no extra row for Y |
| `repeated_record_of_the_same_value_refreshes_without_a_new_row` | `record` twice with a later `observed_at`: one row, `last_verified_at` and `observed_at` updated, `effect = Refreshed` the second time, version incremented |
| `many_predicates_deduplicate_equal_values` | two `owner_note` facts with equal text are one row; different text are two rows, never disputed |
| `stale_fact_is_marked_by_sweep_and_labelled_on_read` | advance past the `role` period; the fact's `effective_status(now)` is `Stale` before the sweep while its stored status is still `Current`; `sweep()` stores `Stale` and journals `stale`; a second `sweep()` marks nothing |
| `refresh_revives_a_stale_ordinary_fact` | stale `role` fact, `record` same value same source: `Current` again |
| `reasserting_a_stale_management_path_does_not_make_it_current` | stale user path, `assert_fact` of the same cidr: `RecordEffect::Unchanged`, still `Stale`, `last_verified_at` unchanged |
| `observation_cannot_revive_a_stale_management_path` | the same with a `discovered` record |
| `confirming_a_path_needs_verified_coverage` | user path + `FakeCoverage::covering`: `confirm_management_path` stores a confirmation with the fake's evidence, fact `Current`, `path_state` `Confirmed`, audit `knowledge.management_path.confirmed`, journal `confirmed` |
| `owner_cannot_confirm_uncovered_management_path` | the fake answers `Uncovered`: `PathUncovered`, no confirmation, no audit, no journal row |
| `unavailable_coverage_refuses_every_confirmation` | `UnavailableCoverage`: `CoverageUnverifiable(NotAvailable)`; nothing written; `coverage_available()` false |
| `unverifiable_coverage_reasons_are_passed_through` | `FakeCoverage::set_unverifiable(AttestationStale)` gives `CoverageUnverifiable(AttestationStale)` |
| `confirming_a_discovered_path_is_refused_until_the_owner_accepts_it` | `discovered` path: `NotOwnerFact`; after `accept`, the new user fact can be confirmed |
| `management_path_values_are_canonical_and_deduplicated` | assert `192.168.1.7/24` then `192.168.1.0/24` as the owner: one fact, stored as `{"cidr":"192.168.1.0/24"}` |
| `verify_refuses_a_management_path_and_revives_a_trusted_cidr` | `UseConfirmation` for the path; for a stale `trusted_cidr` `verify` makes it `Current` |
| `safety_critical_facts_ignore_non_user_sources` | a `discovered` `management_path` and `trusted_cidr` are `Candidate`/unusable in `management_paths` and `usable_for_safety` |
| `host_platform_accepts_only_observation_sources` | `Inferred` and `Imported` give `InvalidInput`; `Discovered` works |
| `journal_and_audit_never_contain_fact_values` | assert, accept, confirm and retract facts whose values are distinctive strings; serialize every journal payload and audit detail and assert none contains any of them |
| `stale_version_changes_nothing` | `retract` with an old version: `Conflict`; the fact, the journal and the audit log are unchanged |
| `retract_works_on_every_predicate_and_is_audited` | `knowledge.fact.retracted` audit row, status `Retracted`, `retracted_by` the user |
| `entity_names_are_unique_ignoring_case` | create "VPN", then "vpn": `DuplicateEntity` |
| `an_entity_in_use_cannot_be_deleted` | a live fact on `entity:<id>`, a live relation endpoint, and a `depends_on` value each give `EntityInUse`; after retraction deletion succeeds |
| `only_a_user_creates_entities` | the service API takes a `UserId`; the created entity's `created_by` is `Actor::User` |
| `relation_record_is_idempotent` | `record_relation` twice: one live row, `observed_at` updated, journal `recorded` then `refreshed` |
| `relation_to_self_is_refused` and `retracted_relation_is_not_listed_by_default` | as named |
| `sweep_marks_stale_relations` | a relation past 180 days |

### 12.3 Store tests (`crates/domovoi-store/tests/sqlite/`, register in `main.rs`)

`knowledge.rs`: runs the contract suite of section 12.7 against a fresh `Store`, plus:

| Test | Expected |
|---|---|
| `retracted_fact_leaves_the_full_text_index_in_the_same_transaction` | insert and search a fact (found); retract through `FactUpdate`; `facts_fts MATCH` finds nothing; the row is still listable by status |
| `purge_clears_retracted_content_after_the_configured_days_and_keeps_the_tombstone` | retract at T0; `purge(T0 + 29 d, policy 30 d)` leaves the value; `purge(T0 + 31 d, ..)` sets `value`, `search_text` NULL and `content_purged_at`, reports `retracted_facts >= 1`, and the row keeps id, subject, predicate, source, status, actor, `supersedes_id` and `evidence_ref`; a retracted relation loses `from_ref` and `to_ref` |
| `purge_does_not_touch_live_or_superseded_facts` | a current fact and a superseded one keep their values after a purge far in the future |
| `purged_fact_cannot_be_found_by_search_or_listed_with_a_value` | after the purge, search finds nothing and the page read returns `value: None` |
| `unique_live_value_index_refuses_a_second_live_row_but_not_a_superseded_one` | inserting the same subject, predicate and value twice while live gives `Constraint`; after superseding the first, the second insert works |
| `fact_update_of_a_retracted_fact_is_refused` | `Constraint` |
| `confirmation_for_another_host_or_a_non_path_fact_is_refused` | `Constraint` for both |
| `knowledge_writes_roll_back_with_a_failing_later_write` | a transaction with a valid `FactInsert` and an invalid second write leaves no fact and no journal row |

`metric_samples.rs` (section 8.5): `upgrade_copies_digest_facts_into_metric_samples_keeping_ids` (ids 7, 9, 12 and their values preserved; a new sample gets an id above 12), `digest_facts_view_serves_the_previous_release` (run, through a raw connection, the previous release's exact statements: `INSERT INTO digest_facts (host_id, key, label, value, unit, observed_at) VALUES (...)` and `SELECT key, label, value, unit, observed_at FROM digest_facts WHERE host_id = ? AND observed_at >= ? AND observed_at < ? ORDER BY observed_at, id`; the insert lands in `metric_samples`), `knowledge_migration_leaves_existing_tables_untouched` (row counts of `incidents`, `notes`, `audit_log`, `actions` equal before and after), `metric_sample_repo_still_records_and_lists` is the renamed contract test. Also in `purge.rs`: update the helper literals and add `retracted_facts_days_policy_field_is_applied`.

### 12.4 Hub API tests (`crates/domovoi-hub/tests/hub/knowledge_api.rs`)

Use `api()` and `admin()`; the harness builds `KnowledgeService` over the test store with a `FakeCoverage` it exposes as `a.coverage` (unavailable by default). Register the module in `main.rs`.

| Test | Expected |
|---|---|
| `knowledge_endpoints_require_a_session` | every route in section 6.9 answers 401 without a cookie |
| `owner_asserts_lists_and_retracts_a_fact` | POST a `role` fact (201, `source: "user"`, `actor: "user:…"`); list finds it; retract (200); default list no longer has it; `status=retracted` list has it |
| `the_api_refuses_a_smuggled_source_or_actor` | a body with `"source": "inferred"` or `"actor"` is 400 |
| `list_filters_by_host_predicate_status_and_source` | assorted filters, plus `limit` and `next_cursor` resuming |
| `search_finds_live_facts_and_never_retracted_ones` | `q=media` finds the live fact, not the retracted one |
| `safety_critical_fact_writes_need_the_password` | POST `management_path` without a password is 403; with a wrong one 403; with `PASSWORD` 201; a `role` fact needs none; `retract` of a path needs none |
| `confirm_needs_the_password_and_verified_coverage` | with the unavailable coverage: 409 and no confirmation (`GET management-paths` shows `state: "unconfirmed"`, `coverage_available: false`); after `a.coverage.cover(...)`: 200, `state: "confirmed"`, the confirmation carries no evidence field; wrong password 403 and nothing changes |
| `a_discovered_path_must_be_accepted_before_it_can_be_confirmed` | 409 until `accept` |
| `a_stale_version_is_a_409` | the second of two actions with the same `expected_version` |
| `audit_entries_name_the_fact_but_never_its_value` | `a.audit_events("knowledge.fact.asserted")` has the predicate and subject, and the value string is absent |
| `relations_and_entities_round_trip` | create an entity (201), a relation, list both, rename, retract the relation, delete the entity; duplicate name 409; delete while referenced 409 |
| `the_openapi_document_lists_every_knowledge_route` | `domovoi_hub::router::openapi_json()` (as `openapi_snapshot` in `api_routes.rs` reads it) contains every operation id of section 6.9 |

### 12.5 Profile tests (`crates/domovoi-hub/tests/hub/profile_backfill.rs`)

| Test | Expected |
|---|---|
| `backfill_writes_a_discovered_host_platform_fact_with_note_evidence` | a stored profile and its note; after `backfill`, one `host_platform` fact: source `Discovered`, actor `system:profile`, evidence `note:<note id>`, value equal to the profile facts, `observed_at = collected_at` |
| `backfill_is_idempotent` | a second run adds no fact and changes no version |
| `backfill_keeps_the_note_and_labels_it_superseded` | the note keeps its text, id and tags `host-profile` and `generated`, gains the tag `superseded-by-facts`, and its first lines contain "_Generated by Domovoi." and "Superseded by facts" |
| `backfill_skips_hosts_without_a_profile_or_a_note_id` | no facts, no error |
| `profile_refresh_records_the_platform_fact_and_an_unchanged_refresh_adds_no_row` | two refreshes with the same facts: one fact, refreshed; `effect Refreshed` observable by `last_verified_at` |
| `a_changed_kernel_supersedes_the_platform_fact` | refresh with a new kernel: old `Superseded`, new `Current` |
| `profile_refresh_succeeds_when_knowledge_is_not_configured` | `ProfileDeps.knowledge = None`: refresh works, no fact |

### 12.6 Extra checks to run

Beyond the standard list: `cargo nextest run -p domovoi-core -p domovoi-config -p domovoi-store -p domovoi-engine -p domovoi-hub -p domovoi-testkit`; `cargo nextest run -E 'rdeps(domovoi-core)'` (core's public items changed); `cargo xtask arch-check`; `cargo xtask openapi` then `cd web && npm run gen:api && npm run typecheck && npm run lint` (generated files only); `cargo xtask docs-config`; `cargo xtask docs-links`. Review every `insta` snapshot change: the existing `openapi_snapshot` in `crates/domovoi-hub/tests/hub/api_routes.rs` must change by exactly the new knowledge paths and schemas and nothing else (`cargo insta review`).

### 12.7 The contract suite (testkit, run against `InMemoryStore` and SQLite)

`crates/domovoi-testkit/src/contracts/knowledge_store.rs` exports named functions over a new trait `KnowledgeSubject` **(new)** with `uow() -> &dyn UnitOfWork`, `knowledge() -> &dyn KnowledgeRead`, `journal() -> &dyn JournalRead`, `audit() -> &dyn AuditLog`, implemented for `InMemoryStore` in the testkit and for a `SqliteKnowledge(Store)` wrapper in `tests/sqlite/knowledge.rs` (model it on `UowSubject` in `contracts/uow/mod.rs` and `SqliteSubject` in `tests/sqlite/uow.rs`; do **not** add methods to `UowSubject`). Functions: `fact_insert_and_update_advance_versions_and_journal_together`, `stale_fact_version_conflicts_and_changes_nothing`, `retracted_fact_leaves_search_but_stays_listable_by_status`, `live_value_is_unique_per_subject_and_predicate`, `retracted_fact_cannot_be_updated`, `confirmation_upserts_and_is_read_back`, `relation_live_edge_is_unique`, `entity_names_are_unique_ignoring_case_and_versions_are_checked`, `facts_page_orders_newest_first_and_resumes_from_the_cursor`, `search_matches_whole_words_in_live_facts_only`, `entity_in_use_sees_subjects_endpoints_and_depends_on_values`. The in-memory twin must reproduce SQLite's conflict, uniqueness and liveness behaviour; the shared suite is what proves it.

## 13. Acceptance criteria

1. After `Store::open` on a fresh database and on the N-1 fixture, tables `metric_samples`, `facts`, `facts_fts`, `relations`, `entities` and `management_path_confirmations` exist, `digest_facts` is a view, and `PRAGMA quick_check` is `ok`.
2. Rows that were in `digest_facts` appear in `metric_samples` with their ids and values; the previous release's `INSERT INTO digest_facts` and `SELECT ... FROM digest_facts` still work and the insert lands in `metric_samples`; `MIN_READER` is unchanged.
3. `grep -rn "FactRepo" crates` finds only `MetricSampleRepo`, and the wire message `digest_facts` and the type `DigestFact` are unchanged.
4. The migration contains no `CHECK` for a new enum, and no merged migration was edited.
5. `canonical_cidr` returns the 04b canonical form for the vectors in section 12.1.
6. A `source = user` fact can be written only with `Actor::User`; `System`, `Agent` and `Policy` actors are refused for it, and the HTTP request types cannot carry a source or an actor.
7. An `inferred` fact becomes `user`-sourced only through `accept` by an authenticated user, which audits `knowledge.fact.accepted`.
8. Different values from different sources for a single-valued predicate leave both facts `Disputed` until the owner asserts a value or the sources agree.
9. A fact past its staleness period (or `valid_until`) reads as `Stale`; the sweep stores that and journals it; observation and re-assertion never revive a stale `management_path` or `trusted_cidr`.
10. A retracted fact is absent from `search`, from the default list and from every live read, and its FTS row is gone in the transaction that retracted it.
11. After `retracted_facts_days` (default 30), `Store::purge` sets the retracted fact's `value` and `search_text` (and a retracted relation's endpoints) to NULL, keeps the tombstone, and does not touch live or superseded facts; `retracted_facts_days` now has `Applies::Now`.
12. Journal entries and audit entries written by the knowledge service never contain a fact value, a relation endpoint or an entity name.
13. Management-path confirmation needs the password, a user-sourced current-or-stale `management_path` and a `Covered` verdict from `ManagementCoverage`; with the shipped `UnavailableCoverage` every confirmation returns 409 and writes no row, no journal entry and no audit entry.
14. `note_network_change` marks the host's current management paths `Stale` with an audit entry, and `path_state` reports `Stale`.
15. `POST/accept/verify` on a safety-critical predicate and every `confirm` need the password (403 without, 429 when locked); `retract` does not.
16. The knowledge endpoints of section 6.9 exist with those methods, paths, operation ids and status codes; `web/src/api/openapi.json` and `schema.d.ts` were regenerated by the commands, not edited; `npm run typecheck` passes.
17. The profile backfill writes one `discovered` `host_platform` fact per host with a stored profile and a note id, with `evidence_ref = note:<id>`; running it twice changes nothing; the note keeps its text and gains the `superseded-by-facts` tag and the label.
18. A profile refresh records or refreshes the platform fact; a knowledge failure there is logged and does not fail the refresh or hub start.
19. The hourly sweeper is spawned once at hub start, runs once at start, and stops on shutdown.
20. The in-memory store and SQLite both pass the `knowledge_store` contract suite.
21. Every named test in section 12 exists and passes; no existing test was deleted or weakened, and the retention registry test and the `declared_retention_keys` test were updated only as described.
22. `cargo fmt --all`, `cargo clippy -p <crate> --all-targets -- -D warnings` for each edited crate, the scoped nextest runs, `cargo xtask arch-check` and `cargo xtask docs-links` are green; no source file exceeds 500 lines (web files untouched).

## 14. Migration and compatibility

- **Existing installs.** The migration is additive except for `digest_facts`, which becomes a view over `metric_samples` after its rows are copied in the same transaction. A pre-migrate backup is taken by the existing mechanism. Nothing else changes for existing data; no setting is added; no agent action is needed.
- **Host profile notes** are kept untouched except for the label (a tag and a line of text). Their facts appear at the first hub start after the upgrade (the backfill) and stay current through profile refreshes.
- **Rollback.** Per RW-31 the previous binary runs against the new schema: it ignores the new tables and uses the view. A rollback after knowledge was written loses nothing; the new tables simply stay unused.
- **The view is temporary.** It exists for one release; a later migration removes the view and trigger. Record that in the migration's header comment and in `data-model.md`.
- **Agents.** Unchanged (no wire change).

## 15. Documentation requirements

- `docs/book/src/reference/data-model.md`: add rows for `metric_samples` (and say `digest_facts` is a compatibility view kept for one release), `facts`, `facts_fts`, `relations`, `entities`, `management_path_confirmations`; change the retention table row to "Retracted fact content | `retracted_facts_days` | 30 days" (remove "not built yet"); change "digest facts are kept" to "metric samples (formerly digest facts) are kept".
- `docs/book/src/guide/retention.md`: the `retracted_facts_days` row now says "the content of a retracted fact or relation (the tombstone stays)" and "Kept: the tombstone, live facts, superseded facts"; update the paragraph that lists what is kept; keep the backup guarantee text.
- `docs/book/src/reference/configuration.md`: regenerate with `cargo xtask docs-config` (do not hand-edit); the source of the text is the doc comment you changed.
- `docs/book/src/reference/api.md`: a new "Knowledge" section describing the endpoints, the step-up rule, the forced `user` source, and that confirmation is unavailable until the helper attestation feature ships.
- `docs/book/src/concepts/knowledge.md` **(new)**, added to `docs/book/src/SUMMARY.md` under Concepts: what a fact is (subject, predicate, source, status, freshness), who may assert what, disputes, retraction and the 30-day purge (and that backups keep purged content until they rotate), management paths and why they cannot be confirmed yet. Plain, short, no mention of prompts or plans. Run `cargo xtask docs-links`.
- `ARCHITECTURE.md`: add `### 5.8 Knowledge` (the types, ownership and lifecycle in six to ten lines), add `KnowledgeRead` and `ManagementCoverage` rows to the §6 port table, update the §9 table list (`metric_samples`, `facts`, `facts_fts`, `relations`, `entities`, `management_path_confirmations`, and the `digest_facts` view) and add one line in §10 for `/api/v1/knowledge`. Do not change §8 (the wire).
- Regenerate OpenAPI: `cargo xtask openapi && cd web && npm run gen:api`. Never hand-edit `web/src/api/openapi.json` or `web/src/api/schema.d.ts`.
- Committed files, commit messages and the PR description never mention prompts, prompt numbers, the build plan, coding agents or AI assistance. Write "arrives with the helper attestation work" or "coming in v0.1", not a prompt id.

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
