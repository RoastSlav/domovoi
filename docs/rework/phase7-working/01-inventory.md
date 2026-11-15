# Phase 7 prompt-pack inventory (handoff)

This is an inventory only. It creates no implementation prompt, and it changes no canonical document:
`07-implementation-prompt-index.md`, `06-implementation-roadmap.md` and the prompt files are exactly as
found. Every status below is either read from a document or observed in this checkout; where the two
disagree, both are recorded.

## 1. Repository identity and worktree condition

| | |
|---|---|
| Repository root | `C:\Programming\domovoi` (git repo) |
| Current branch | `durable-timers` — not `overhaul-investigation`; it is the prompt 15 implementation branch |
| HEAD | `acf3278e23bc46546e581ddcdf768d14c953c557` — `fix(engine): track each effect handler task and count dead effects in a batch` |
| Upstream of HEAD | `origin/durable-timers` == `acf3278` (in sync) |
| Base branch state | local `overhaul-investigation` = `452086e` (retention #133), **2 commits behind** `origin/overhaul-investigation` = `55ab5b7` (ops readiness #135). `durable-timers` merged `overhaul-investigation` at `96b46ea` and already contains `55ab5b7`, so this checkout has all merged prompt work. |
| Staged / unstaged | none |
| Untracked | `pr-body.md` (pre-existing user file, 6187 bytes, not read or modified) |
| Stashes / worktrees | none created or touched; no branch switched, merged, rebased, committed or pushed |
| Working-tree age of docs | `docs/rework/` last changed by `3873249` (2026-11-14, LLM integration #131) |

`durable-timers` commits after the merge `96b46ea`: `3323cff`, `7ebe448`, `b059bdf`, `c5d898f`,
`291a370`, `b55f1b0`, `e4165f2`, and after the merge `eca740a`, `7f82e4c`, `4c436bf`, `9e7a2d3`,
`acf3278`. These were prompt 15 work in progress; it merged into `overhaul-investigation` as #136
(`bca079d`) after this report was written.

**Update after this report.** `origin/overhaul-investigation` is now `bca079d` (`feat(engine): run
verification as a durable effect with lease fencing (#136)`). Prompt 15 is merged, migrations still end at
`0010_llm_admission.sql`, and the current checkout is `phase7-inventory` with this inventory and
`02-remaining-work-map.md`. The table above describes the checkout the inventory was written in.

## 2. Canonical documents used

Read in this checkout, not from memory:

- `docs/rework/07-implementation-prompt-index.md` — authoritative for prompt IDs, tables, waves,
  launch order, file status and migration rules (`README.md:5-7` also says so).
- `docs/rework/06-implementation-roadmap.md` — milestones F1–F4, M1–M9, P1, H1, U1–U4, readiness table.
- `docs/rework/03-architecture-decisions.md` (heading map: RW-01 … RW-42).
- `docs/rework/04-target-architecture.md` (heading map: §1 … §19; `§19 Status` at `:1185`).
- `docs/rework/04b-attestation-encoding.md`, `docs/rework/05-migration-strategy.md`,
  `docs/rework/99-open-decisions.md` (decision register; O-30 is **still open** at `:46`).
- `docs/rework/00-executive-findings.md`, `01-repository-reality.md`, `02-design-pack-validation.md`,
  `docs/rework/investigations/*` (used only to resolve statuses).
- Prompt directory: `docs/rework/prompts/`, one file per prompt, naming `NN-branch-name.md`
  (`docs/rework/prompts/README.md:3`).
- Shared-contract / authoring standard: `docs/rework/prompts/README.md` (§`Contract | Owner | Planned
  names`, lines 37–57) and `docs/rework/prompts/_blocks.md` (three verbatim blocks:
  `## Execution constraints`, `## Repository rules`, `## Completion report`).

`docs/design-pack/` was not opened or modified.

## 3. Prompt inventory

Waves are the ones the index actually defines, in `07 §3 "Recommended launch order"`; they are not
derived from numeric ranges. "Doc" states: **complete** = file exists and contains the front matter plus
the standard blocks; **missing** = no file. "Impl" states: **implemented** (merged into the base line),
**in progress** (commits on an unmerged prompt branch), **not implemented**.

Wave membership (index `07 §3`):

- **A** 08 (already present) → 13; research 53, 54, 55, 56 may be written/launched at any point in wave A.
- **B** 09. **C** 10 ∥ 11. **D** 12 ∥ 14 ∥ 16 ∥ 18 ∥ 57 → 17 → 19 → 15.
- **E** 20. **F** 21 → 22 → 23. **G** 24 → (25 ∥ 39). **H** 26 → (27 ∥ 28 ∥ 29).
- **I** 30 ∥ 31 ∥ 34 ∥ 36 ∥ 41 ∥ 40, then 58. **J** 32 ∥ 35 ∥ 37 ∥ 42 ∥ 33.
- **K** 43 ∥ 48 ∥ 49 ∥ 50 ∥ 51 → 38 → 52. **L** 44 → 47. **M** 45 → 46.

### Waves A–D (implemented; do not regenerate)

| ID | Title (as in the prompt document) | Path | Ms | Wave | Doc | Impl | Depends on | Shared contract owned | Action |
|---|---|---|---|---|---|---|---|---|---|
| 08 | Unit of work: one atomic commit for state, journal, audit and effects | `prompts/08-store-unit-of-work.md` | F1 | A | complete | implemented (#114) | — | unit-of-work port, transaction, commit errors; `journal`, `effects`, `schema_compat`; migrations 0006, 0007 | preserve |
| 13 | Exact protocol rule, durable Incompatible state | `prompts/13-exact-protocol-rule.md` | F3 | A | complete | implemented (#116; migration 0008) | 08 | exact protocol rule, durable incompatibility | preserve |
| 09 | Audit hash chain, Actor identity and audit verification | `prompts/09-audit-hash-chain.md` | F1 | B | complete | implemented (#117; migration 0009) | 08 | `Actor`, audit hash chain | preserve |
| 10 | Atomic action writes: status, audit and journal in one commit | `prompts/10-atomic-action-writes.md` | F1 | C | complete | implemented (#118) | 09 | action-handle methods, `action.transition` payload | preserve |
| 11 | Atomic incident writes: signal, incident and timeline in one commit | `prompts/11-atomic-incident-writes.md` | F1 | C | complete | implemented (#120) | 09 | incident version-cache rule | preserve |
| 12 | Crash harness: crash sweep over every action and incident write, and the upgrade flow | `prompts/12-crash-harness.md` | F1 | D | complete | implemented (#121) | 10, 11 | `domovoi_testkit::crash::*`, sweep scenarios | preserve |
| 14 | Host availability, startup order and dispatch freshness | `prompts/14-recovery-order.md` | F2 | D | complete | implemented (#122) | 10 | host availability | preserve |
| 16 | Delivery durability: spool before state, notify state after confirmed delivery | `prompts/16-delivery-durability.md` | F2 | D | complete | implemented (#124) | 08 | none | preserve |
| 18 | Structured approvals (hash v1) | `prompts/18-structured-approvals.md` | F4 | D | complete | implemented (#132) | 10 | `PlanBinding`, `PlanHash::short`, `CardPlan`, `ApprovalError::PlanChanged` | preserve |
| 57 | Configurable retention | `prompts/57-configurable-retention.md` | F4 | D | complete | implemented (#133) | 08 | `[retention]` / `RETENTION_KEYS`, `RetentionPolicy`, `backup.keep_pre_migrate` | preserve |
| 17 | LLM reliability | `prompts/17-llm-reliability.md` | F4 | D | complete, **reconciled** | implemented (#134; migration 0010) | 08 + integrated LLM work | `AdmissionRepo`, `llm_admission`, `LlmError::Timeout`, `prompt_bytes` | preserve; status only |
| 19 | Operations readiness | `prompts/19-ops-readiness.md` | F4 | D | complete | implemented (#135) | 08 | `settings/step_up.rs`, `StoreHealth`, `/readyz`, `verify_backup`, `restore_from_backup` | preserve; status only |
| 15 | Durable timers and the effect lifecycle with lease fencing | `prompts/15-durable-timers.md` | F2 | D | complete | **implemented (#136)**; needs no migration (0006 pre-created the columns) | 08, 10 (14 ordering) | effect lifecycle and lease fencing | preserve; merged |

### Waves E–M (not implemented; author only what is missing)

| ID | Working title (index §2 branch name; no prompt file) | Path | Ms | Wave | Doc | Impl | Depends on | Shared contract owned | Action |
|---|---|---|---|---|---|---|---|---|---|
| 20 | agent-action-journal | — | M1 | E | missing | not implemented (protocol is 1.1, `crates/domovoi-proto/src/version.rs:17`) | 13, 14, 15, 16, 17, 18, 19 | wire 1.2 | author next; needs 15 merged |
| 21 | outcome-unknown-reconcile | — | M1 | F | missing | not implemented | 14, 20 | — | author after 20's brief exists |
| 22 | plan-hash-v2-and-authorization | — | M1 | F | missing | not implemented | 18, 20, 21 | `ExecutionId`, plan hash v2, `Authorization`, `PolicyPermit` | author |
| 23 | single-execution-boundary | — | M1 | F | missing | not implemented | 22 | — | author; blocks 24 |
| 24 | Capability contracts | `prompts/24-capability-contracts.md` | M2 | G | complete | not implemented | 23 (ordering only) | capability contract types | preserve existing doc |
| 25 | capability-registry | — | M2 | G | missing | not implemented | 23, 24 | capability registry and availability | author |
| 26 | operation-aggregate | — | M3 | H | missing | not implemented | 23, 25 | operation aggregate, trace schema | author |
| 27 | investigate-operation | — | M3 | H | missing | not implemented | 26 | — | author |
| 28 | journal-sse | — | M3 | H | missing | not implemented | 26 | journal-backed SSE | author |
| 29 | use-case-layer | — | M3 | H | missing | not implemented | 26 | — | author |
| 58 | remove-legacy-triage-path | — | M3 | I (after parity) | missing | not implemented | 27, 28; gate: parity suite green | — | author late |
| 30 | operations-ui-timeline | — | U1 | I | missing | not implemented | 27, 28 | approval card component | author |
| 31 | meta-tools-and-validation | — | M4 | I | missing | not implemented | 27 | context retrieval port | author |
| 34 | knowledge-store | — | M5 | I | missing | not implemented | 11, 27 | knowledge API | author |
| 36 | operator-conversations | — | M6 | I | missing | not implemented | 28 | — | author |
| 41 | packages-agent | — | P1 | I | missing | not implemented | 20, 25 | — | author |
| 40 | policy-shadow | — | M7 | I | missing | not implemented | 27, 39 | — | author |
| 32 | layered-context | — | M4 | J | missing | not implemented | 31 | — | author |
| 35 | experience-search | — | M5 | J | missing | not implemented | 27, 31, 34 | — | author |
| 37 | visibility-profiles | — | U3 | J | missing | not implemented | 30 | visibility categories | author |
| 42 | packages-hub | — | P1 | J | missing | not implemented | 41 | — | author |
| 33 | graph-spike | — | U2 | J | missing | not implemented | 30 | R-1 evidence | author |
| 39 | policy-evaluator | — | M7 | G | missing | not implemented | 24 | evaluator API | author |
| 43 | policy-pages | — | U4 | K | missing | not implemented | 37, 40 | — | author |
| 48 | knowledge-pages | — | U4 | K | missing | not implemented | 37, 34, 35 | — | author |
| 49 | capability-center-and-install-wizard | — | U4 | K | missing | not implemented | 37, 25, 42 | — | author |
| 50 | audit-and-retention-pages | — | U4 | K | missing | not implemented | 37, 09, 57 | — | author |
| 51 | control-room-attention-and-search | — | U4 | K | missing | not implemented | 37, 28, 30 | — | author |
| 38 | graph-canvas | — | U2 | K | missing | not implemented | 33, 37; gate: R-1 accepted | — | author after 33 |
| 52 | graph-playback | — | U2 | K | missing | not implemented | 38 | — | author after 38 |
| 53 | spike-nft-containment | — | R-2 | A window | missing | not implemented | —; gate: WSL2 available | R-2 evidence | author (can start now) |
| 54 | spike-helper-and-skill-runner-platforms | — | R-3, R-9 | A window | missing | not implemented | —; gate: WSL2 available | R-3, R-9 evidence | author (can start now) |
| 55 | spike-safety-attestation | — | R-10 | A window | missing | not implemented | —; no environment gate | R-10 evidence | author (can start now) |
| 56 | spike-systemd-unit-identity | — | R-8 | A window | missing | not implemented | —; gate: WSL2 available | R-8 evidence | author (can start now) |
| 44 | helper-adrs | — | H1 | L | missing | not implemented | gate: 54 and 55 evidence, or unproven items recorded | ADRs | author after evidence |
| 47 | three-identities-helper-and-skill-runner | — | H1 | L | missing | not implemented | 20, 22, 44; gate: O-30 answered, R-3/R-9/R-10 accepted | helper protocol, `SafetyIdentity` | block: O-30 open |
| 45 | policy-permit-autonomy | — | M8 | M | missing | not implemented | 32, 40, 47; gate: R-8 accepted, H1 complete | — | block: gates |
| 46 | containment | — | M9 | M | missing | not implemented | 45, 34, 47; gate: R-2, R-10 accepted | — | block: gates |

## 4. Wave-by-wave status

| Wave | Prompts | Documents | Implementation |
|---|---|---|---|
| A | 08, 13 (+ 53–56 research window) | 08, 13 complete; 53–56 missing | 08, 13 implemented; research not started |
| B | 09 | complete | implemented |
| C | 10, 11 | complete | implemented |
| D | 12, 14, 16, 18, 57, 17, 19, 15 | all 8 complete | all 8 merged; 15 merged as #136 |
| E | 20 | missing | not started; protocol constant is still `1.1` |
| F | 21, 22, 23 | missing | not started |
| G | 24, 25, 39 | 24 complete; 25, 39 missing | not started |
| H | 26, 27, 28, 29 | missing | not started |
| I | 30, 31, 34, 36, 41, 40, 58 | missing | not started |
| J | 32, 35, 37, 42, 33 | missing | not started |
| K | 43, 48, 49, 50, 51, 38, 52 | missing | not started |
| L | 44, 47 | missing | not started; O-30 open, R-3/R-9/R-10 open |
| M | 45, 46 | missing | not started; R-8, R-2, R-10 open |

Counts: 51 prompt IDs in `07 §2`; 14 documents exist; 37 do not. 13 of the 14 documents are waves A–D;
1 (prompt 24) is wave G. No prompt ID has more than one document; no document lacks an index row.

## 5. Confirmed status

- **Waves A–D implemented**, as the owner stated. Each maps to a merged commit on the base line:
  08 #114 `f562d06`; 13 #116 `4edda64`; 09 #117 `e3252aa`; 10 #118 `689e2df`; 11 #120 `b9bd5ca`;
  12 #121 `bd8beb5`; 14 #122 `c9c28d9`; 16 #124 `9512ada`; 18 #132 `8f8b3d6`; 57 #133 `452086e`;
  17 #134 `958dc40`; 19 #135 `55ab5b7`. Migrations present: `0006_unit_of_work.sql`,
  `0007_advisories_kind_unchecked.sql`, `0008_agent_protocol_report.sql`, `0009_audit_chain.sql`,
  `0010_llm_admission.sql`. Names match the migrations the prompts specify.
- **Prompt 15 merged as #136 (`bca079d`)** after this report was written; waves A–D are now all merged.
  It is the same work this report inspected on `durable-timers`, now on the base line.

Implementation evidence (paths and named tests, read in this checkout):

| ID | Evidence |
|---|---|
| 08 | `domovoi-core/src/ports/uow/mod.rs:28 trait UnitOfWork`; `domovoi-store/src/uow/mod.rs:37`; `domovoi-testkit/src/repos/uow.rs:152`; migrations `0006`, `0007` |
| 09 | `domovoi-core/src/actor.rs:17`, `audit_chain.rs`; `domovoi-store/src/audit_verify.rs:22`; CLI `domovoi/src/cmd/audit.rs`; `domovoi-store/tests/sqlite/audit_chain.rs` (`audit_chain_detects_edit`) |
| 10 | `domovoi-engine/src/actions/{mod.rs:78,commit.rs:113}`; `tests/services/atomic.rs` (`audit_failure_rolls_back_transition`) |
| 11 | `domovoi-engine/src/manager/{actor.rs:161,commit.rs}`; `tests/engine/actor.rs` |
| 12 | `domovoi-testkit/src/crash/{mod.rs,seams.rs}` (`CrashingStore`, `SweepStore`); `tests/crash/{sweep,coverage}.rs`; `domovoi-hub/tests/hub/crash_sqlite.rs` |
| 13 | `domovoi-proto/src/version.rs:22` (same major and minor); `domovoi-hub/src/agent_server/compat.rs`; `HostAvailability::Incompatible` |
| 14 | `domovoi-core/src/ports/agent_link.rs:92`; `engine/src/actions/host_wait.rs`; `tests/services/freshness.rs` |
| 15 | `core/src/ports/uow/{effects.rs,lifecycle.rs}` (`EffectKind::Wake`, claim token, fencing, dead-letter); `store/src/uow/{effects.rs,queue.rs}`; `engine/src/effects/{runner.rs,backoff.rs}`, `actions/verify_wake.rs`; `tests/services/effects.rs` (`verification_resumes_after_restart`, `verification_resumes_after_a_restart_that_held_the_claim`) |
| 16 | `domovoi-hub/src/runtime/{delivery.rs,delivery/state.rs,dispatcher.rs}`; `domovoi-agent/src/spool.rs`; `hub/src/runtime/tests/delivery_state.rs` |
| 17 | migration `0010_llm_admission.sql`; `core/src/ports/repos/admission.rs`; `engine/src/triage/{admission.rs,complete.rs,prompt.rs}`; `tests/services/reliability.rs` (`admission_survives_restart`, `provider_error_redacted`) |
| 18 | `core/src/action/approval.rs:38 enum PlanBinding`; `tests/services/plan_binding.rs` (`approve_with_stale_v1_plan_hash_refused`) |
| 19 | `hub/src/api/readiness.rs` (`/readyz`), `api/system.rs:137-146` (metrics auth), `runtime/backup_job.rs`, `admin/mod.rs:76 restore`, `store/src/backup.rs` (`verify_backup`, `prune_pre_migrate`) |
| 57 | `domovoi-config/src/hub/retention.rs` (`RetentionSection`, `RETENTION_KEYS`); `hub/src/runtime/schedulers.rs` `DailyJob::Retention`; two keys documented as not yet applied by design (`operation_journal_days`, `execution_outputs_days`) |
- **Prompt 17 is reconciled** and should not be reopened: `prompts/17-llm-reliability.md:26-36` is the
  section `## Already implemented by the integrated LLM compatibility work`, and the file's last change
  is `3873249` (#131), the PR that integrated that work. The index's row for 17 (`07:121`, `07:268`)
  already carries the narrowed scope. No other reconciliation record exists in `docs/rework/`.

## 6. Documents and decisions to preserve

Do not regenerate: `08`, `09`, `10`, `11`, `12`, `13`, `14`, `15`, `16`, `17`, `18`, `19`, `57`, and
`24` (written early, wave G, depends on the missing 23 for ordering only — preserve the doc, do not
re-author it).

Constraints already resolved that later prompts must keep honouring:

- The `advisories.kind` CHECK is removed **once**, by 08's `0007_advisories_kind_unchecked.sql`
  (`05`/`07 §4`). Prompt 15 and prompt 19 both rely on it and add no rebuild; no later prompt may
  reintroduce a table rebuild for advisory kinds.
- 14 before 15 where they touch the same engine files (`07 §1`, `§3`).
- 17 reconciled; waves A–D implemented.
- 24 owns the capability contract types and every later prompt consumes them; 23 must not absorb them.

## 7. Stale, partial or inconsistent documents (status-only fixes; no rewrite)

None of these change a decision, a scope or an accepted architecture. They are index/roadmap snapshots
that have moved on:

1. `07:267-277` — §6.1 "Launch state" rows still say 11, 12, 14, 16, 18, 17, 19 are `ready`/`waiting`.
   All seven are merged (#120, #121, #122, #124, #132, #134, #135). Prompt 15's row now reads merged
   (#136) after the correction made with the remaining-work map.
2. `07:178-180` — wave D says "17 and 19 each add a migration". Prompt 19 states **no migration**
   (`prompts/19-ops-readiness.md:248`: Rust-only `AdvisoryKind::BackupFailed`). Prompt 15 states **no
   migration** too (`prompts/15-durable-timers.md:418`, `:611`); that separate claim has been corrected in
   `07` with the remaining-work map. Only 17 added one (`0010_llm_admission.sql`), which is why no `0011`
   exists: `0006_unit_of_work.sql` already created the effect columns and marks the lifecycle as
   implemented later. The 19 claim is repeated at `07:269`.
3. `07:20-24` — "it currently ends at `0009`". The directory ends at `0010_llm_admission.sql`. The
   surrounding text already instructs readers to read the directory, so this is a snapshot, not a rule.
4. `07:7` and `07:255` — "14 of 51 written" / "Written prompts (14)" is still **accurate**; do not
   "fix" it upward. `07:280-286` (29 unwritten implementation), `07:291-301` (4 research) and
   `07:324-334` (4 downstream) are also accurate.
5. `04 §19` (`04:1191`) still lists research as `R-1…R-7`; `99 §C` has since added R-8, R-9, R-10.
   The register is authoritative (`99:3`).

No contradiction was found between the prompts and `ARCHITECTURE.md` that would stop authoring E–M.

## 8. Genuine blockers for later waves (none for wave E)

| Item | Evidence | Blocks |
|---|---|---|
| O-30 open | `99-open-decisions.md:42-46` | 47, and the H1 part of wave L |
| R-1 open | `99-open-decisions.md:54` | 38 (`graph-canvas`), then 52; 33 is the spike that resolves it |
| R-2, R-10 open | `99:55`, `99:59` | 46; 53 and 55 produce the evidence |
| R-3, R-9 open | `99:56`, `99:58` | 47; 54 produces the evidence |
| R-8 open | `99:57` | 45; 56 produces the evidence |

Nothing blocks authoring 20–23, 25, 26–29, 30–37, 39–43, 48–52, 58 as documents; only their launch gates
are sequenced. 53–56 can be authored and launched now. Prompt 15's merge (#136) removed the last
implementation prerequisite in wave D, so 20 is launchable once its brief exists.

## 9. Canonical paths

- Prompt index: `docs/rework/07-implementation-prompt-index.md`
- Shared-contract table and authoring standard: `docs/rework/prompts/README.md` (contracts at lines
  37–57); verbatim blocks: `docs/rework/prompts/_blocks.md`
- Prompt directory and naming: `docs/rework/prompts/NN-branch-name.md` (51 planned IDs, 14 files)
- Roadmap: `docs/rework/06-implementation-roadmap.md`; decisions `03`, target architecture `04`,
  attestation `04b`, migration strategy `05`, decision register `99`
- This report: `docs/rework/phase7-working/01-inventory.md`
