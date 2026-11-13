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
| Protocol version rule and decoding | Refactor to exact major.minor match (D-20, RW-17) | FACT: today same-or-one-behind minor plus a release-number gate; `serde(other)` drops values |
| Protocol messages | Extend; every minor bump is a coordinated hub + agent upgrade (D-20) | no legacy adapter |
| Incident manager (ADR 0011) | Retain; writes move to unit of work; timeline → journal | FACT: writes not atomic |
| Action service (ADR 0019) | Refactor into the operation writer's execution module (RW-04) | strongest existing safety code; one owner |
| `HumanApproval` restriction | Strengthen in CI (clippy disallowed-methods + arch rule + scan, RW-11); type fields unchanged, plan/session binding stored on the execution | FACT: text scan only |
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
- Audit chain (RW-27): existing rows are never rewritten. The migration writes one `audit_anchor`
  row (install id, last legacy id, legacy count, SHA-256 over the canonical serialization of every
  legacy row); the first chained row commits to that anchor. Fresh installs write the same anchor with
  count 0. `domovoi audit verify` and the UI report legacy (unchained, digest-checked) and chained
  history separately.
- Execution identity (RW-36): from F2/M1 the agent journal and plan hash v2 key on `execution_id`
  (= `ActionId`); from M3 new executions use the step id. An id never changes after dispatch; legacy
  actions pending at the M3 upgrade finish on the legacy path under their `ActionId`.
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

## 4. Protocol compatibility (D-20, RW-42)

- Exact protocol major.minor match; no window, no adapter; release floors are removed in F3.
- Protocol stays 1.1 through F1–F4. **M1 is the only coordinated upgrade before v1 (1.1 → 1.2)**:
  upgrade the hub, then every agent through the supported install path. Until an agent is upgraded
  its host shows `Incompatible` (persisted after authentication, survives hub restart); history and
  inventory stay; work for it is `Unavailable(agent_incompatible)`.
- Agents from the F3 release onward idle without reconnecting when incompatible. Older agents exit
  and are restarted by systemd every 5 s until upgraded; the hub writes their state only on change.
- Executions dispatched before the M1 upgrade (no journal on the old agent) become `OutcomeUnknown`
  when the upgraded agent connects with a new journal epoch.
- Patch releases with the same protocol interoperate. Agent self-update is not provided (INV-3).

## 4a. Agent privilege migration (D-29, RW-39, workstream H1, after M1)

- FACT: `install.sh` installs the agent unit running as root; the runner switches skills to their
  `run_as` user only because the agent is root.
- The installer (rerun by the owner as root) creates `domovoi-agent` and `domovoi-skill`, sets
  `/var/lib/domovoi-agent` to `domovoi-agent` 0700, creates `/var/lib/domovoi-packages`
  (`domovoi-agent`, 0755), installs the helper and skill-runner units and a fresh-install
  `/etc/domovoi/helper.toml` with read groups only, and switches the agent unit to
  `User=domovoi-agent`.
- Existing owner-placed skills in `/etc/domovoi/skills` run as `domovoi-skill`; manifests whose
  `run_as` names another user follow O-30. `domovoi agent selftest` lists affected checks and skills.
- Any `[unattended.exclusions]` in agent config from a pre-release build is ignored with a warning;
  exclusions are configured only in `helper.toml` `[safety]` (RW-37).
- Until the owner reruns the installer, root installs keep working, labelled "privilege boundary not
  in effect", with no unattended execution.
- Platforms without the skill runner (R-9) run without skills rather than running skills as the
  control agent.
- Rollback: rerunning the previous installer restores the root unit; the new units stay inert.

## 4b. Approval migration stages (RW-11)

- F4: action approvals bind the v1 hash shown on the card (no script bytes, no target identity).
- M1: new actions use hash v2 and binding columns; pre-upgrade actions finish with v1, labelled.
- M3: new work is approved on steps; the action endpoint serves legacy actions only.

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
