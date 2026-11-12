# 05 — Migration strategy

Principle: the product on the branch stays usable after every slice (single owner, D-01, D-13).
New mechanisms run beside old ones (feature switch, shadow or adapter) until a parity suite passes;
the old path is removed in its own PR. No two critical subsystems are replaced in one milestone.
There is no separate v0.1 release (D-01).

## 1. Subsystem decisions

| Subsystem | Decision | Evidence / reason |
|---|---|---|
| Crate graph, ports, `Clock`, runner, argv-only | Retain | FACT: enforced by arch-check (A §1, F §1) |
| Transport, enrolment, ed25519 identity | Retain | FACT (D §1) |
| Protocol version rule and decoding | Refactor (RW-17) | FACT: newer minor rejected permanently; `serde(other)` drops values |
| Protocol messages | Extend additively with features | old agents keep working |
| Incident manager (ADR 0011) | Retain; writes move to unit of work; timeline → journal | FACT: writes not atomic |
| Action service (ADR 0019) | Refactor into the operation writer's execution module (RW-04) | strongest existing safety code; one owner |
| `HumanApproval` restriction | Strengthen (clippy disallowed-methods + arch rule + scan, RW-11) | FACT: text scan only |
| Triage queue + deferred retries | Replace with Investigate operations (RW-04) | FACT: not reloaded at start |
| `recheck.rs` in-memory verification | Replace with Verification steps + `Wake` effects | FACT: lost on restart |
| `PROPOSALS:` text protocol | Remove after RW-06 | lesson #110 |
| `ReadTool` enum | Wrap as built-in read contracts | keeps INV gates |
| Skill manifests and agent revalidation | Retain; wrap as `skill.<name>` | owner trust boundary |
| `PolicyLink` | Replace with registry checks inside the single execution boundary | FACT: consumer-handed wrapper |
| `Hello.capabilities`, `RevertSpec` | Remove / repurpose in protocol slice | FACT: dead data |
| Notes + FTS | Retain; add subject links | works |
| Host profile Markdown note | Replace incrementally with `discovered` facts | provenance |
| `digest_facts` | Rename to `metric_samples` | name clash |
| `incident_events` | Copy 1:1 into `journal`, then retire | one journal (RW-15) |
| `audit_log` | Retain; same transaction; hash chain from first row written after the migration | RW-27 |
| Chat page | Replace with Operator conversations | FACT: sessionStorage |
| SSE stream | Replace with journal-backed SSE | FACT: no ids/replay |
| Web kit, tokens, tone, guardrails | Retain; extend for graph, profiles, install wizard | FACT (B §9) |
| Approval cards | Replace with one structured component | RW-22 |
| `/healthz` | Refactor; add `/readyz` | FACT: constant |

## 2. Data migration rules

- Additive migrations only, except retiring tables after a copy has shipped for at least one
  release. No rebuild of tables protected by triggers (`audit_log`) or carrying FTS rowids.
- New enums validated in Rust, not with SQL CHECK (FACT: CHECK widening forced two advisory table
  rebuilds, migrations 0003/0004).
- First migration of the rework: `version` columns on `incidents`, `notes`; `schema_version` key in
  `incidents.body`; `schema_compat` table (RW-31).
- `incident_events` → `journal`: rows copied in `(at, id)` order with original timestamps and kinds
  mapped to typed kinds; unmappable kinds copied as `Legacy{raw_kind}`. No events invented.
- Audit chain: existing rows are not rehashed in place (triggers forbid updates). The first chained
  row records `prev_hash = SHA-256(install_id ‖ count_of_prior_rows ‖ hash_of_their_canonical_dump)`,
  so pre-chain rows are covered by one anchor without being modified.
- Legacy actions (RW-32): rows stay in `actions`; Operations list and incident page show them via a
  read-only projection labelled **Legacy**. Nothing is synthesized. The release that removes the
  legacy executor expires remaining `Proposed/Approved` legacy rows with an audited reason.
- Triage reports: stay readable on the incident page; new investigations are operations.
- Host profile notes → facts: backfill writes `discovered` facts with `evidence_ref = note:<id>`;
  the note is kept and labelled "generated; superseded by facts".
- `digest_facts` → `metric_samples`: new table, copy, view under the old name for one release.
- Every migration slice ships an N-1 upgrade test from a committed fixture database.

## 3. Rollback and recovery (RW-31)

| Situation | Procedure | Data effect |
|---|---|---|
| Bad release, schema additive | run previous binary; it checks `schema_compat` and ignores unknown tables/columns | none |
| Bad release, schema not readable by previous binary | forward fix (new release) | none |
| Corruption or unrecoverable migration | `domovoi restore <backup>` (explicit, confirmation required) | everything after the backup is lost; restore is audited in the new chain; agents' journals may report executions newer than the backup — shown as "executed after restored backup" |
| Old binary on newer schema below `schema_compat.min_reader` | binary refuses to start with a message | none |

Restoring a pre-migration backup is never presented as a routine rollback.

## 4. Protocol compatibility

See `04` §12. New hub + old agent: adapter gives human-approved read tools and skills only;
unattended, reconciliation, install and containment refused with "agent too old". Support window
OPEN (recommended: current + previous two minors until 1.0, then same major). An agent below the
window is rejected with a final, explicit reason (FACT: reject reason mechanism exists since #86).

## 5. Configuration

- New optional sections with conservative defaults: hub `[orchestration]`, `[policy]`,
  `[retention]`, `[packages] registries`; agent `[unattended]`, `[packages]`.
- Absent sections = today's behaviour (no unattended, no installs).
- Settings managed in the UI keep their config-file source label (FACT: ADR 0017).
- `cargo xtask docs-config` regenerated in each slice that adds config.

## 6. Feature switches

`orchestration.enabled` (Investigate operations instead of triage queue), `policy.shadow` (record
decisions), `autonomy.enabled` (permits can be issued; still needs per-rule promotion out of shadow
and host ceilings), `packages.enabled`, `containment.enabled`. All off until their milestone's
acceptance passes; global pause available from M8.

## 7. Parity checkpoints before removal

- triage → Investigate: same report fields, budgets, fallback, host scoping; existing scripted
  triage drills (FACT: 6 drills, F §1) pass unchanged.
- actions → execution module: all ADR 0014/0019 tests and action gates pass on the new path.
- chat → Operator: host-scoped read-only Q&A unchanged, now durable.
- SSE v1 → journal SSE: every event type the web uses has a journal equivalent.

## 8. Never lost

Audit rows, approvals and approvers, incident history within retention, notes, enrolment keys and
tokens, settings with provenance, legacy actions. Each has a store contract test before any
migration touching it.
