# 05 — Migration strategy

Principle: **the product stays releasable after every milestone.** New mechanisms run beside old
ones (shadow or adapter) until gate tests prove parity; then the old path is removed in its own PR.
Never replace two critical subsystems in one milestone.

## 1. Subsystem decisions

| Subsystem | Decision | Why |
|---|---|---|
| Crate graph, ports, Clock, runner, argv-only | **Retain** | sound, enforced (A §1, F §1) |
| Hub/agent transport, enrolment, ed25519 identity | **Retain** | sound (D §1) |
| Protocol versioning rule | **Refactor** (RW-17) | newer minor stranded today |
| Protocol messages | **Extend additively** (features, `run_capability`, `action_status`) | old agents keep working |
| Incident manager (ADR 0011) | **Retain, refactor writes to unit of work** | pattern is right; writes are not atomic |
| Action service (ADR 0019) | **Refactor incrementally** into the execution writer (RW-08) | strongest existing safety code |
| `HumanApproval` seal | **Retain, strengthen** to compile-time seal (RW-11) | text scan evadable |
| Triage queue + deferred retries (in memory) | **Replace behind interface** with Investigate operations (RW-04) | not restart-safe |
| `recheck.rs` in-memory verification | **Replace** with persisted Verification steps + timers | lost on restart |
| Triage timeline-string control flow | **Replace** with typed steps | stringly typed |
| `PROPOSALS:` text protocol, cached proposals | **Remove** after RW-06 lands | lesson #110 |
| `ReadTool` enum | **Wrap** as built-in read contracts; enum stays as the closed implementation list | keeps INV gates |
| Skill manifests / agent revalidation | **Retain**, wrap as `skill.<name>` contracts | owner trust boundary |
| `PolicyLink` per-host skill narrowing | **Replace behind interface** by registry enable state + single execution boundary | consumer-handed wrapper today |
| `Hello.capabilities`, `RevertSpec` (dead data) | **Remove/repurpose** in the protocol milestone | dead |
| Notes + `notes_fts` | **Retain**, add subject links | works |
| Host profile as generated Markdown note | **Replace incrementally** with discovered facts; keep note for one release | provenance |
| `digest_facts` | **Rename** to `metric_samples` | name clash |
| `audit_log` | **Retain**, move into tx, add chain + denied events | security record |
| `incident_events` | **Retain** for incident lifecycle; triage kinds move to operation steps | experience |
| Chat page (sessionStorage) | **Replace** with Operator conversations | not durable |
| SSE stream | **Replace** with SSE v2 (ids, replay, topics) | drops events |
| Web kit, tokens, tone, guardrails, `useActionFeedback` | **Retain** | strong (B §9) |
| Web pages | **Redesign** per IA, page by page, old routes redirect | pack direction |
| Approval cards (incident-bound) | **Replace** with one structured approval component | RW-22 |
| `/healthz` | **Refactor** + add `/readyz` | constant today |

## 2. Data migration

- All new tables are **additive** migrations; no existing table is rebuilt (C: `audit_log` triggers
  block rebuilds; FTS rebuilds change rowids; advisory CHECK widening already caused two rebuilds).
  New enums are validated in Rust, not with SQL CHECK.
- First data migration: add `schema_version` to `incidents.body` JSON and `version` columns for
  optimistic concurrency.
- Existing actions: stay in `actions`; the execution writer reads both shapes. Historic actions
  appear in Operations as **legacy operations** created lazily by a read-model view (no rewrite of
  rows). New work writes operations.
- Triage reports: remain readable; new investigations produce operations. The incident page shows
  both during the transition.
- Host profile notes → facts: a one-time backfill job writes `discovered` facts with
  `source_ref = note id`; the note is kept and marked "generated, superseded by facts".
- `digest_facts` rename: create new table, copy, keep a view under the old name for one release.
- Every migration milestone ships an **N-1 upgrade test**: open a database produced by the previous
  release fixture, migrate, assert row counts and key invariants.

## 3. Protocol compatibility

| Hub \ Agent | old agent (no features) | new agent |
|---|---|---|
| old hub | today | new agent sees no features, behaves as today |
| new hub | adapter: legacy skills/read tools as capabilities; human approval only; dispatched-at-restart → `OutcomeUnknown` | full features |

Policy-authorized execution is refused for agents without `journal` + `run_capability` features;
the UI shows "agent too old for unattended actions".

## 4. Configuration

- New sections (`[orchestration]` budgets, `[policy]` defaults, agent `[policy] unattended`)
  are optional with conservative defaults; absent = today's behaviour.
- Settings that move to the UI keep their config-file source label (existing settings
  framework, ADR 0017).
- `cargo xtask docs-config` regenerated per milestone.

## 5. Operational continuity and rollback

- Every milestone is a minor release. Rollback = previous binary + the automatic `pre-migrate-*`
  backup (exists today). Add a `domovoi restore` command in M0 and document that a downgrade
  across a migration requires restore.
- Feature switches: operations path (`orchestration.enabled`), policy shadow, autonomy each have a
  config switch so a release can ship with the new path dark.
- Autonomy ships **off**; per-rule shadow first; global pause available from day one.

## 6. Feature parity checkpoints

Before removing an old path, a parity test suite must pass on the new one:
- triage → Investigate operation: same report fields, same budgets, same fallback behaviour, same
  host scoping, drills from F §1 pass unchanged;
- actions → execution writer: all existing action gate tests and ADR 0014/0019 tests pass;
- chat → Operator: host-scoped read-only Q&A unchanged, now durable.

## 7. What must never be lost

Audit rows; approvals and who gave them; incident history inside retention; notes; enrolment
keys/tokens; settings with provenance. Each has a contract test in the store suite before any
migration touching it.
