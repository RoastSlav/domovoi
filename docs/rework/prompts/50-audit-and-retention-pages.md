# 50 — Audit page and retention settings

| | |
|---|---|
| Branch | `audit-and-retention-pages` |
| Milestone | U4 |
| Depends on (must be merged) | 37 (visibility profiles), 30 (shell, kit additions, `npm run smoke`), and through them 29, 28, 27 and 26. 09 (audit hash chain) and 57 (configurable retention) are **already merged**: cite their real code. Prompts 26 to 30 and 37 are written but may not be merged when this brief is read: **every name taken from them below is "introduced by prompt NN — verify in code before use"**. If 37 or 30 is not merged, stop and report: this prompt classifies its elements with 37's categories and extends 30's browser smoke test |
| Can run in parallel with | 43, 48, 49, 51 (order-flexible, never concurrent). 51 edits the same shell files (`web/src/app/nav.ts`, `pages.ts`, `routes.tsx`), `web/src/design/visibility.ts` (`VISIBILITY_PAGES`), `web/DESIGN.md`, `web/scripts/smoke/*`, `web/scripts/screens.mjs` and `crates/domovoi-hub/src/api/mod.rs`; whichever runs second rebases on the first |
| Produces (shared contracts) | (1) The read-only port `domovoi_core::ports::AuditInspect` (rows with their chain fields, the anchor, the head, verification) implemented by `Store`. (2) `GET /api/v1/audit` and `GET /api/v1/audit/verification` with their DTOs. (3) `SettingsDto.retention` (the nine D-21 periods and whether each applies yet). (4) The web Audit page `/audit`, the design vocabularies `LEGACY_STATUSES` and `CHAIN_STATUSES` (`legacyStatusMeta`, `chainStatusMeta`), the settings section `retention` and the page key `audit` in `VISIBILITY_PAGES` |
| Consumes | 09: `Store::{verify_audit, audit_head, audit_anchor}`, `domovoi_core::audit_chain::{AuditAnchor, ChainRow, ChainStatus, LegacyStatus, BreakReason, VerifyReport, VerifyScope}`. 57: `RetentionSection`, `RETENTION_KEYS`, `backup.keep_pre_migrate`, the `retention` step-up rule in `settings/step_up.rs` (19). 37: `Visible`, `DetailsScope`, `VisibilityPage`, `useVisibility`, `expectP0Everywhere`, `renderWithProfile`, `VISIBILITY_PAGES`, the §6.5 category rule. 30: `npm run smoke` and its mock hub. Existing: the settings editor (`SectionSpec`, `FieldSpec`, `PasswordPrompt` flow), the kit |
| Launch gate | none |
| Suggested model | small: one read-only hub surface, one page, one settings section; every decision is made here |

## 1. Objective

Give the person who runs the servers two things they cannot see in the web UI today, built only on what the hub already records:

- **An Audit page** (`/audit`) that shows the audit log as the two parts the hash chain really has, **separately**:
  the **legacy** history (rows written before the chain existed; unchained, checked as a count and one digest) and the
  **chained** history (every row since; each linked to the one before). Each part has its own verification status in
  words, the chain head hash can be copied for safekeeping off the host, and a check of the whole log can be run on
  request. The page says plainly what the chain can and cannot prove (it does not overclaim).
- **A Retention section in Settings** for the D-21 values: the nine `[retention]` periods (editable through the existing
  settings editor, with each field's source and lock), the two backup counts (the existing Backups section gains the
  missing `backup.keep_pre_migrate` field), what is **kept and has no period** (operations and their summaries, the
  audit log), the honest statement of what deletion guarantees (deleted data stays in backups until they rotate), and
  the password re-entry the hub already demands when a period changes. A hub whose settings are managed by file shows the
  same values read-only.

The hub has **no audit read route today** (`crates/domovoi-hub/src/api/mod.rs` has none) and `SettingsDto` carries no
retention values, so this prompt adds the minimal read-only backend the pages need: one core port, its SQLite
implementation, two read handlers under `api/`, one additive field on `SettingsDto`. Everything is read-only: no route
changes, appends, repairs or prunes the audit log, and no route changes retention except the existing settings save.

Not a goal: any audit write, pruning, a checkpoint writer, a restore flow, filtering UI, export, or changing who may
read the log (one signed-in owner, D-13).

### Observations to carry into the completion report

There is no blocker. Two findings the executor reports (the first is fixed here only as far as stated):

1. The hub's step-up list (`STEP_UP_SETTINGS`, `crates/domovoi-hub/src/settings/step_up.rs`) already covers `retention.*`,
   `metrics` and `llm.*.cli`, but the **web** copy of that decision, `isSensitive` in
   `web/src/features/settings/form/sensitive.ts`, knows only secrets, `discord.allowed_user_ids`, `web.trusted_proxies`
   and `webhooks`. A retention save from the UI would therefore be refused with 403 "enter your password to make this
   change" and shown as a generic failure. This prompt fixes it **for `retention` only** (section 6.8); `metrics` and
   `llm.*.cli` stay as they are and are reported.
2. `backup.keep` and `backup.keep_pre_migrate` are D-21 retention values but are **not** in the step-up list (only
   `retention.*` is). The owner decided (D-34) that they need the password too: this prompt adds them (section 6.8a).

## 2. Architectural context

Everything needed is here; the cited documents are background.

**D-14 / RW-27 (audit hash chain, merged by prompt 09).** Every audit row since the chain began carries `prev_hash` and
`hash` (SHA-256 over the row's sequence number, previous hash and canonical JSON). One `audit_anchor` row, written by
`Store::open`, commits to everything written **before** the chain: an install id, `legacy_last_id` (highest legacy
sequence number; 0 on a fresh install), `legacy_count` and `legacy_digest`. Legacy rows are never rewritten; they are
checked as a **count and a digest**: a mismatch says "legacy rows changed" and cannot say which. Chained rows are checked
link by link; the first broken link is reported with a reason. The two parts fail differently, so the page, like
`domovoi audit verify`, reports them **separately**.

**Threat model, stated honestly.** The chain detects accidental corruption, edits made by tools that do not recompute
the chain, mid-chain deletion and rows inserted without the chain. It does **not** stop someone who rewrites the whole
database and recomputes every hash, and it does not detect removal of the **newest** rows unless the head hash was kept
somewhere else. That is why the page shows the head hash with a copy button ("the cheap anchor", RW-27). The page and
the book say this in these words; neither may say the log is "tamper-proof" or "secure".

**Failure policy (decided by 09).** A broken chain does not stop the hub: it is evidence, not a gate. At start the hub
verifies the newest 10 000 chained rows (`STARTUP_AUDIT_VERIFY_ROWS`, `crates/domovoi-hub/src/runtime/mod.rs:56`), logs
an error and appends one chained `audit.chain_broken` row (once per distinct first broken sequence). The page shows that
row like any other and the verification card shows the break; nothing here repairs it.

**Pruning and checkpoints (RW-27).** Audit rows are retained forever by default (D-21). If pruning is ever configured, a
chained `audit.checkpoint` row is written first, and verification starts at the newest checkpoint; pruning legacy rows
records `legacy_pruned` and the legacy part then reads "not verifiable". No pruner exists yet. The page must already
render all three legacy outcomes (`verified`, `count_only`, `mismatch`) plus `not_verifiable`, and treat a checkpoint row
as an ordinary row with a plain-language kind hint.

**D-09 / D-21 / RW-28 (configurable retention, merged by prompt 57).** Nine periods are settings in `[retention]`
(`signals_days` 30, `incident_timeline_days` 90, `conversations_days` 180, `notifications_days` 30, `triage_cache_days`
90, `resolved_advisories_days` 30, `operation_journal_days` 180, `execution_outputs_days` 30, `retracted_facts_days` 30),
each 1 to 3650 days (zero is refused so a typo cannot delete everything). Three of them are **declared but applied to
nothing yet** (`operation_journal_days`, `execution_outputs_days`, `retracted_facts_days`; the registry
`RETENTION_KEYS` in `crates/domovoi-config/src/hub/retention.rs` says what each waits for). Operations, resolution
summaries and the audit log have **no period** (they are retained); incidents, digest facts and notes are kept too.
Backups are kept by **count** (`backup.keep` 7, `backup.keep_pre_migrate` 3), not days.

**What deletion guarantees (04 §14).** A purge removes data from the live database, its full-text index and model
context in one transaction. It does **not** remove it from backups: deleted rows stay in the nightly backups until they
rotate out (up to `backup.keep` of them) and in the last `backup.keep_pre_migrate` pre-upgrade copies, and restoring a
backup brings them back until the next purge. The retention settings must say so.

**RW-24 (step-up).** Changing retention can destroy data sooner, so the hub requires the admin's password again for any
`retention.*` change (rule `Under("retention")` in `STEP_UP_SETTINGS`, enforced by `SettingsService::update`, `settings/service.rs:232`, which answers
403 with "enter your password to make this change" when the password is missing). The web must ask for the password **before** sending, as it does for secrets.

**RW-19 / prompt 29 (use-case layer).** Commands go through `crate::services`; "reads may stay in the transport that
renders them" (a read shared by two transports lives in a service). Both new routes are reads used only by the web, so
they are `api/` read handlers over a repository port, like `logs::list` or `overview::show`. They hold no engine handle.

**RW-21 / D-10 (profiles, prompt 37).** Every element of the Audit page carries one visibility category through 37's
kit; P0 (what was recorded, current state, failures, loading and error states) is never hidden or collapsed in any
profile. Section 6.5 of this brief gives the table. Profiles are display only: they never change what the hub returns.

**Decisions made here (do not revisit).**

1. **One new read port, `AuditInspect`**, beside `AuditLog` (which stays append and list only). It mirrors
   `StoreHealth`: core trait, `Store` implementation, `InMemoryStore` implementation, an `Arc<dyn AuditInspect>` field on
   `ApiDeps`. It exposes rows **with** their chain fields, which `AuditLog::list` deliberately does not.
2. **History is classified by sequence number against the anchor** (`seq <= legacy_last_id` is legacy), never by "has a
   hash": a chained row that lost its hash must still read as chained (and break the chain), not as legacy.
3. **Two routes.** `GET /api/v1/audit` lists rows (newest first, cursor, optional `history` and `kind`);
   `GET /api/v1/audit/verification?scope=recent|full` verifies. Both are `GET` because both change nothing. The default
   `recent` scope is the same bound as the start-up check (the newest 10 000 chained rows plus the legacy **count**);
   `full` is asked for by the person (it also recomputes the legacy digest) and is never run automatically.
4. **A verification failure is never shown as a pass.** If the store cannot verify (for example the anchor row was edited:
   `StoreError::Backend("audit anchor hash does not match its contents")`), the route answers 500 and the page shows a
   `LoadError`; it never renders "intact".
5. **Row detail is text, redacted and bounded.** `detail` is the row's JSON as compact text, passed through the hub's
   `Redactor` and cut at 4096 bytes (on a character boundary) with `detail_truncated`. Hashes are shown exactly as the hub
   sends them (64 lower-case hex characters); the browser never hashes or compares.
6. **The page is `/audit`, a nav item "Audit log" in the existing System group** (`nav.ts:85` has the comment "The audit
   log goes here when it exists"). The two parts are two tabs of one list (`?tab=chained|legacy`, chained first), and the
   verification cards sit above the tabs so both parts' status is always visible.
7. **Retention is a settings section** (`retention`, in `UPKEEP_SECTIONS` after Backups), not a new page: the periods are
   settings, and the settings editor already gives sources, locks, validation, the save bar and the password flow. A
   shared read-only view draws what is kept and what deletion guarantees, in the editor and in the file-managed view.
8. **`SettingsDto.retention` is added** (additive) so a hub managed by file can show its retention values. It is built
   from `RETENTION_KEYS` and `HubConfig.retention`, so a later prompt that makes a declared key apply (it changes
   `Applies::Once` to `Applies::Now`) changes what the page says with no web change.
9. **No new nav item or page for retention**, no change to the purge, the config schema or the migrations.

## 3. Repository context

Verify each item in step 1. Marks: **(existing, path:line)**, **(new)**, or **(introduced by NN — verify)**. Paths are
relative to `C:\Programming\domovoi`; web paths start with `web/`.

### 3.1 What the audit chain gives us (merged, prompt 09)

- `crates/domovoi-core/src/ports/repos/audit.rs` **(existing)**: `AuditEvent { at, actor: String, kind, subject, detail:
  Value }` (`:9`), `AuditFilter { actor, kind, subject, since }` (all `Option`, exact match, `Default`, `PartialEq, Eq`),
  `trait AuditLog { append, list }` (`:41`). `list` returns `Paged<AuditEvent>` newest first and **drops the sequence
  number and both hashes**: that is why a new port is needed.
- `crates/domovoi-core/src/audit_chain/mod.rs` **(existing)**: `type Hash = [u8; 32]`, `CHECKPOINT_KIND = "audit.checkpoint"`,
  `ChainRow { seq: u64, prev_hash: Option<Hash>, hash: Option<Hash>, event: AuditEvent }` (`:27`), re-exports of
  `AuditAnchor { install_id, legacy_last_id, legacy_count, legacy_digest }` (`encode.rs:11`, `hash()` gives the anchor
  hash), `LegacyStatus`, `ChainStatus`, `BreakReason`, `VerifyReport`, `VerifyScope`.
- `crates/domovoi-core/src/audit_chain/verify.rs` **(existing)**:

```rust
pub enum LegacyStatus { Verified { count }, Mismatch { expected_count, actual_count, digest_matches },
                        CountOnly { count }, NotVerifiable }                                  // :144
pub enum ChainStatus  { Empty, Intact { checked, head_seq, head_hash },
                        Broken { checked, first_broken_seq, reason: BreakReason } }           // :170
pub enum BreakReason  { MissingChainFields, HashMismatch, LinkMismatch, SequenceGap, CheckpointMismatch } // :23, Display = the words
pub struct VerifyReport { legacy: LegacyStatus, chained: ChainStatus, anchor: AuditAnchor }   // :195
pub enum VerifyScope  { Full, Last(u64) }                                                     // :206
```
  `BreakReason`'s `Display` words: "a row has no hash chain fields", "a row was edited", "a row was removed or inserted",
  "rows are missing", "the retained start does not match the checkpoint".
- `crates/domovoi-store/src/audit_verify.rs` **(existing)**: `Store::verify_audit(scope) -> Result<VerifyReport,
  StoreError>` (`:22`; `Full` checks the legacy digest and every retained chained row, `Last(n)` only counts legacy rows and
  checks the newest n chained rows), `Store::audit_head() -> Option<(u64, Hash)>` (`:34`, the newest row **with a hash**),
  `Store::audit_anchor() -> Option<AuditAnchor>` (`:40`; errors when the stored anchor hash does not match its contents).
  Reads use `self.read` (the read pool): verification never blocks the writer.
- `crates/domovoi-store/src/audit_chain.rs` **(existing, 346 lines)**: private `struct AuditChainRow` with the eight
  columns and private `fn into_chain_row` (`:22`, `:34`), `pub(crate) const BATCH = 1000` (`:19`),
  `pub(crate) rows_after(conn, after_seq, max_seq, limit)` (`:239`, ascending), `pub(crate) newest_chained` (`:225`),
  `checked_anchor`. Table `audit_log(seq, at, actor, kind, subject, detail, prev_hash, hash)` and `audit_anchor` (one row,
  append-only triggers) come from `migrations/0001_init.sql` and `0009_audit_chain.sql`.
- `crates/domovoi-store/src/repos/audit.rs` **(existing)**: `impl AuditLog for Store`; `list` binds the filter and a seq
  cursor (`Cursor::bind`, `paginate`, `fetch_limit` from `codec.rs`), `ORDER BY seq DESC`. Copy its shape for `rows`.
- `crates/domovoi-store/src/health.rs`, `crates/domovoi-core/src/ports/store_health.rs`,
  `crates/domovoi-testkit/src/repos/health.rs` **(existing)**: the three-file pattern for a small read port (trait in
  core, `impl for Store`, `impl for InMemoryStore`). `crates/domovoi-testkit/src/repos/audit.rs` keeps audit events in a
  `Vec` and reports no chain (its doc says so).
- Hub start: `crates/domovoi-hub/src/runtime/adapters.rs:63` `verify_audit_on_start` appends `audit.chain_broken`;
  `STARTUP_AUDIT_VERIFY_ROWS = 10_000` at `crates/domovoi-hub/src/runtime/mod.rs:56`. CLI: `crates/domovoi/src/cmd/audit.rs`
  (`render_report` prints "Legacy history:" and "Chained history:" lines: the words to match).
- Docs that exist: `docs/book/src/reference/data-model.md` (heading "Legacy and chained audit history"),
  `reference/cli.md:162` (`domovoi audit verify`), `reference/backup.md:12`, `ARCHITECTURE.md:279`.

### 3.2 Hub API today

- `crates/domovoi-hub/src/api/mod.rs` **(existing)**: `pub struct ApiDeps` (`:135`) with `audit: Arc<dyn AuditLog>`
  (`:183`) and `redactor: Arc<dyn Redactor>`; `ApiDoc` `tags(...)` (`:295`, the last tag is `system` at `:301`);
  `pub fn routes()` (`:322`) with one `.routes(routes!(...))` line per handler; `pub(crate) fn page(cursor, limit)`
  (`:267`, default 50, clamp 1..=`MAX_PAGE` = 200); `Authed` extractor. There is **no** audit route.
  `ApiDeps` is built in three places: `runtime/mod.rs:329`, `examples/dev_hub.rs:529`, `tests/hub/api_harness.rs:457`
  (all `audit: store.clone()`; the store implements every port).
- `crates/domovoi-hub/src/api/logs.rs` **(existing)**: the model of a small read handler (`#[utoipa::path]`, `Authed`,
  `state.deps.redactor.redact(..)` on served text). `api/dto_query.rs` **(existing)**: query structs with `IntoParams`
  (`IncidentQuery` is the cursor and limit model). `api/error.rs`: `ApiError::{BadRequest, NotFound, Internal, ...}`,
  `ApiError::internal(context, err)`, `From<StoreError>`.
- `crates/domovoi-hub/src/api/settings.rs` **(existing)**: `SettingsDto` (`:124`, fields `hub`, `llm_primary`,
  `llm_secondary`, `triage`, `discord`, `webhooks`, `heartbeat`, `usage`, `editable`) built in `show` (`:233`) from
  `deps.config` (`HubConfig`).
- `crates/domovoi-config/src/hub/retention.rs` **(existing)**: `RetentionSection` (nine `u32` fields), its **`pub(crate)`**
  `fn days(&self) -> [(&'static str, u32); 9]` (`:62`), `Applies::{Now, Once(&'static str)}`,
  `RetentionKey { key, default_days, applies }`, `RETENTION_KEYS` (nine entries, the order of the table in section 2),
  re-exported as `domovoi_config::hub::{Applies, RETENTION_KEYS, RetentionKey, RetentionSection}`. `validate.rs:128` refuses
  a period outside 1 to 3650 with the issue path `retention.<key>` and message "must be 1 to 3650 days".
  `BackupSection { dir, keep, keep_pre_migrate }` (`hub/mod.rs`, defaults 7 and 3).
- `crates/domovoi-hub/src/settings/step_up.rs` **(existing)**: `STEP_UP_SETTINGS` with `Under("retention")` (`:60`);
  `path_needs_step_up`. `settings/service.rs:232` demands the password for a sensitive change.
- Hub tests: `crates/domovoi-hub/tests/hub/api_harness.rs` **(existing)**: `api()`, `Api { store: Arc<Store>, clock,
  audit, .. }`, `a.admin().await -> Login`, `a.get(uri, &login) -> Reply { status, body, text }`, a real SQLite store in a
  temp directory (`Store::open`), `a.store.pool_for_tests()` for raw SQL; `RETENTION_LOCKING_VAR` and `SETTINGS_FILE` lock
  `retention.signals_days` (env) and `retention.incident_timeline_days` (file) in the settings tests.
  `tests/hub/api_overview.rs`, `api_logs.rs` are the model of a small read-endpoint test file; `tests/hub/main.rs`
  registers test modules; `tests/hub/api_routes.rs:529` `openapi_snapshot` compares the generated OpenAPI with
  `tests/hub/snapshots/hub__api_routes__openapi.snap`; `tests/gates.rs` is the hub's gate file.
- Store tests: `crates/domovoi-store/tests/sqlite/audit_chain.rs` **(existing)** has `open_fixture(dir)` (copies the
  committed `tests/fixtures/n1-schema5.db`, which holds **five legacy audit rows**, and opens it), `append`, `drop_trigger`,
  `recreate_trigger`, `stored_hash`; `common.rs` has `Fresh`; `tests/sqlite/main.rs` registers modules.

### 3.3 Web today

- Shell: `web/src/app/nav.ts` **(existing)** `NAV_GROUPS` (`:36`); the System group is `{ title: "System", items: [Settings] }`
  with the comment "The audit log goes here when it exists." (`:85`). `web/src/app/pages.ts` `PAGES` (lazy pages with
  `preload`), `web/src/app/routes.tsx` (routes under `<Layout/>`), `web/src/app/PageTitle.tsx` (`defaultDetail`, only for
  detail pages). The command palette lists every nav item automatically (`AppPalette.tsx`).
- Settings: `web/src/features/settings/` **(existing)**: `SettingsPage.tsx` shows `SettingsEditor` (sections) only when
  `settings.editable` is set, else `ReadOnlySettings` (176 lines; a column of `Block title=...` entries ending with
  "Advisories" (`:167`) and "About and updates" (`:170`); prompt 37 adds a "Display" block before "Advisories").
  `sections/upkeep.ts` **(existing)** `UPKEEP_SECTIONS`: `exposed-secrets`, `backups` (fields `backup.dir`, `backup.keep`
  only), `updates`. `sections/types.ts` `SectionSpec { id, title, intro?, fields(values), view?, test?, extra? }`;
  `SectionBody.tsx` draws `view` above the fields. `form/types.ts` `FieldSpec { path, label, kind, hint?, placeholder?, ... }`
  with kind `number`. `form/sensitive.ts` `isSensitive(changes)` (**does not know `retention`**) used at
  `EditorForm.tsx:136`; the password dialog's reason text is the literal at `EditorForm.tsx:224` ("This change affects who
  can act or what is trusted. ..."). `EditorForm.tsx` is **233 lines**: stay under 250.
  Help text for a field comes from the schema's rustdoc (`form/schema.ts` `describe`); the retention rustdoc already says
  "Not applied yet: applies once operations exist." for the three declared keys.
- Tests: `web/src/test/settingsFixture.ts` **(existing)**: `settings(editable)` (the `SettingsDto` answer; it will need the
  new `retention` field) and `routeSettingsPage()`; `settingsFields.test.tsx:107` `sensitive_save_prompts_for_password`
  is the model of a password-flow test (dialog name "Confirm with your password", body
  `{ base_revision, changes, password }`). `settings.test.tsx`, `system.test.tsx` render the settings page.
- Data layer: `web/src/api/hooks/system.ts:14` `useSettings()`; `web/src/api/types.ts` DTO aliases from `schema.d.ts`;
  `web/src/api/queryClient.ts` `keys`; `call`, `defined` helpers. `web/src/design/status.ts` (`Table<K>`, `StatusMeta`,
  `lookup`), `design/tone.ts` (**208 lines before prompt 30; the cap is 250**) with `VOCABULARIES` (`:188`);
  `design/icons.tsx` has no shield or lock icon: use `check-circle`, `x-circle`, `alert`, `info`, `clock`, `ban`.
- Kit pieces used: `PageHeader`, `Section`, `Card`, `Grid`, `Stack`, `Cluster`, `Tabs` (controlled with `selected` and
  `onSelect`, ids), `DataTable` (`columns`, `rows`, `rowKey`, `caption`, `empty`, `rowState`), `KeyValueList`, `StatusPill`,
  `Tag`, `RelativeTime`, `CopyBlock`, `CopyButton`, `Button`, `Dialog`, `InlineMessage`, `InfoTip`, `Progress`,
  `QueryBoundary`, `LoadError`, `EmptyState`, `Skeleton`, `LinkButton`.
- Prompt 37 **(introduced by 37 — verify)**: `web/src/design/visibility.ts` (`VISIBILITY_PAGES`), kit `Visible`,
  `DetailsScope`, `VisibilityPage`, `src/test/visibility.tsx` (`renderWithProfile`, `expectP0Everywhere`, `p0Elements`), the
  source gate `src/features/operations/visibilityGate.test.ts` (tests `visible_usage_follows_the_rules` and
  `every_p0_id_of_the_table_appears_in_the_sources`). Prompt 30 **(introduced by 30 — verify)**: `web/scripts/smoke.mjs`
  and `scripts/smoke/{server,data,checks}.mjs` (a mock hub that **fails the run on any unlisted `/api` request**),
  `web/src/test/rightSlot.tsx`.

## 4. Exact scope

**(new)** means the file does not exist yet. Every web source file stays at or under 250 lines and every non-test Rust
file at or under 500.

### 4.1 Core, store, testkit (the read port)

| File | Change |
|---|---|
| `crates/domovoi-core/src/ports/audit_inspect.rs` **(new)** | `AuditHistory`, `AuditRowFilter`, `trait AuditInspect` (6.1) |
| `crates/domovoi-core/src/ports/mod.rs` | `pub mod audit_inspect;` and `pub use audit_inspect::{AuditHistory, AuditInspect, AuditRowFilter};` |
| `crates/domovoi-store/src/audit_inspect.rs` **(new)** | `impl AuditInspect for Store` (`rows`; `anchor`, `head`, `verify` delegate to the existing methods) |
| `crates/domovoi-store/src/audit_chain.rs`, `src/lib.rs` | make `AuditChainRow` and `into_chain_row` `pub(crate)`; `mod audit_inspect;` |
| `crates/domovoi-testkit/src/repos/audit_inspect.rs` **(new)**, `repos/mod.rs` | `impl AuditInspect for InMemoryStore` (6.1: lists the events without chain fields; no anchor; `verify` is an error that says there is no chain) |
| `crates/domovoi-store/tests/sqlite/audit_inspect.rs` **(new)**, `tests/sqlite/main.rs` | the store tests of 12.1 |

### 4.2 Config and hub

| File | Change |
|---|---|
| `crates/domovoi-config/src/hub/retention.rs` | `RetentionSection::days` becomes `pub` (it has a doc comment already); nothing else |
| `crates/domovoi-hub/src/api/dto_audit.rs` **(new)** | `AuditQuery`, `VerificationQuery`, `AuditRowDto`, `AuditPageDto`, `AuditVerificationDto`, `LegacyHistoryDto`, `ChainedHistoryDto`, `AuditAnchorDto`, `HeadDto`, the mappers and their unit tests (6.2, 6.3) |
| `crates/domovoi-hub/src/api/audit.rs` **(new)** | the handlers `list` and `verification` (6.2) |
| `crates/domovoi-hub/src/api/mod.rs` | `mod audit; mod dto_audit;`, `ApiDeps.audit_inspect: Arc<dyn AuditInspect>`, tag `("audit", "The audit log and its verification")`, `.routes(routes!(audit::list))`, `.routes(routes!(audit::verification))` |
| `crates/domovoi-hub/src/api/settings.rs` | `RetentionSettings`, `RetentionEntry` DTOs and `SettingsDto.retention` (6.4); built in `show` |
| `crates/domovoi-hub/src/runtime/mod.rs`, `examples/dev_hub.rs`, `tests/hub/api_harness.rs` | `audit_inspect: store.clone()` in the three `ApiDeps` constructions |
| `crates/domovoi-hub/tests/hub/api_audit.rs` **(new)**, `tests/hub/main.rs` | the HTTP tests of 12.2 |
| `crates/domovoi-hub/tests/hub/settings.rs` or `api_overview.rs` | one retention-in-settings test (12.2) |
| `crates/domovoi-hub/tests/gates.rs` | the gate `audit_inspection_is_read_only_and_only_the_audit_routes_use_it` (12.2); extend, never weaken |
| `crates/domovoi-hub/tests/hub/snapshots/hub__api_routes__openapi.snap` | regenerated and **read** with `cargo insta review` |
| `web/src/api/openapi.json`, `web/src/api/schema.d.ts` | **regenerated**, never hand-edited (`cargo xtask openapi && cd web && npm run gen:api`) |

No `Cargo.toml` change, no migration, no change to `domovoi-engine`, `domovoi-runner`, `domovoi-proto`, `xtask` or the
`domovoi` binary. `cargo xtask arch-check` still runs once (a core port and a hub module file were added).

### 4.3 Web: design vocabulary, data layer

| File | Change |
|---|---|
| `web/src/design/auditVocab.ts` **(new)** | `LEGACY_STATUSES`, `CHAIN_STATUSES`, `legacyStatusMeta`, `chainStatusMeta` (6.5) |
| `web/src/design/auditText.ts` **(new)** | `breakReasonText(code, fallback)`, `auditKindHint(kind)`, `scopeSentence(scope, limit)` (6.5) |
| `web/src/design/status.ts` | types `LegacyStatus`, `ChainStatus` |
| `web/src/design/tone.ts` | one import line, one `export { chainStatusMeta, legacyStatusMeta }` line, two `VOCABULARIES` entries (**stay at or under 250 lines**; see 6.5 for why the lookups live in `auditVocab.ts`) |
| `web/src/api/types.ts` | aliases `AuditPage`, `AuditRow`, `AuditVerification`, `RetentionSettings` |
| `web/src/api/queryClient.ts` | `keys.audit` = `["audit"]`, `keys.auditRows(history)`, `keys.auditVerification(scope)` |
| `web/src/api/hooks/audit.ts` **(new)** | `useAuditRows(history)`, `useAuditVerification(scope, enabled)` (6.6) |

### 4.4 Web: the Audit page (`web/src/features/audit/`, all **(new)**)

`AuditPage.tsx` (header, check section, tabs), `VerificationCards.tsx` (the two status cards, the limits note, the check
button), `ChainCard.tsx`, `LegacyCard.tsx`, `AnchorDetails.tsx` (P2 key-value list), `AuditTable.tsx` (one history's list,
load more), `auditColumns.tsx`, `EntryDialog.tsx` (one row's details), `audit.module.css` (placement only), and the tests of
section 12. Wiring: `web/src/app/pages.ts` (`audit` lazy page), `routes.tsx` (`/audit`), `nav.ts` (item
`{ to: "/audit", label: "Audit log", icon: "eye", preload: PAGES.audit.preload }` in the System group before Settings; no
count; `eye` because `log` already means Logs and no shield icon exists, and a shield would read as a security promise), `web/src/design/visibility.ts` (`VISIBILITY_PAGES` gains `{ key: "audit", label:
"Audit log" }`), `web/scripts/screens.mjs` (page entries).

### 4.5 Web: retention in Settings

| File | Change |
|---|---|
| `web/src/features/settings/sections/retention.ts` **(new)** | the `retention` `SectionSpec` (nine `number` fields, `view: RetentionKept`); `RETENTION_LABELS` |
| `web/src/features/settings/sections/upkeep.ts` | `UPKEEP_SECTIONS` gains the retention section after `backups`; `backups` gains the field `backup.keep_pre_migrate` |
| `web/src/features/settings/views/RetentionKept.tsx` **(new)** | the shared read-only blocks: what is kept with no period, what deletion guarantees, the backups pointer (6.8) |
| `web/src/features/settings/views/RetentionRows.tsx` **(new)** | the read-only value rows for a file-managed hub, from `settings.retention` |
| `web/src/features/settings/ReadOnlySettings.tsx` | one `<Block title="Retention">` (stay under 250 lines) |
| `web/src/features/settings/form/sensitive.ts`, `EditorForm.tsx` | `isSensitive` covers `retention`; `sensitiveReason(changes)` supplies the dialog text (6.8) |
| `web/src/test/settingsFixture.ts`, `web/src/test/fixtures.ts` | the new `retention` field in the settings answer; helper `auditRow()` and `verification()` factories |
| `web/src/dev/AuditDemos.tsx` **(new)**, `web/src/dev/DesignPage.tsx`, `web/src/dev/auditDemos.test.tsx` **(new)** | `/dev/design` demos of the two status vocabularies' cards in every state |

### 4.6 Smoke test, docs

| File | Change |
|---|---|
| `web/scripts/smoke/server.mjs`, `data.mjs`, `checks.mjs` (prompt 30) | the mock answers `GET /api/v1/audit` and `GET /api/v1/audit/verification`; `/settings` gains `retention`; two new checks (12.4) |
| `web/DESIGN.md` | section 15 |
| `docs/book/src/guide/audit.md` **(new)**, `SUMMARY.md`, `reference/api.md`, `reference/data-model.md`, `guide/retention.md`, `ARCHITECTURE.md` | section 15 |

## 5. Explicit non-goals

| Not in this prompt | Where it belongs |
|---|---|
| Any audit write path: appending from the UI, the checkpoint writer, pruning, `audit.restored`, repairing a broken chain | later (pruning is not designed yet); the hub only reads here |
| Changing the chain, the anchor, the hash encoding, `Store::verify_audit` or `insert_audit` | merged and frozen by the audit chain work; this prompt adds one port beside them |
| A filter or search UI on the audit page, a date range, export to a file | later; the route accepts `history` and `kind` only (the CLI and automation can use `kind`) |
| The head hash in the daily digest, off-host anchoring, signed heads | deferred (06 "Deferred and after-v1"); the page only shows and copies the head |
| Making the three declared retention keys apply (`operation_journal_days`, `execution_outputs_days`, `retracted_facts_days`) | their owners (operations, outputs, knowledge); this page reports what the registry says |
| Pruning, deleting or shortening anything; a "purge now" button; the purge schedule | not needed; the purge runs daily at 03:30 as 57 built it |
| Fixing `metrics` and `llm.*.cli` in the web `isSensitive` | reported; a separate settings fix |
| Per-profile classification of other Settings sections; a `VisibilityPage` for the retention section | prompt 37 left Settings alone; this prompt classifies only its own new elements |
| A new hub top-level module, a migration, a config key, a wire (protocol) change, a new dependency | none needed; stop and report if one seems needed |
| Restoring from backup, `domovoi restore`, `audit verify` flags | the CLI work of earlier prompts |
| Showing the audit log to anyone but the signed-in owner; roles | D-13 (one owner); `Authed` is the only gate |
| Touching `crates/domovoi/src/cmd/audit.rs` or its output | the CLI is unchanged; the page copies its words, not its code |

## 6. Required interfaces and contracts

Marks: **fixed** (must match exactly; later prompts or tests call it) or **convention** (follow repository style). Every public
Rust item gets a doc comment saying what it is for; every kit or web helper gets a JSDoc. Never write a prompt number, "the
build plan" or an RW or D id into code, comments, the book, commit messages or the PR description.

### 6.1 The read port **fixed** (names, signatures, rules); file layout **convention**

```rust
// crates/domovoi-core/src/ports/audit_inspect.rs   (new; no I/O; imports crate::audit_chain and super::{AuditFilter, Page, Paged, StoreError})
use async_trait::async_trait;
use crate::audit_chain::{AuditAnchor, ChainRow, Hash, VerifyReport, VerifyScope};
use super::{AuditFilter, Page, Paged, StoreError};

/// Which part of the audit history a listing covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditHistory {
    /// Rows written before the hash chain existed: `seq <= audit_anchor.legacy_last_id`.
    Legacy,
    /// Rows written since: `seq > audit_anchor.legacy_last_id`.
    Chained,
}

/// Which audit rows a listing returns: the event filters of `AuditLog::list`, optionally one part of the history.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuditRowFilter { pub events: AuditFilter, pub history: Option<AuditHistory> }

/// Read-only access to the audit log with its hash-chain fields, for the audit page. Nothing here writes.
#[async_trait]
pub trait AuditInspect: Send + Sync {
    /// One page of rows matching `filter`, highest sequence number first, with `prev_hash` and `hash` (both `None` on a legacy row).
    async fn rows(&self, filter: &AuditRowFilter, page: &Page) -> Result<Paged<ChainRow>, StoreError>;
    /// The anchor over the legacy history; `None` only if the store was not opened through `Store::open`.
    async fn anchor(&self) -> Result<Option<AuditAnchor>, StoreError>;
    /// The newest chained row's sequence number and hash, `None` before the first chained row.
    async fn head(&self) -> Result<Option<(u64, Hash)>, StoreError>;
    /// Verifies the history as `Store::verify_audit` does; an unreadable or edited anchor is an error, never a pass.
    async fn verify(&self, scope: VerifyScope) -> Result<VerifyReport, StoreError>;
}
```

**Store (`impl AuditInspect for Store`)**: `anchor`, `head` and `verify` call `Store::audit_anchor`, `Store::audit_head` and
`Store::verify_audit` unchanged. `rows` copies `repos/audit.rs`'s `list` (seq cursor through `Cursor::bind`, `fetch_limit`,
`paginate`, reads on `self.read`) with this statement; the extra bind `?6` is `0` (no history filter), `1` (legacy) or `2`
(chained); `into_chain_row` (made `pub(crate)`) builds each `ChainRow`:

```sql
SELECT seq, at, actor, kind, subject, detail, prev_hash, hash FROM audit_log
WHERE (?1 IS NULL OR actor = ?1) AND (?2 IS NULL OR kind = ?2) AND (?3 IS NULL OR subject = ?3)
  AND (?4 IS NULL OR at >= ?4) AND (?5 IS NULL OR seq < ?5)
  AND (?6 = 0
       OR (?6 = 1 AND seq <= (SELECT legacy_last_id FROM audit_anchor WHERE id = 1))
       OR (?6 = 2 AND seq >  (SELECT legacy_last_id FROM audit_anchor WHERE id = 1)))
ORDER BY seq DESC LIMIT ?7
```
A missing anchor row makes the two history filters match nothing (fail closed); `Store::open` always writes the anchor.

**In-memory twin (`impl AuditInspect for InMemoryStore`)**: `rows` lists its audit events (sequence = position from 1, the
existing `matches` filter made `pub(super)`) with both hashes `None`; `AuditHistory::Legacy` returns nothing and
`Chained` everything (it has no legacy part); `anchor` and `head` return `None`; `verify` returns
`Err(StoreError::Backend("the in-memory store keeps no audit chain".to_owned()))`. Its rustdoc says it does not chain.

### 6.2 `GET /api/v1/audit` **fixed** (path, query, statuses, JSON); DTO layout **convention**

Operation id `list_audit`, tag `audit`, behind `Authed`. Query (`AuditQuery`, `IntoParams`, `parameter_in = Query`):

| Parameter | Rule |
|---|---|
| `history` | `legacy` or `chained`; anything else is 400 `unknown history `<text>`; use legacy or chained` (text cut to 40 characters) |
| `kind` | exact audit kind, at most 100 characters (400 `kind is too long` beyond) |
| `cursor`, `limit` | the existing `page(cursor, limit)` helper: default 50, clamp 1..=200 |

Response 200 `AuditPageDto`:

```json
{ "legacy_last_id": 5,
  "next_cursor": "…",
  "items": [
    { "seq": 12, "at": "2026-11-18T10:00:00Z", "actor": "user:0190…", "kind": "action.approved", "subject": "action:0190…",
      "detail": "{\"plan_hash\":\"ab…\"}", "detail_truncated": false, "history": "chained",
      "hash": "9f2c…64 lower-case hex characters…" },
    { "seq": 3, "at": "2026-01-02T08:00:00Z", "actor": "system:actions", "kind": "action.proposed", "subject": "action:0190…",
      "detail": "", "detail_truncated": false, "history": "legacy", "hash": null } ] }
```

Rules: `legacy_last_id` is the anchor's (0 when `anchor()` is `None`); `history` of a row is `"legacy"` when `seq <=
legacy_last_id`, else `"chained"` (never decided by the presence of `hash`); `hash` is `hex(row.hash)` or `null`; `at` is
the timestamp's RFC 3339 `Display`; `detail` is `""` for a `Null` value, else the compact JSON text (`serde_json::to_string`)
passed through `state.deps.redactor.redact(..)` **first** and then cut to at most `MAX_DETAIL_BYTES = 4096` bytes on a
character boundary (write the small loop by hand; do not rely on an unstable `str` method), with `detail_truncated` true when
it was cut. `prev_hash` is not returned. Errors: 401 without a session; 400 as above; a store failure is 500 through
`ApiError::internal("reading the audit log", err)` (the text is logged, never returned).

```rust
// crates/domovoi-hub/src/api/audit.rs   (new; convention: copy the doc style of api/logs.rs)
pub(super) async fn list(State(state): State<ApiState>, _auth: Authed, Query(query): Query<AuditQuery>)
    -> Result<Json<AuditPageDto>, ApiError>;
// 1. validate history and kind; 2. anchor = state.deps.audit_inspect.anchor().await?  (map_err to ApiError::internal)
// 3. rows = state.deps.audit_inspect.rows(&AuditRowFilter { events: AuditFilter { kind, ..Default::default() }, history }, &page(..)).await?
// 4. map every ChainRow with row_dto(&row, legacy_last_id, &*state.deps.redactor)   (pure, unit-tested in dto_audit.rs)
```

### 6.3 `GET /api/v1/audit/verification` **fixed**

Operation id `verify_audit`, tag `audit`, behind `Authed`, query `VerificationQuery { scope: Option<String> }`: absent or
`recent` runs `VerifyScope::Last(RECENT_ROWS)` where `pub const RECENT_ROWS: u64 = 10_000;` is declared in `api/audit.rs`
with the doc "the same bound the hub uses when it starts"; `full` runs `VerifyScope::Full`; anything else is 400
`unknown scope `<text>`; use recent or full`. It reads, appends nothing, and runs one verification per request (no cache, no
background run). Response 200 `AuditVerificationDto`:

```json
{ "scope": "recent", "recent_limit": 10000, "checked_at": "2026-11-18T10:00:00Z",
  "legacy":  { "status": "count_only", "count": 5, "expected_count": null, "actual_count": null, "digest_matches": null },
  "chained": { "status": "intact", "checked": 812, "head_seq": 817, "head_hash": "9f2c…",
               "first_broken_seq": null, "break_reason": null, "break_text": null },
  "anchor":  { "install_id": "…", "legacy_last_id": 5, "legacy_count": 5, "legacy_digest": "…64 hex…", "anchor_hash": "…64 hex…" },
  "head":    { "seq": 817, "hash": "9f2c…" } }
```

Mapping from `VerifyReport` (**exhaustive `match`es**, so a new core variant is a compile error, not a silent default):

| Core value | JSON |
|---|---|
| `LegacyStatus::Verified { count }` | `status "verified"`, `count` |
| `LegacyStatus::CountOnly { count }` | `status "count_only"`, `count` |
| `LegacyStatus::Mismatch { expected_count, actual_count, digest_matches }` | `status "mismatch"`, those three fields |
| `LegacyStatus::NotVerifiable` | `status "not_verifiable"` |
| `ChainStatus::Empty` | `status "empty"` |
| `ChainStatus::Intact { checked, head_seq, head_hash }` | `status "intact"`, `checked`, `head_seq`, `head_hash` (hex) |
| `ChainStatus::Broken { checked, first_broken_seq, reason }` | `status "broken"`, `checked`, `first_broken_seq`, `break_reason` = code, `break_text` = `reason.to_string()` |
| `BreakReason` codes | `MissingChainFields` `missing_chain_fields`; `HashMismatch` `hash_mismatch`; `LinkMismatch` `link_mismatch`; `SequenceGap` `sequence_gap`; `CheckpointMismatch` `checkpoint_mismatch` |

Fields not used by a status are `null` (serialised, not omitted). `anchor` is the report's anchor with `anchor_hash =
hex(anchor.hash())`; `head` is `audit_inspect.head()` read **after** the verification (`null` before the first chained row; it
is the newest stored row, also when the chain is broken); `checked_at` is `state.deps.clock.now()` taken after the
verification. A verification error (including the edited-anchor error) is 500 through `ApiError::internal("verifying the audit
log", err)`; the response never contains a status for it.

### 6.4 `SettingsDto.retention` **fixed** (names, JSON), mapper **convention**

```rust
// crates/domovoi-hub/src/api/settings.rs   (SettingsDto gains `pub retention: RetentionSettings`)
/// The retention periods in force and how many backups are kept.
pub struct RetentionSettings { pub entries: Vec<RetentionEntry>, pub backups_keep: u32, pub backups_keep_pre_migrate: u32 }
/// One `[retention]` period.
pub struct RetentionEntry {
    pub key: String,              // "signals_days"
    pub days: u32,                // the value in force
    pub default_days: u32,        // from RETENTION_KEYS
    pub applies: String,          // "now" | "later"
    pub applies_once: Option<String>,   // Some("operations exist") when applies is "later"
}
```
Built in `show` by walking `domovoi_config::hub::RETENTION_KEYS` in order: `days` is looked up **by key name** in
`config.retention.days()` (now `pub`) with `map_or(key.default_days, |(_, days)| *days)` (no `unwrap`); `Applies::Now` gives
`"now"` and `None`; `Applies::Once(reason)` gives `"later"` and `Some(reason.to_owned())`. `backups_keep = config.backup.keep`,
`backups_keep_pre_migrate = config.backup.keep_pre_migrate`. Nine entries, in registry order, always. Example entry:
`{"key":"operation_journal_days","days":180,"default_days":180,"applies":"later","applies_once":"operations exist"}`.

### 6.5 Web vocabularies and words **fixed** (words), wiring **convention**

`web/src/design/auditVocab.ts` (new) holds the two tables and their lookups. The lookups live **here**, not in `tone.ts`,
because `tone.ts` is near the 250-line cap (208 before prompt 30, more after it); `tone.ts` only imports them, re-exports
`chainStatusMeta` and `legacyStatusMeta` (so pages still import from `design/tone`) and lists both tables in `VOCABULARIES`
(the existing `tone.test.ts` then checks label, tone, hint and icon). Check `wc -l web/src/design/tone.ts` before and after.

| `LEGACY_STATUSES` key | label | tone | icon | hint |
|---|---|---|---|---|
| `verified` | Verified | success | check-circle | The entries from before the chain still match the digest recorded when the chain began. |
| `count_only` | Count matches | info | info | Only the number of older entries was checked. Check the whole log to compare their digest. |
| `mismatch` | Changed | danger | alert | The older entries no longer match what was recorded. The check cannot say which entry changed. |
| `not_verifiable` | Not verifiable | warning | alert | Older entries were pruned, so their digest can no longer be recomputed. |

| `CHAIN_STATUSES` key | label | tone | icon | hint |
|---|---|---|---|---|
| `empty` | No chained entries yet | neutral | clock | Nothing has been recorded since the chain began. New entries are chained as they are written. |
| `intact` | Intact | success | check-circle | Every checked entry links to the one before it and its hash is correct. |
| `broken` | Broken | danger | x-circle | A checked entry does not match the chain. Entries may have been edited, removed or added outside Domovoi. |

Both are `Table<…>` with the string-union types `LegacyStatus` and `ChainStatus` in `design/status.ts`; an unknown value (a
newer hub) is the neutral pill with the raw string (`lookup`).

`web/src/design/auditText.ts` (new, pure, unit-tested):

```ts
export function breakReasonText(code: string, fallback: string | null): string;
//  missing_chain_fields "An entry has no hash chain fields."  hash_mismatch "An entry was edited."
//  link_mismatch "An entry was removed or inserted."  sequence_gap "Entries are missing."
//  checkpoint_mismatch "The retained start does not match the checkpoint."   unknown: fallback ?? code
export function auditKindHint(kind: string): string | undefined;
//  "audit.chain_broken" "The hub found the chain broken when it started."
//  "audit.checkpoint"   "Older entries were removed here. The check starts from this entry."   others: undefined
export function scopeSentence(scope: string, recentLimit: number): string;
//  recent "Checked the newest {n} chained entries and counted the older ones."   (n with thousands separators)
//  full   "Checked every entry and compared the older ones with their digest."   other: "Checked."
```
The audit page says "entry" and "entries" in the UI; the rows are audit rows in code and the API. Sentence case, no
exclamation marks, no "tamper-proof" or "secure" anywhere (`voice.test.ts` must still pass).

### 6.6 Hooks **fixed** (names, request shapes), layout **convention**

```ts
// web/src/api/hooks/audit.ts   (at most 250 lines)
export const AUDIT_PAGE = 50;
export type AuditHistoryName = "chained" | "legacy";
useAuditRows(history: AuditHistoryName)                                 // useInfiniteQuery, key keys.auditRows(history);
                                                                        // GET /api/v1/audit with defined({ history, cursor, limit: AUDIT_PAGE }); next page = next_cursor
useAuditVerification(scope: "recent" | "full", enabled = true)          // useQuery, key keys.auditVerification(scope);
                                                                        // GET /api/v1/audit/verification?scope=<scope>; refetchOnWindowFocus false; staleTime Infinity for "full"
```
`keys.audit = ["audit"]`, `keys.auditRows(h) = ["audit","rows",h]`, `keys.auditVerification(s) = ["audit","verification",s]`. No
mutation exists, so `useActionFeedback` is not needed; the "Check the whole log" button runs the `full` query and reports
through `Progress`, the result cards and a `LoadError` (6.7). Nothing here is invalidated by the live stream (the audit
log has no topic); the page offers a Refresh button.

### 6.7 The Audit page **fixed** (behaviour), **convention** (markup)

Route `/audit` (`PAGES.audit`, lazy). `PageHeader title="Audit log" intro="What Domovoi recorded, and whether the record still
holds." docs="guide/audit.html"`; `AllHostsNote` ("The audit log belongs to the hub, not to one host."); then
`<VisibilityPage page="audit">` around the rest. Top to bottom:

1. `Section title="History check"` holding `<Visible category="P0" id="audit.verification">` around a `QueryBoundary`
   (`useAuditVerification("recent")`, `context="check the audit log"`, `loading` = a block `Skeleton`; **no empty state**: the
   response always has two parts). The loaded content, in order:
   - a `Grid min="sm"` of two `Card`s, **chained first**:
     - **Chained history** (`h3`): `StatusPill chainStatusMeta(status)`. `intact`: "{checked} entries checked, up to #{head_seq}."
       `empty`: "No entries since the chain began." `broken`: an `InlineMessage tone="danger" title="Broken at entry
       #{first_broken_seq}"` with `breakReasonText(break_reason, break_text)` and "{checked} entries were accepted before it.".
       Then **Head hash**: when `head` is present a `CopyBlock label="Head hash"` with `head.hash` and the muted line "Entry
       #{head.seq}. Keep a copy somewhere off this host: it is the only way to notice that the newest entries were removed.";
       when `head` is `null` the muted line "There is no head hash until the first chained entry."
     - **Legacy history** (`h3`): `StatusPill legacyStatusMeta(status)`. `verified`: "{count} entries from before the chain. Their
       digest matches."; `count_only`: "{count} entries from before the chain. Their digest was not compared in this check.";
       `mismatch`: "Expected {expected_count} entries and found {actual_count}. The digest {matches|differs}." (from `digest_matches`);
       `not_verifiable`: "Older entries were pruned, so they cannot be checked." Whatever the status, when `anchor.legacy_count`
       is 0 the line is "This install has no entries from before the chain." and the legacy status pill is still shown.
   - a muted line "Checked {RelativeTime checked_at}. {scopeSentence(scope, recent_limit)}" and
     `Button variant="secondary" icon="refresh"` **"Check the whole log"** (label "Check again" once a full result exists) with
     `hint="Reads every entry and compares the older ones with their digest. It can take a while on a large log."`. Clicking
     sets the page state `full = true` (enables `useAuditVerification("full")`); while it is fetching the button is `loading`
     and a `Progress label="Checking every entry…"` stands under the cards; when it succeeds the cards show the **full**
     result (`scope "full"`); when it fails a `LoadError error context="check the whole log" onRetry={full.refetch}` stands above
     the cards and the recent result stays visible. "Check again" calls `full.refetch()`.
2. `<Visible category="P0" id="audit.limits">`: `InlineMessage tone="info" title="What this check can show"` with the body "A
   broken chain shows that entries were edited, removed or added outside Domovoi. It cannot show that someone rewrote the whole
   database and recomputed every hash, or that the newest entries were removed, unless you kept the head hash elsewhere."
3. `<Visible category="P2" id="audit.anchor" label="Anchor details" count={5}>`: a `KeyValueList mono` (Install ID; Entries before
   the chain; Last of them, "#N" or "–" when 0; Digest; Anchor hash). Hashes show the first 12 characters, the whole value in a
   `Tooltip`, with a `CopyButton` for the whole value.
4. `Section title="Entries"` with `actions` = `<Visible category="P1" id="audit.refresh">` around a `Button variant="ghost"
   icon="refresh"` "Refresh" (refetches the selected tab's rows), then `<Visible category="P0" id="audit.entries">` around a
   controlled `Tabs label="Audit history"` with tabs `chained` ("Chained history") and `legacy` ("Legacy history"); the choice
   is `?tab=` (`useSearchParams`, `replace`; chained = no parameter; an unknown value reads as chained).
   `AuditTable({ history, firstBroken })`: `QueryBoundary` over `useAuditRows(history)` (`context="load audit entries"`,
   `Skeleton shape="rows" count={5}`), empty states `EmptyState icon="info" title="No chained entries yet"` ("New entries appear
   here as Domovoi records them.") and `title="No entries from before the chain"` ("This install started with the chain.")
   (never for a failed load), then a `DataTable` (`caption` "Chained audit entries" or "Legacy audit entries", stable `useMemo`
   columns): **No.** (numeric, `#{seq}`; for `seq === firstBroken` a `Tag tone="danger"` "First broken entry" beside it and
   `rowState` `"urgent"`), **When** (`RelativeTime`), **Who** (`actor`, mono, ellipsis, tip = the full value), **What** (`kind`,
   mono, `tip` = `auditKindHint(kind)` or the kind), **About** (`subject`, mono, ellipsis, tip), and an actions column
   (`hideLabel`) with `IconButton icon="chevron-right" label="Open entry {seq}"` that opens `EntryDialog`. Below the table, when
   `hasNextPage`, `Button` "Load more" (`loading` while fetching); a failed next page is an `InlineMessage` with Retry and the
   rows already loaded stay.
5. `EntryDialog` (a kit `Dialog`, title "Audit entry #{seq}", a "Close" button): `KeyValueList mono` with Number, Time (the RFC 3339
   `at` as sent), Who, What, About, History (`Tag` "Chained" or "Legacy"), Hash (chained: the whole hash with a `CopyButton`;
   legacy: "None. This entry was written before the chain began."), then Detail: `CopyBlock label="Entry detail"` with the text,
   or "No detail was recorded." when `detail` is empty; muted lines "Secrets are removed before it is shown." and, when
   `detail_truncated`, "Shortened to 4 KiB.". Dialogs carry no visibility category.

Keyboard: tabs follow the kit's arrow-key behaviour; every row's open button is reachable; Escape closes the dialog (kit).
All text is rendered as React text (never `dangerouslySetInnerHTML`): `actor`, `subject` and `detail` come from rows that
may contain text a remote party influenced.

**Visibility categories of the page** (37's rule, the four questions in order; the executor does not change them):

| id | element | category | mark |
|---|---|---|---|
| (none) | `PageHeader` title and intro, the section headings: strings given to the kit | P1 by definition | none |
| `audit.verification` | both status cards, head hash, the check line and button, the check's error and progress, the load error | P0 (what was recorded; failure and state) | V |
| `audit.limits` | "What this check can show" | P0 (what authority the evidence has) | V |
| `audit.anchor` | anchor details | P2, label "Anchor details", `count={5}` | V |
| `audit.refresh` | the Refresh button | P1 | V |
| `audit.entries` | the tabs and both tables with their loading, empty and error states | P0 (what was recorded) | V |

In Standard and Essential the anchor details are one collapsed toggle "Anchor details" with the count 5; everything else is
the same in every profile. `p0_visible_in_every_profile` for this page expects the ids `audit.verification`, `audit.limits`
and `audit.entries`.

### 6.8 Retention section **fixed** (behaviour), **convention** (markup)

`web/src/features/settings/sections/retention.ts` exports `RETENTION_LABELS` (key to label) and the `SectionSpec`:

```ts
export const RETENTION_LABELS: Readonly<Record<string, string>> = {
  signals_days: "Signals", incident_timeline_days: "Incident timeline", conversations_days: "Follow-up conversations",
  notifications_days: "Notification state", triage_cache_days: "Cached triage reports",
  resolved_advisories_days: "Resolved advisories", operation_journal_days: "Operation journal",
  execution_outputs_days: "Raw action output", retracted_facts_days: "Retracted knowledge",
};                                                      // the order of RETENTION_KEYS; an unknown key shows its raw name
export const RETENTION_SECTION: SectionSpec = {
  id: "retention", title: "Retention",
  intro: "How long raw data is kept, from 1 to 3650 days. Operations, their summaries and the audit log are kept and have no period.",
  fields: fixed(/* one FieldSpec per entry: { path: `retention.${key}`, label: `${label} (days)`, kind: "number" } */),
  view: RetentionKept,
};
```
Added to `UPKEEP_SECTIONS` right after `backups`. The `backups` section gains `{ path: "backup.keep_pre_migrate", label: "Pre-upgrade
backups to keep", kind: "number" }` after `backup.keep`. The field help text comes from the schema's rustdoc (do not retype it);
the three declared keys therefore read "Not applied yet: applies once ... exist" from the hub's own text.

`RetentionKept` (used as the section's `view`, so it draws above the fields, and inside the file-managed block): reads
`useSettings()` inside `QueryBoundary` (`context="load retention settings"`, `Skeleton`), and renders, in order:

1. `<Visible category="P0" id="retention.kept">`: `Text strong` "Kept, with no period" and a plain list: "Operations and their summaries", "The
   audit log, checked on the Audit log page" (a `Link` to `/audit`), "Incidents, digest facts and notes".
2. `<Visible category="P0" id="retention.guarantee">`: `InlineMessage tone="info" title="What deletion does"` with the body "When a
   period ends the data is removed from the live database, its search index and what the model sees. It stays in backups until
   they rotate out ({backups_keep} nightly and {backups_keep_pre_migrate} before upgrades, set under Backups), and restoring a
   backup brings it back until the next daily clean-up." (numbers from `settings.retention`).
3. `<Visible category="P1" id="retention.stepup">`: muted `Text` "Changing a period asks for your password again, because a shorter
   period deletes data sooner."

`RetentionRows({ retention })` (file-managed hub only; `SettingsCard` and `ValueRow` from `SettingRow.tsx`): one row per entry,
label `"{label} (days)"`, value `"{days} days"` (`"1 day"` for 1), and for `applies === "later"` a `Tag` "Not applied yet" with the
muted text "Applies once {applies_once}."; then rows "Nightly backups kept" (`backups_keep`) and "Pre-upgrade backups kept"
(`backups_keep_pre_migrate`). `ReadOnlySettings` renders `<Block title="Retention"><RetentionKept /><RetentionRows .../></Block>`
(place it after "Backups" if that block exists, else before "Advisories"; after 37's "Display" block).

**Password re-entry.** `web/src/features/settings/form/sensitive.ts`:

```ts
/** Whether a changed path is under [retention]. */
export function isRetentionPath(path: string): boolean;     // path === "retention" || path.startsWith("retention.")
export function isSensitive(changes: Changes): boolean;     // existing rules OR any changed path isRetentionPath
export function sensitiveReason(changes: Changes): string;  // only retention paths changed: "A shorter period deletes data sooner. Enter your password to save it."
                                                            // any other sensitive change (also mixed): "This change affects who can act or what is trusted. Enter your password to save it."
```
`EditorForm.tsx` passes `reason={sensitiveReason(form.changes())}` in place of the literal (`:224`); nothing else in the file
changes. The hub already enforces the rule; the web only asks first. A wrong password keeps the existing "That password is not right."
flow.

### 6.9 Smoke test additions **fixed** (what they check), **convention** (layout)

Prompt 30's mock hub fails the run on any unlisted `/api` request. Add: `GET /api/v1/audit` (a page of five chained rows and two
legacy rows by `history`; `legacy_last_id` 2; one row with `detail` text that contains an angle bracket), `GET
/api/v1/audit/verification` (`recent`: legacy `count_only`, chained `intact`; `scope=full`: legacy `verified`), and the `retention`
field in the `/settings` payload (nine entries, three `later`). Two new checks after prompt 30's and 37's: **12. Audit page**
(sign in, open `/audit`: heading "Audit log"; both status pills visible with their words; the head hash block has the text;
choosing the Legacy tab lists the legacy rows; "Check the whole log" turns the legacy pill to "Verified"; the entry dialog opens
with the hash; at 375 px no horizontal scroll; axe in the real browser, including colour contrast, in both colour schemes and in
each profile through the mock's preference) and **13. Retention** (open `/settings/retention`: the nine fields, the "What deletion
does" text; changing a period and pressing Save opens the password dialog **before** any `PATCH`; with `editable: null` the
read-only block lists the nine values and "Not applied yet" three times). No `console.error`, no unexpected request.


### 6.8a Backup counts need the password (owner decision D-34) **fixed**

Add two rules to `STEP_UP_SETTINGS` in `crates/domovoi-hub/src/settings/step_up.rs`, after the `retention` rule
and in the existing style:

```rust
StepUpRule {
    path: PathRule::Exact("backup.keep"),
    why: "lowering the backup count can remove recovery points",
},
StepUpRule {
    path: PathRule::Exact("backup.keep_pre_migrate"),
    why: "lowering the pre-upgrade backup count can remove recovery points",
},
```

The web's `form/sensitive.ts` marks the same two paths sensitive, so the settings editor asks for the password
before sending, with the same dialog wording as retention. Tests: `backup_counts_require_step_up` (hub: a settings
update changing either key without a password is `403`, with the right password succeeds and is audited) and
`backup_count_fields_ask_for_the_password` (web: editing either field and saving opens `PasswordPrompt`). Update the
existing step-up unit test that lists the rules, if it pins the count.

## 7. Ordered implementation steps

Work on branch `audit-and-retention-pages`, cut from the latest `overhaul-investigation`. Commit in small green steps (one-line
Conventional Commits, exactly one scope each: `feat(core)`, `feat(store)`, `feat(hub)`, `feat(web)`, `test(web)`, `docs`), after
steps 3, 5, 7, 9, 11 and 12. Run the scoped checks of each area after every commit.

1. **Inspect and verify the repository context; report discrepancies before editing.**
   - Confirm 37 and 30 are merged: `web/src/design/visibility.ts`, kit `Visible` and `DetailsScope`, `web/src/test/visibility.tsx`,
     `web/scripts/smoke.mjs` and `scripts/smoke/server.mjs`, `web/src/app/rightSlot/`. **If any is missing, stop and report.** Write
     down every real name that differs from section 3 (the `Visible` props, `VISIBILITY_PAGES`, the smoke mock's route table).
   - Open every file of section 3 and confirm the line numbers. Read `crates/domovoi-store/src/audit_chain.rs`,
     `audit_verify.rs`, `repos/audit.rs` completely; `crates/domovoi/src/cmd/audit.rs` `render_report` (the words to match);
     `api/logs.rs`, `api/overview.rs`, `api/settings.rs`; `settings/step_up.rs`; `RetentionSection` and `RETENTION_KEYS`.
   - Confirm that **no audit route exists** (`grep -n "audit" crates/domovoi-hub/src/api/*.rs`) and that `ApiDeps` still has
     `audit: Arc<dyn AuditLog>`; list the three `ApiDeps` construction sites (`grep -rn "ApiDeps {" crates`).
   - Read `web/DESIGN.md` completely (rules and the "Planned" section as prompts 30 and 37 left it), `web/src/components/index.ts`,
     `Tabs.tsx`, `DataTable.tsx`, `Dialog.tsx`, `CopyBlock.tsx`, `RelativeTime.tsx`, `features/settings/{SettingsPage,
     ReadOnlySettings,EditorForm,SectionBody}.tsx`, `sections/{types,upkeep}.ts`, `form/sensitive.ts`, `src/test/{apiMock,render,
     setup,settingsFixture,fixtures}.ts*`. Run `wc -l` on `design/tone.ts`, `ReadOnlySettings.tsx` and `EditorForm.tsx`.
   - Run once for the baseline: `cargo nextest run -p domovoi-store -p domovoi-hub` (or `cargo test`) and, in `web/`, `npm test`.
     Note the number of passing tests.
   - Check `docs/book/src/guide/retention.md` and `reference/data-model.md` exist, and that `docs/book/src/guide/audit.md` does not.
2. **Core port and testkit.** `ports/audit_inspect.rs`, the exports, the in-memory impl (6.1). `cargo nextest run -p domovoi-core
   -p domovoi-testkit`.
3. **Store.** `audit_inspect.rs`, the two `pub(crate)` changes, the store tests of 12.1. `cargo clippy -p domovoi-store
   --all-targets -- -D warnings`, `cargo nextest run -p domovoi-store`. Commit `feat(store): read audit rows with their chain fields`.
4. **Config visibility.** `RetentionSection::days` becomes `pub`; its existing tests still pass (`cargo nextest run -p domovoi-config`).
5. **Hub.** `dto_audit.rs` (mappers and their unit tests first), `audit.rs`, `api/mod.rs` (tag, routes, the `ApiDeps` field and its three
   constructions), the `SettingsDto.retention` field and its builder, the HTTP tests and the gate of 12.2. Then `cargo xtask openapi &&
   cd web && npm run gen:api`, `cargo insta review` (read the whole diff: two paths, the audit DTO schemas, `RetentionSettings` and
   `RetentionEntry`, one tag, one field on `SettingsDto`; nothing else) and accept. `cargo fmt --all`, `cargo clippy -p domovoi-hub
   --all-targets -- -D warnings`, `cargo nextest run -p domovoi-hub`, `cargo xtask arch-check`. Commit `feat(hub): audit entries and
   verification endpoints`.
6. **Web, vocabulary and data layer.** `auditVocab.ts`, `auditText.ts`, `status.ts`, the three lines in `tone.ts`, `types.ts`, keys, `hooks/audit.ts`
   with its tests (12.3). `npx vitest run src/design src/api`; `npm run lint && npm run typecheck`.
7. **Web, Audit page.** `AuditPage` and its parts, `pages.ts`, `routes.tsx`, `nav.ts`, `VISIBILITY_PAGES`, the demos; tests of 12.3. Commit
   `feat(web): audit page`.
8. **Web, retention section.** `sections/retention.ts`, `upkeep.ts` (the new section and the new backups field), `RetentionKept`, `RetentionRows`,
   `ReadOnlySettings`, `sensitive.ts`, `EditorForm.tsx`, `settingsFixture.ts`; tests of 12.3. Run `npx vitest run src/features/settings` first
   **unchanged** (they must pass before you edit a test), then add the new tests.
9. Commit `feat(web): retention settings with password re-entry`.
10. **Product tests.** `p0_visible_in_every_profile` cases for the page, the source-gate id list (add this prompt's P0 ids to
    `every_p0_id_of_the_table_appears_in_the_sources`; extend, never weaken), and `/dev/design`. Look at `/dev/design`, `/audit` and
    `/settings/retention` in both themes. Commit `test(web): audit page across detail levels`.
11. **Smoke test and screens.** `scripts/smoke/*` (6.9), `scripts/screens.mjs` entries `["audit", "/audit"]` and `["retention",
    "/settings/retention"]`; `npm run build` then `npm run smoke`. Commit `test(web): browser smoke test for the audit page and retention`.
12. **Docs.** Section 15; `cargo xtask docs-links`. Commit `docs: audit log page and retention settings`.
13. **Checks and completion report.**
    - Hub and libraries: `cargo fmt --all`; `cargo clippy` and `cargo nextest run` for `domovoi-core`, `domovoi-store`, `domovoi-testkit`,
      `domovoi-config`, `domovoi-hub`; `cargo nextest run -E 'rdeps(domovoi-core)'` (a core port was added); `cargo xtask arch-check`.
    - Web: `npm run format` then `npm run format:check`, `npm run lint`, `npm run typecheck`, `npm test`, `npm run build` (record the new
      `npm run size` numbers; if a limit is exceeded stop and report), `npm run smoke`, `npm run gen:api` then `git diff --exit-code -- src/api`.
    - By hand at 375, 768, 1280 and 1920 px, both themes, keyboard only, in all three profiles: `/audit` and `/settings/retention` (and
      `node scripts/screens.mjs` if the dev hub runs; say so if not).
    - Write the completion report in the standard structure.

## 8. Database and persistence

**No migration, no schema change, no new table or column.** The page reads `audit_log` and `audit_anchor` (created by earlier
migrations) through the read pool; the new `rows` statement is a `SELECT`. `ls crates/domovoi-store/migrations/` must be identical
before and after. Retention values live in the existing configuration layers (`settings` table, `hub.toml`, environment); nothing
new is stored. **Transactions:** none are opened; every statement is a single read. **N-1 upgrade test:** not applicable (no
schema change); the existing upgrade tests (`crates/domovoi-store/tests/sqlite/upgrade.rs`, `crates/domovoi-hub/tests/hub/
upgrade_flow.rs`) must still pass, and `legacy_rows_of_the_upgraded_fixture_are_listed_as_legacy` (12.1) reads the committed
N-1 fixture (five legacy rows) through the new port. **Backups and restore:** unchanged; a restored database lists and verifies as
it was at backup time.

## 9. Protocol and compatibility

No agent protocol change (wire stays 1.2, RW-42), no change to any existing response except one **additive** field,
`SettingsDto.retention`, and no change to any status code, audit kind, journal kind or SSE topic. New HTTP surface: operations
`list_audit` and `verify_audit` (tag `audit`) and the schemas `AuditPageDto`, `AuditRowDto`, `AuditVerificationDto`,
`LegacyHistoryDto`, `ChainedHistoryDto`, `AuditAnchorDto`, `HeadDto`, `RetentionSettings`, `RetentionEntry`. A client that never
calls them is unaffected.

Inside the pair web and hub (they ship together, but a dev server may point at an older hub): an older hub answers 404 on
`/api/v1/audit`, so the page shows a `LoadError` (never an empty log); a settings answer without `retention` (older hub) makes the
file-managed block show the fallback text "This hub did not report its retention settings." instead of throwing (the
web type marks the field required, so read it defensively with `settings.retention?.entries ?? []` and show the fallback when the
array is empty). A newer hub that sends an unknown legacy or chain status, an unknown break reason, an unknown `history` or a new
retention key renders as a neutral pill with the raw string, raw break text or the raw key name, never an exception. The audit
chain's own encoding is unchanged; no hash is recomputed in the browser.

## 10. Error handling and recovery

| Situation | Exact behaviour |
|---|---|
| `GET /audit` or `/audit/verification` without a session | 401 `unauthorized` (the `Authed` extractor); nothing is read |
| `history` is neither `legacy` nor `chained`; `scope` is neither `recent` nor `full`; `kind` over 100 characters | 400 `bad_request` with the text of 6.2 or 6.3; the store is not called |
| The store fails while listing or verifying | 500 `internal`, the cause logged, never returned; the page shows `LoadError` "load audit entries" or "check the audit log" with Retry; never an empty table, never a status pill |
| The stored anchor was edited (`verify` or `anchor` returns `StoreError::Backend`) | the same 500; the page says it could not check the log; the hub keeps running (09's policy); the owner runs `domovoi audit verify` on the host (the muted line under the `LoadError`: "If this keeps failing, run domovoi audit verify on the hub's host.") |
| The chain is broken | 200 with `chained.status "broken"`; the card shows "Broken at entry #N" with the reason, the table marks that entry when it is loaded, the head hash still shows; **nothing is repaired or hidden** |
| The legacy part changed (digest or count) | 200 with `legacy.status "mismatch"`; the card says it cannot say which entry changed |
| A recent check cannot compare the legacy digest | 200 `count_only`; the card says the digest was not compared and offers "Check the whole log" |
| The whole-log check fails or is slow | the button shows `loading` and a `Progress` line; on failure a `LoadError` "check the whole log" with Retry stands above the cards and the recent result stays; a second click while it runs does nothing (the button is busy) |
| A page of rows fails after the first | rows already loaded stay; an `InlineMessage` with Retry under the table |
| Zero chained or zero legacy rows | `EmptyState` with the sentence of 6.7; it is never used for a failed load |
| `detail` is large or contains a secret-looking value | cut at 4096 bytes after redaction, `detail_truncated` true, the dialog says "Shortened to 4 KiB." |
| `detail` or `actor` contains markup | shown as text; nothing is interpreted |
| An unknown status, break reason or history value arrives | neutral pill or raw text; never an exception |
| The settings answer has no `retention` field (older hub) | the file-managed block shows "This hub did not report its retention settings."; the editable section still works from the schema |
| A retention save without the password | the web asks first; if a request is sent anyway the hub answers 403 "enter your password to make this change" and the existing error display shows it |
| A retention value outside 1 to 3650 | the hub answers with the issue `retention.<key>` "must be 1 to 3650 days"; the form places it on the field (existing behaviour) |
| A retention key is locked by file or environment | the field is read-only with its reason (existing behaviour); the password flow is not reached |
| `backup.keep_pre_migrate` is set to 0 | the hub's issue "must be at least 1" is placed on the field (existing validation) |

## 11. Security requirements

- **Read-only, by construction.** `AuditInspect` has no write method; `AuditLog::append` stays the only way a row is added and
  `insert_audit` the only code that inserts. The two routes are `GET`; neither calls `append`, writes a journal row, opens a
  transaction or touches `audit_anchor`. The gate `audit_inspection_is_read_only_and_only_the_audit_routes_use_it` (12.2) scans
  the hub sources: `audit_inspect` is read only in `api/audit.rs`, `api/mod.rs` (the field) and the three constructions, never
  in chat, the tool catalogue (`ReadTool`/meta-tools), the engine or a service; `AuditLog::append` is not called from
  `api/audit.rs` or `api/dto_audit.rs`.
- **Who may read.** `Authed` only (the signed-in owner, D-13). The model, chat and webhooks never reach these routes; the LLM
  tool catalogue stays closed and gains nothing.
- **Fail closed on the evidence.** A verification that cannot run is an error, never a status. The web never turns a failed or
  missing answer into "Intact" or "Verified"; the only green words come from `intact` and `verified` in a successful answer.
  Unknown statuses are neutral.
- **Honesty.** The page and the book state the chain's limits (section 2). No "tamper-proof", "secure" or "guaranteed" anywhere;
  a `voice.test.ts`-style scan of the new strings is part of 12.3.
- **Secrets.** Audit details were stored already redacted; the hub redacts again before serving and bounds the size. The web
  shows `detail` as text only and never logs it, stores it, puts it in a URL or builds a link from it. Hashes are public by
  nature (they are what the owner keeps off-host).
- **Retention changes need the password.** The hub enforces it (`STEP_UP_SETTINGS`); the web asks before sending. The password goes
  only in the request body (`useApplySettings` as today); it is never stored. A retention change is audited by the settings
  service as it already is; this prompt adds no audit kind.
- **Display is not authority.** Visibility profiles change what is drawn on these pages, never what the hub returns or allows; P0
  (the verification block, the limits note, the entries and every state) is never hidden or collapsed in any profile.
- **No new network path.** No `fetch` outside `src/api`, no external origin, no inline script or style; the smoke test's server is
  local.

## 12. Testing requirements

Name every test after the behaviour. No test sleeps: hub tests use `FakeClock` from `domovoi-testkit`; web tests use `findBy…` and
fake timers. Web page and component tests use `renderAt` (or `renderWithProfile`, introduced by 37) and `apiMock`; **axe runs
automatically** over whatever a UI test leaves on screen and no test of this prompt calls `skipAxe()` except `auditDemos.test.tsx`
(a catalogue of fragments, with a comment saying so). Expectations are derived from the fixtures with a small helper written in the
test, never by calling the function under test.

### 12.1 Store and testkit (`crates/domovoi-store/tests/sqlite/audit_inspect.rs`, one test in the testkit)

Setup helpers: `Fresh::new()` (an empty store) and a local copy of `open_fixture` (the five-legacy-row fixture, never modified).

| Test | Setup, action | Expected |
|---|---|---|
| `rows_come_newest_first_with_their_chain_fields` | fresh store, append three events, `rows(default filter, Page::first(10))` | seq 3, 2, 1; every `hash` and `prev_hash` is `Some`; the oldest row's `prev_hash` equals `anchor().hash()`; each next row's `prev_hash` is the previous row's `hash` |
| `rows_page_with_a_cursor_until_the_last_page_has_none` | five appends, limit 2 | pages `[5,4]`, `[3,2]`, `[1]`; the last `next_cursor` is `None` |
| `history_filter_splits_legacy_from_chained` | the fixture (five legacy rows) plus two appends | `Legacy` gives exactly seq 1 to 5, all with `hash None`; `Chained` gives seq 6 and 7 with `hash Some`; no filter gives all seven |
| `kind_filter_narrows_the_rows_and_combines_with_history` | appends of two kinds on the fixture | the event filter and the history filter both apply |
| `a_chained_row_that_lost_its_hash_is_still_chained_and_breaks_the_chain` | fresh store, append two events, drop `audit_log_no_update`, set `hash = NULL` on seq 2, recreate the trigger | `rows(Chained)` returns seq 2 with `hash None`; `rows(Legacy)` does not; `verify(Full)` reports `ChainStatus::Broken` with `MissingChainFields` |
| `anchor_head_and_verify_agree_with_the_store_methods` | fresh store, two appends | `anchor()` equals `Store::audit_anchor`, `head()` equals `Store::audit_head`, `verify(Last(1))` equals `Store::verify_audit(Last(1))` |
| `an_edited_anchor_is_an_error_not_a_report` | drop `audit_anchor_no_update`, edit `install_id`, recreate it | `anchor()` and `verify(Full)` return `Err(StoreError::Backend(_))`; `rows` still lists rows (listing is not verification) |
| `reading_the_audit_rows_writes_nothing` | count `audit_log` and `audit_anchor` rows, call `rows`, `anchor`, `head`, `verify` on both scopes, count again | counts unchanged |
| `in_memory_audit_inspect_lists_events_without_a_chain` (testkit unit test) | `InMemoryStore`, append two events | `rows` returns both with `hash None`; `anchor` and `head` are `None`; `verify` is an error whose text says there is no chain; `Legacy` filter returns nothing |

### 12.2 Hub (`crates/domovoi-hub/tests/hub/api_audit.rs`, unit tests inside `api/dto_audit.rs`, one gate)

Harness: `api()`, `a.admin().await`, `a.get(uri, &login)`, `a.audit` and `a.store` (real SQLite). Seed rows with
`AuditLog::append` using a kind of its own (`test.audit.entry`) so rows the harness writes while creating the account do not matter.

| Test | Setup, action | Expected |
|---|---|---|
| `audit_routes_require_a_session` | `GET /api/v1/audit` and `/api/v1/audit/verification` without a login | both 401 |
| `audit_list_returns_chained_rows_newest_first_with_hashes` | append three events, `GET /api/v1/audit?kind=test.audit.entry` | three items, `seq` descending, `history "chained"`, `hash` 64 lower-case hex characters, `legacy_last_id 0`, `at` parses as RFC 3339, `next_cursor null` |
| `audit_list_pages_with_a_cursor_and_a_limit` | five appends, `limit=2` then the returned `cursor` | `[5th, 4th]`, `[3rd, 2nd]`, `[1st]` of the seeded rows; the last page has `next_cursor null` |
| `audit_list_filters_by_history` | fresh harness | `history=chained` lists the seeded rows; `history=legacy` answers `items: []` and 200 |
| `audit_list_rejects_an_unknown_history_and_a_long_kind` | `history=old`, `kind=` of 101 characters | 400 `bad_request`, texts of 6.2 |
| `audit_detail_is_redacted_cut_and_empty_for_null` | append an event whose detail contains `sk-` followed by 24 letters, another with a 6 KiB string, another with `Null` | the first shows `[REDACTED]` (the key is absent), the second has `detail_truncated true` and a `detail` of at most 4096 bytes that is valid UTF-8, the third has `detail ""` |
| `row_dto_marks_rows_up_to_the_anchor_as_legacy_whatever_their_hash` (unit, `dto_audit.rs`) | `ChainRow`s with seq 3 and 6, `legacy_last_id 5`; a seq 6 row with `hash None` | `"legacy"`, `"chained"`, `"chained"`; `hash` is `hex` or `null` |
| `verification_of_a_fresh_install_is_empty_and_count_only` | `GET /api/v1/audit/verification` | 200, `scope "recent"`, `recent_limit 10000`, `legacy.status "count_only"` with `count 0`, `chained.status "empty"` (or `intact` if the harness already wrote rows: assert it is one of the two and that `head` matches), `anchor.legacy_last_id 0`, `checked_at` equals the harness clock |
| `full_verification_checks_the_legacy_digest` | `?scope=full` | `legacy.status "verified"`, `chained.status "intact"` after appends, `head.seq` equals the newest appended `seq`, `chained.head_hash` equals `head.hash` |
| `a_tampered_chain_is_reported_as_broken_with_the_first_broken_entry` | append three events, drop `audit_log_no_update` via `a.store.pool_for_tests()`, edit the middle row's `detail`, recreate the trigger, `?scope=full` | 200, `chained.status "broken"`, `first_broken_seq` is the edited row's `seq`, `break_reason "hash_mismatch"`, `break_text` is the words of `BreakReason`; `legacy.status` unaffected |
| `verify_report_mapping_covers_every_core_variant` (unit, `dto_audit.rs`) | build a `VerifyReport` for every `LegacyStatus` (4), `ChainStatus` (3) and `BreakReason` (5) and map it | exactly the JSON of the table in 6.3 (statuses, codes, `break_text`, hex hashes, unused fields `null`) |
| `a_verification_error_is_a_500_with_no_status` | drop `audit_anchor_no_update`, edit `install_id`, recreate | 500 `internal` with the generic text; the body has no `legacy` or `chained` key (the cause is logged only) |
| `unknown_scope_is_a_400` | `?scope=everything` | 400 with the text of 6.3 |
| `reading_the_audit_log_appends_nothing` | count rows of `audit_log` and `journal`, call both routes with each scope, count again | unchanged |
| `settings_report_the_nine_retention_periods_and_which_apply` | `GET /api/v1/settings` | `retention.entries` has nine entries in registry order; `signals_days` is `days` 14 (the harness's env override), `incident_timeline_days` 45 (the file), the others default; `operation_journal_days`, `execution_outputs_days`, `retracted_facts_days` have `applies "later"` and a reason; `backups_keep 7` and `backups_keep_pre_migrate 3` unless the harness config differs (assert the values of `config()`) |
| `the_openapi_document_lists_the_audit_operations_and_the_retention_schema` | the generated document | `list_audit` and `verify_audit` under `/api/v1/audit` and `/api/v1/audit/verification`; `AuditRowDto.history` is a `string`; `SettingsDto` requires `retention`; the existing `openapi_snapshot` is regenerated and reviewed |

Gate `audit_inspection_is_read_only_and_only_the_audit_routes_use_it` in `crates/domovoi-hub/tests/gates.rs` (a source scan like the
existing ones; extend, never weaken): the text `audit_inspect` appears in no `.rs` file under `crates/domovoi-hub/src` except
`api/audit.rs`, `api/mod.rs` and `runtime/mod.rs` (the construction); `AuditLog::append` and `.append(` on the audit port do not appear in
`api/audit.rs` or `api/dto_audit.rs`; no file under `crates/domovoi-engine/src` or `crates/domovoi-hub/src/{services,auth}` mentions `AuditInspect`.
Include a self-test of the scan on an in-memory sample with a forbidden use.

### 12.3 Web

Vocabularies and text (`src/design/auditVocab.test.ts`, `auditText.test.ts`):

| Test | Expected |
|---|---|
| `audit_vocabularies_cover_every_value_the_hub_sends` | the four legacy statuses and three chain statuses each have label, tone, hint and icon (the existing `tone.test.ts` covers them through `VOCABULARIES`); `intact` and `verified` are the only `success`; `broken` and `mismatch` are `danger` |
| `unknown_audit_statuses_are_neutral_with_the_raw_value` | `legacyStatusMeta("from_the_future")` and `chainStatusMeta(...)` are neutral and carry the raw string |
| `break_reasons_have_plain_words_and_unknown_ones_fall_back` | the five codes give the sentences of 6.5; an unknown code gives the hub's `break_text`, else the raw code |
| `scope_sentences_say_what_was_checked` | `recent` with 10000 reads "…newest 10,000 chained entries…"; `full` and an unknown scope as 6.5 |
| `audit_words_never_overclaim` | no string of `auditVocab.ts`, `auditText.ts`, the page or the book text constants contains "tamper-proof", "secure", "guaranteed" or "!" |

Hooks (`src/api/hooks/audit.test.ts`): `audit_rows_hook_sends_history_cursor_and_limit` (query `history`, `limit 50`, then `cursor`);
`verification_hook_sends_the_scope_and_does_not_refetch_on_focus`.

Page (`src/features/audit/audit.test.tsx`, fixtures `auditRow()`/`verification()` in `src/test/fixtures.ts`):

| Test | Expected |
|---|---|
| `audit_page_shows_both_histories_separately_with_their_own_status` | h1 "Audit log"; headings "Chained history" and "Legacy history" each with a pill whose label is `chainStatusMeta(...)` and `legacyStatusMeta(...)` of the fixture; chained card first in document order |
| `a_recent_check_says_the_legacy_digest_was_not_compared_and_offers_the_full_check` | `count_only`: the sentence of 6.7 and the button "Check the whole log" |
| `the_whole_log_check_runs_only_on_request_and_updates_both_cards` | before the click no request has `scope=full`; click: `Progress` "Checking every entry…", then the legacy pill reads "Verified"; the button reads "Check again" |
| `a_failed_whole_log_check_keeps_the_recent_result_and_offers_retry` | `failure(500, …)` for `scope=full`: `LoadError` "check the whole log" with Retry above the cards; the recent pills still on screen |
| `a_failed_verification_is_a_load_error_never_a_status` | `failure(500, "internal", …)` on the recent call: "Could not check the audit log", Retry, **no** "Intact" or "Verified" text |
| `a_broken_chain_names_the_entry_and_the_reason_and_marks_the_row` | `broken` fixture: "Broken at entry #…" with the reason words; the chained table row with that `seq` has the "First broken entry" tag; the head hash block is still there |
| `a_changed_legacy_history_says_it_cannot_say_which_entry` | `mismatch`: "Expected 5 entries and found 4…" and the digest sentence |
| `a_fresh_install_says_there_are_no_legacy_entries` | `anchor.legacy_count 0`: "This install has no entries from before the chain."; the Legacy tab shows its `EmptyState` |
| `the_head_hash_is_copyable_and_comes_with_the_off_host_advice` | `CopyBlock` labelled "Head hash" holds the whole hash; the advice sentence is present |
| `the_limits_note_states_what_the_chain_cannot_show` | the `InlineMessage` text of 6.7 |
| `entries_list_rows_with_who_what_about_and_open_a_dialog_with_the_hash` | rows from the fixture; click "Open entry N": dialog "Audit entry #N" with the whole hash for a chained row and "None. This entry was written before the chain began." for a legacy row; detail in a block labelled "Entry detail"; Escape closes it |
| `entry_detail_is_shown_as_text_and_says_when_it_was_cut` | `detail` of `<b>x</b>` appears literally; `detail_truncated` shows "Shortened to 4 KiB." |
| `load_more_fetches_the_next_cursor_and_keeps_the_rows` | `next_cursor` set: click "Load more": second request has `cursor`; rows from both pages; a failing second page keeps the first and shows Retry |
| `the_tab_choice_lives_in_the_url` | clicking "Legacy history" sets `?tab=legacy`; the chained tab has no parameter; an unknown `?tab=` reads as chained |
| `a_failed_entries_load_is_a_load_error_not_an_empty_table` | `failure(500, …)` on `GET /audit`: "Could not load audit entries" with Retry |
| `refresh_refetches_the_selected_tab` | click "Refresh": one more `GET /api/v1/audit` with the tab's `history` |
| `an_unknown_history_status_or_reason_renders_neutral_and_never_throws` | mutate fixtures with `"from_the_future"` values |
| `the_page_notes_that_it_ignores_the_host_selector` | `AllHostsNote` text with a host selected |

Settings and retention (`src/features/settings/retention.test.tsx`; the existing settings tests must pass **unchanged** first):

| Test | Expected |
|---|---|
| `retention_is_a_section_with_nine_period_fields_and_the_kept_list` | `/settings/retention`: a region "Retention"; nine number inputs labelled "… (days)" in the order of `RETENTION_KEYS`; "Kept, with no period" with "Operations and their summaries" and a link to `/audit`; "What deletion does" with the backup counts from the settings answer |
| `retention_fields_show_their_default_and_the_not_applied_yet_help` | the placeholder is the default from the schema fixture; the declared keys' help starts "Not applied yet" (use schema fixtures with that text) |
| `a_field_set_by_file_or_environment_is_locked_with_its_source` | `fields: { "retention.signals_days": "env:DOMOVOI_RETENTION__SIGNALS_DAYS" }`: the input is read-only with the source tag |
| `changing_a_retention_period_asks_for_the_password_before_any_request` | change `retention.signals_days` to 14, press Save: dialog "Confirm with your password" with the text "A shorter period deletes data sooner. Enter your password to save it."; **no** `PATCH` yet; typing the password sends `{ base_revision, changes: { retention: { signals_days: 14 } }, password }` |
| `a_wrong_retention_password_keeps_the_dialog_and_says_so` | `failure(403, "forbidden", …)`: "That password is not right." in the dialog |
| `a_mixed_change_uses_the_trust_wording` | change a retention period and `web.trusted_proxies`: the dialog uses the "who can act or what is trusted" sentence |
| `a_retention_value_out_of_range_is_placed_on_its_field` | the PATCH answers the issue `retention.signals_days` "must be 1 to 3650 days": it appears on that field |
| `is_sensitive_covers_retention_and_keeps_the_old_rules` (`form/sensitive.test.ts`) | `retention.signals_days`, `retention` true; `retentionx`, `hub.name` false; the existing cases unchanged |
| `backups_section_has_the_pre_upgrade_field` | `/settings/backups` shows "Pre-upgrade backups to keep" |
| `a_file_managed_hub_shows_the_retention_values_read_only` | `editable: null` and a `retention` block: nine rows ("30 days"), "Not applied yet" three times with "Applies once …", the two backup rows; no inputs |
| `a_hub_that_sends_no_retention_block_shows_the_fallback` | settings answer without `retention`: "This hub did not report its retention settings." |
| `retention_kept_failures_are_load_errors` | `failure(500, …)` on `GET /settings`: "Could not load retention settings" |

Navigation, routes, visibility, catalogue:

| Test | Expected |
|---|---|
| `audit_log_is_in_the_system_group_before_settings_and_in_the_palette` (`routes.test.tsx`, `palette.test.tsx`) | the nav item and the palette entry |
| `audit_route_renders_the_page` | `/audit` |
| `p0_visible_in_every_profile_on_the_audit_page` | `expectP0Everywhere` over `/audit` with a **broken** chain fixture and both tabs' rows, `expectedIds = ["audit.verification", "audit.limits", "audit.entries"]`; also for a clean fixture; and with a page override in both directions |
| `standard_collapses_the_anchor_details_and_keeps_every_other_block` | Standard and Essential: a toggle "Anchor details" with the count 5; the five anchor facts are not on screen until it is opened; verification, limits and entries unchanged |
| `the_profile_never_changes_which_requests_the_audit_page_makes` | the set of calls in Detailed and Essential is equal apart from the preferences request |
| `the_visibility_gate_lists_this_pages_p0_ids` | the 37 gate test `every_p0_id_of_the_table_appears_in_the_sources` includes `audit.verification`, `audit.limits`, `audit.entries`, `retention.kept`, `retention.guarantee` (extend its list) |
| `audit_demos_render_every_state` (`dev/auditDemos.test.tsx`, `skipAxe()` with a comment) | cards for each status of both vocabularies, a broken chain card, the entry dialog |

### 12.4 Smoke test (extends `npm run smoke`)

Checks 12 and 13 of 6.9 pass against the built bundle in a real browser (the only place colour contrast, real focus and the
375 px layout are exercised), with no `console.error` and no unexpected API request.

### 12.5 Guardrails that must keep passing unchanged

`guardrails.test.ts`, `cssGuard`, `contrast`, `voice`, `tone`, `messages`, the 250-line cap (check `tone.ts`, `ReadOnlySettings.tsx`,
`EditorForm.tsx`), no inline style, no native `title`, no raw `<button>` in `src/features` or `src/app`, every prompt 30 and 37 test, the
existing settings and routes tests, and the hub's `gates.rs`.

## 13. Acceptance criteria

1. `AuditInspect` exists in `domovoi-core` with the signatures of 6.1, is implemented by `Store` and `InMemoryStore`, and is read only by
   `api/audit.rs` (gate); it has no write method and the audit routes never append, journal or open a transaction.
2. `GET /api/v1/audit` returns rows newest first with `seq`, `at`, `actor`, `kind`, `subject`, redacted bounded `detail`, `history`
   and `hash`; `history` is decided by the anchor, never by the presence of a hash; `history` and `kind` filter; cursor and limit page.
3. `GET /api/v1/audit/verification` reports the legacy and the chained part **separately** with the statuses and fields of 6.3, the
   anchor and the head; `recent` is the start-up bound, `full` is on request; a verification that cannot run is a 500, never a status.
4. `SettingsDto.retention` carries the nine periods in force in registry order, whether each applies now, and the two backup counts.
5. The Audit page at `/audit` shows both histories' status cards with their own pills, the head hash with copy and the off-host advice,
   the "What this check can show" note, anchor details, two tabs of entries with load more, the entry dialog, and a "Check the whole log"
   button that runs the full check only on request; every state (loading, error, empty, broken, changed, count-only) reads correctly and a
   failed or missing answer is never shown as intact or verified.
6. No string on the page, in the vocabularies or in the book claims the log is tamper-proof or secure; the limits sentence is present.
7. The Retention section edits the nine periods through the settings editor (source, lock, default, validation), shows what is kept with
   no period, what deletion guarantees and the backup counts, and `backup.keep_pre_migrate` is editable under Backups.
8. Saving a retention change asks for the password **before** any request, with wording about deleting data sooner; a mixed change uses the
   trust wording; the hub's rule is unchanged and still enforced; `isSensitive` knows `retention`.
9. A hub managed by file shows the same values read-only, three marked "Not applied yet" with their reasons.
10. Every element of the Audit page and of `RetentionKept` carries the category of 6.7 and 6.8 through 37's kit; P0 is present and expanded in
    all three profiles, with page overrides in both directions (`p0_visible_in_every_profile_on_the_audit_page`); Standard and Essential
    collapse only the anchor details.
11. Unknown values (status, reason, history, retention key) render neutral and never throw; an older hub's missing route or field is a
    `LoadError` or the fallback sentence, never an empty log.
12. `cargo xtask openapi && npm run gen:api` was run, the insta snapshot was read and accepted, and `npm run gen:api` then leaves no diff.
13. Every new Rust file is at most 500 lines and every new web file at most 250 (`tone.ts`, `ReadOnlySettings.tsx`, `EditorForm.tsx` included); no
    raw colour, inline style, native `title` or raw `<button>` was added; icons only through `Icon`; no migration, `Cargo.toml` or wire change.
14. The browser smoke test passes checks 12 and 13 including colour contrast, in both colour schemes and each profile.
15. `cargo fmt`, `clippy`, `nextest` for the touched crates, `cargo xtask arch-check`, `npm run format:check`, `lint`, `typecheck`, `test`, `build`
    (size within budget) and `smoke` pass; every existing test passes unchanged except the fixtures that gained a field.
16. `web/DESIGN.md`, the book (`guide/audit.md`, `retention.md`, `data-model.md`, `api.md`, `SUMMARY.md`), `ARCHITECTURE.md` and `/dev/design` are
    updated as section 15 says, with no prompt number or build-plan reference in any committed file.

## 14. Migration and compatibility

- **Existing installs and data.** Nothing to migrate. Every existing install already has an anchor and a chain (the chain was introduced
  earlier); the page reads them as they are. A database whose anchor was edited shows the page's load error, not a pass.
- **Existing tests.** Tests that build a `SettingsDto` answer (`test/settingsFixture.ts`, `system.test.tsx`, `settings.test.tsx`,
  `settingsFields.test.tsx`, `dev/` fixtures) gain the `retention` field through the shared fixture; no assertion changes. Tests that assert an
  exact navigation list (`routes.test.tsx`, `layout.test.tsx`, `palette.test.tsx`) gain the "Audit log" item; change only that list.
  `ApiDeps` constructions gain one field. The existing `isSensitive` cases are untouched; one case is added.
- **Hub ordering with other U4 prompts.** The policy, knowledge, capability and Control Room prompts also add routes, nav items and
  `VISIBILITY_PAGES` entries; whichever merges second keeps both sides (alphabetical `PAGES`, one `.routes(routes!(..))` line per handler).
  The Control Room prompt consumes nothing from this one.
- **Rollback.** The change adds no schema and one additive JSON field; an older hub binary ignores nothing it needs. Removing the page
  later is deleting files and three lines of wiring.
- **Later pruning.** When an audit pruner exists, the page already renders checkpoint rows and the `not_verifiable` legacy status; the owner
  of the pruner only needs `Applies`-style wording in the retention view if a period is added for audit (none is planned: audit has no day count).

## 15. Documentation requirements

Write for the product's readers; never mention prompts, prompt numbers, the build plan or agents in any committed file.

- **`web/DESIGN.md`** (same PR; re-read it as the earlier prompts left it): (a) "The shell": the System group now holds "Audit log" before
  Settings. (b) "Tones" or "Add a status" note: the two audit vocabularies (`legacyStatusMeta`, `chainStatusMeta`) and why their lookups live
  in `design/auditVocab.ts` (the `tone.ts` line cap). (c) "How people use it": a short "Is the record intact?" paragraph (open Audit log;
  both histories have their own status in words; copy the head hash off-host; the whole-log check is on request) and, under "Change a
  setting", one line that a retention change asks for the password first. (d) Layout patterns: the audit page pattern (two status cards,
  limits note, tabs of entries, entry dialog). (e) Category table: add the audit page and the retention blocks (the tables of 6.7 and 6.8)
  to 37's "Classify an element" tables. (f) "Planned": nothing to remove. (g) "Performance budget": record the new `npm run size` numbers.
- **Book.** New page `docs/book/src/guide/audit.md`, "Audit log", linked from `SUMMARY.md` under Guides after Retention: what the page shows
  (legacy and chained history, status words, head hash, anchor details, entry details), what the chain can and cannot show (the honest
  paragraph of section 2, no overclaiming), why to keep the head hash off the host, how the page relates to `domovoi audit verify` and
  `domovoi audit head`, what `audit.chain_broken` and `audit.checkpoint` entries mean, and that the page only reads. `guide/retention.md`:
  replace "in **Settings → Retention** once the page ships" by the real location, mention the password prompt, the "Not applied yet" marks
  and the file-managed read-only view. `reference/data-model.md`: one sentence in "Legacy and chained audit history" that the web UI shows the
  two parts too. `reference/api.md`: a new "Audit log" section (`GET /api/v1/audit` with `history`, `kind`, `cursor`, `limit`; `GET
  /api/v1/audit/verification?scope=recent|full`; the response fields; 400 texts) and one sentence in "Settings" that `retention` is part of the
  settings answer. Link only to pages that exist; run `cargo xtask docs-links`.
- **`ARCHITECTURE.md`**: one row `AuditInspect | Read audit rows with their chain fields, the anchor and verification | store, testkit | hub` in the §6
  port table, and one line in §10 naming `/api/v1/audit` and `/api/v1/audit/verification`. No other section.
- **OpenAPI**: `cargo xtask openapi && cd web && npm run gen:api`, then `cargo insta review`. **Config reference**: no config key changed, so
  `cargo xtask docs-config` is not needed (run it with `--check` if available to prove it). No ADR (no arch rule, boundary or invariant changes).
- **`AGENTS.md`**: no change (no command or rule changes).

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

Also state in this report: every real name from prompts 30 and 37 that differed from section 3 (`Visible` props, `VISIBILITY_PAGES`, the smoke mock's
route table); the new `npm run size` numbers; the line counts of `tone.ts`, `ReadOnlySettings.tsx` and `EditorForm.tsx` before and after; the two
observations of section 1 (the web `isSensitive` gap for `metrics` and `llm.*.cli`, and the backup counts missing from the step-up list) as open
items for the owner; whether `cargo xtask docs-links`, `node scripts/screens.mjs` and the real-browser smoke test were run; and the number of P0
elements per fixture that `p0_visible_in_every_profile_on_the_audit_page` asserted.
