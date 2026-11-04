# 07 · Feature: manage ack rules ("known conditions") from the web UI

This is the foundation for prompt 08 (dismiss false positives) and prompt 09 (expected
SSH logins). Do it first.

## Goal

A **Known conditions** page lists, creates and deletes ack rules. An ack rule says "this
check on this host (and target, and evidence) is known and acceptable": matching
incidents are still stored but never notify and are not triaged.

## What is already known (verified)

- The domain type exists: `AckRule` and `EvidencePredicate` in
  `crates/domovoi-core/src/ack.rs` (fields: `id`, `host: Option<HostId>`, `check`,
  `target_pattern: Option<TargetPattern>`, `predicates`, `note`,
  `expires_at: Option<Timestamp>`). `TargetPattern::parse` allows `*` only at the start
  and/or end.
- The store implements `AckRepo` (`put_rule`, `delete_rule`, `list_rules`) in
  `crates/domovoi-core/src/ports/repos/knowledge.rs` / `crates/domovoi-store`.
- The incident manager already loads every rule on each ingest
  (`crates/domovoi-engine/src/manager/actor.rs`, `state_for`), and
  `crates/domovoi-engine/src/suppress.rs` marks matching changes `notify: false`; the
  triage queue skips `notify: false` requests. A new rule works immediately; nothing in
  the engine changes.
- There is no API or UI for rules yet. `ApiDeps` (`crates/domovoi-hub/src/api/mod.rs`)
  has no `AckRepo`. `ApiDeps { .. }` is built in three places:
  `crates/domovoi-hub/src/runtime/mod.rs`, `crates/domovoi-hub/examples/dev_hub.rs`,
  `crates/domovoi-hub/tests/hub/api_harness.rs`. All three have a store that implements
  `AckRepo`.
- `ApiDeps.audit` is the audit log; `AuditEvent { at, actor, kind, subject, detail }`.

## Read these files

- `crates/domovoi-core/src/ack.rs`
- `crates/domovoi-hub/src/api/mod.rs`, `api/notes.rs` (a small CRUD module to copy the
  style from), `api/dto_admin.rs`
- `web/src/features/notes/NotesPage.tsx` (page style), `web/src/app/routes.tsx`,
  `web/src/app/Layout.tsx`

## Steps

1. `ApiDeps`: add `/// Ack rules.` `pub acks: Arc<dyn AckRepo>,` and fill it in the three
   construction sites with the store they already use (`store.clone()` or equivalent).
2. Create `crates/domovoi-hub/src/api/rules.rs` (module doc: "Ack rules: known
   conditions that never notify.") with DTOs in the same file or in `dto_admin.rs`:
   ```rust
   pub struct AckRuleDto { id, host: Option<String> /* host name */, check: String,
       target_pattern: Option<String>, evidence: Vec<PredicateDto>, note: String,
       expires_at: Option<String> }
   pub struct PredicateDto { key: String, op: String /* equals|contains|at_most|at_least */, value: String }
   pub struct AckRuleInput { host: Option<String>, check: String, target_pattern: Option<String>,
       evidence: Vec<PredicateDto>, note: String, expires_in_minutes: Option<u32> }
   ```
   Map `equals` to `EvidencePredicate::Equals` with the value parsed as `i64`, else
   `f64`, else `true`/`false`, else text; `contains` to `Contains`; `at_most` /
   `at_least` to `LessOrEqual` / `GreaterOrEqual` (value must parse as `f64`, else 400).
3. Handlers (all need `Authed`, tag `"incidents"`):
   - `GET /api/v1/ack-rules` → every rule, newest expiry last, expired rules included
     with their `expires_at` so the UI can grey them out.
   - `POST /api/v1/ack-rules` → validate: `CheckKey::parse(check)`, host by name with
     `host_named` (404 → 400 "unknown host"), `TargetPattern::parse`, note 1–500 chars,
     `expires_in_minutes` 1–525600. Store with `AckRuleId::new()` and
     `expires_at = now + minutes`. Append an audit event
     `kind: "ack_rule.created"`, `actor: format!("user:{}", current.user.id)`,
     `subject: format!("ack_rule:{id}")`. Return the DTO.
   - `DELETE /api/v1/ack-rules/{id}` → delete, audit `ack_rule.deleted`, 204.
   Register the routes in `routes()` and regenerate OpenAPI (rules, step 9).
4. Web: add `web/src/api/hooks/rules.ts` (`useAckRules`, `useCreateAckRule`,
   `useDeleteAckRule`; key `keys.ackRules = ["ack-rules"]`).
5. Web: add `web/src/features/rules/RulesPage.tsx` at route `/rules`, nav label
   **Known conditions**. Top text: "Incidents that match a known condition are kept in
   the history but never alert you and are not triaged." A table lists host (or "any
   host"), check, target, evidence conditions, note, expiry ("never", relative time, or
   "expired"), and a Delete button with `ConfirmDialog`. A form adds a rule: host select
   (with "Any host"), check (text, placeholder `security.ssh_login`), target pattern,
   note, expiry select (1 hour, 1 day, 1 week, 30 days, never), and an optional list of
   evidence conditions (key, op select, value).
6. Docs: add a "Known conditions" section to `docs/book/src/concepts/incidents.md`.

## Tests

- `crates/domovoi-hub/tests/hub/`: add tests (in a new `rules.rs` registered in
  `tests/hub/main.rs`, using `api_harness.rs`) for create → list → delete, a bad target
  pattern (400), and that `POST` without a CSRF token is refused (copy an existing CSRF
  test).
- Add `(Method::POST, "/api/v1/ack-rules")` and
  `(Method::DELETE, "/api/v1/ack-rules/<any uuid>")` to the `writes` array of
  `settings_writes_require_admin_session_and_csrf` in `crates/domovoi-hub/tests/gates.rs`
  (gate tests may be extended, never weakened).
- Web: `web/src/features/rules/rules.test.tsx`: renders rules, creates one (check the
  request body), deletes one.

## Done when

- Rules can be created, listed and deleted from the UI, and creating one is audited.
- A new matching signal does not notify (the engine already does this; no engine change).
- OpenAPI regenerated; Rust and web tests pass.

## Do not

- Do not change `domovoi-engine`. Do not add a migration (the table exists).
