# 07 — Implementation prompt index and execution guide

Each prompt is one branch and one pull request. The prompt files live in `prompts/NN-branch-name.md`.
They are self-contained: copy one into a local coding model working in this repository.
Shared-contract ownership and the standard blocks are in `prompts/README.md` and `prompts/_blocks.md`.

**Status:** the prompt pack is written. Gated prompts must not be launched until their gate is met.
Readiness per milestone is in `06` (Readiness).

## 1. How to use this guide

1. Launch prompts in the order of §3, one at a time.
2. A prompt is launchable only when every prompt in its **Depends on** column is merged and its
   **Gate** is met.
3. Prompts in the same **wave** that the table marks parallel touch disjoint areas. They may run in
   any order, or at the same time on separate branches.
4. After each merge, run the checks listed in the prompt's completion report on `main`, and update
   `ARCHITECTURE.md` as that prompt instructs (D-15).
5. If a prompt reports a blocker or discrepancy, resolve it before launching anything that depends on
   it.

**Model suitability:**
- **S** (small) = mechanical, fully specified.
- **M** (medium) = a few local judgement calls.
- **L** (large) = cross-cutting or security-critical. Use the strongest local model and review
  carefully.

## 2. Prompt table

| ID | Branch | Milestone | Scope | Depends on | Parallel with | Produces | Gate | Model |
|---|---|---|---|---|---|---|---|---|
| 08 | `store-unit-of-work` | F1 | UnitOfWork port, SQLite + InMemory impl, contract suite, `journal`/`effects`/`schema_compat`, `version` columns, `synchronous=FULL`, `quick_check` | — | 13 | UnitOfWork, tables | — | L |
| 09 | `audit-hash-chain` | F1 | `Actor`, audit anchor + chain, `audit verify`, startup verify | 08 | 13 | Actor, audit chain | — | M |
| 10 | `atomic-action-writes` | F1 | action writer via UnitOfWork; audit failure fails transition; token rotation via writer | 09 | 11, 13 | — | — | M |
| 11 | `atomic-incident-writes` | F1 | incident manager via UnitOfWork; timeline into journal | 09 | 10, 13 | — | — | M |
| 12 | `crash-harness` | F1 | CrashingStore, restart helper, crash sweep, N-1 upgrade fixture | 10, 11 | 13 | testkit harness | — | M |
| 13 | `exact-protocol-rule` | F3 | exact major.minor, check after auth, durable Incompatible, agent idle state, floors removed; no wire change | — | 08–12 | Incompatible state | — | M |
| 14 | `recovery-order` | F2 | HostAvailability, startup order, dispatch freshness checks | 10 | 16 | HostAvailability | — | M |
| 15 | `durable-timers` | F2 | `Wake` effects, verification resume, effect lifecycle with lease fencing, triage reload | 08, 10, 14 | 16 | effect lifecycle | — | L |
| 16 | `delivery-durability` | F2 | spool-before-state, notify-after-delivery, read errors | 08 | 14, 15 | — | — | M |
| 17 | `llm-reliability` | F4 | admission persistence, deadlines, prompt cap, attribution, redaction, host-scoped similar | 08 | 18, 19, 57 | — | — | M |
| 18 | `structured-approvals` | F4 | structured cards (web, Discord); approve binds the **v1** hash | 10 | 17, 19, 57 | — | — | M |
| 19 | `ops-readiness` | F4 | step-up list, `/readyz`, metrics auth, backup verify, restore, pre-migrate pruning | 08 | 17, 18, 57 | — | — | M |
| 57 | `configurable-retention` | F4 | D-21 retention config and pruning for existing data | 08 | 17, 18, 19 | retention config | — | S |
| 20 | `agent-action-journal` | M1 | **protocol 1.1 → 1.2** (complete v1 wire), agent journal, `action_status`, mutation limiter | 13, 14, 15, 16, 17, 18, 19 | 21 | wire 1.2 | — | L |
| 21 | `outcome-unknown-reconcile` | M1 | OutcomeUnknown, reconcile on hello | 14, 20 | — | — | — | M |
| 22 | `plan-hash-v2-and-authorization` | M1 | plan hash v2, approval binding columns, `Authorization`, `PolicyPermit` type, CI enforcement | 18, 20, 21 | — | ExecutionId, Authorization | — | L |
| 23 | `single-execution-boundary` | M1 | one execute path, one read boundary, arch rules | 22 | — | — | — | M |
| 24 | `capability-contracts` | M2 | contract types, built-ins, skill adapter + floor | 23 | 53–56 | contract types | — | M |
| 25 | `capability-registry` | M2 | availability, catalogue, replace PolicyLink, target identity, inventory UI | 23, 24 | 41 | registry | — | L |
| 26 | `operation-aggregate` | M3 | operations/steps/edges/executions, writer, decide, effect runner, simulator, trace schema | 23, 25 | 39 | Operation model, trace schema | — | L |
| 27 | `investigate-operation` | M3 | triage as Investigate operation (flag), parity suite, legacy projection, step approvals | 26 | 28, 29 | — | — | L |
| 28 | `journal-sse` | M3 | SSE ids/replay/reset, `as_of_seq`, `incident_events` copy | 26 | 27, 29 | SSE contract | — | M |
| 29 | `use-case-layer` | M3 | hub application services, module-edge rule | 26 | 27, 28 | use-case services | — | M |
| 58 | `remove-legacy-triage-path` | M3 | remove triage queue, recheck, flip flag default, legacy expiry | 27, 28 | 30 | — | parity suite green | M |
| 30 | `operations-ui-timeline` | U1 | shell, Operations list/detail, step tree, inspector, approval card, banners, Playwright smoke | 27, 28 | 31, 34, 36, 58 | approval card component | — | M |
| 31 | `meta-tools-and-validation` | M4 | closed meta-tools, plan validator, target resolution, remove text protocol | 27 | 30, 34, 36 | ContextSource port | — | L |
| 32 | `layered-context` | M4 | layered context, statuses, caps, one provider loop | 31 | 34, 36 | — | — | M |
| 34 | `knowledge-store` | M5 | facts/relations/entities, rename, backfill, management-path facts, knowledge API | 11, 27 | 31, 32, 36 | knowledge API | — | M |
| 35 | `experience-search` | M5 | resolution summary fields, FTS, `search_experience`, similar operations | 27, 31, 34 | 32 | — | — | M |
| 36 | `operator-conversations` | M6 | conversation aggregate, streaming, panel + workspace | 28 | 31, 32, 34 | — | — | M |
| 37 | `visibility-profiles` | U3 | visibility categories, profiles, preferences endpoint | 30 | 31–36 | visibility categories | — | M |
| 39 | `policy-evaluator` | M7 | pure evaluator incl. locked Deny, hard limits, publication helpers | 24 | 26–37 | evaluator API | — | L |
| 40 | `policy-shadow` | M7 | policy versions, shadow decisions, simulator, publication report, step-up | 27, 39 | 35, 36 | — | — | M |
| 41 | `packages-agent` | P1 | agent package verification/install/uninstall over `transfer_*` | 20, 25 | 26–40 | — | — | L |
| 42 | `packages-hub` | P1 | index, owner import, store, relay, jobs, audit | 41 | 26–40 | — | — | M |
| 33 | `graph-spike` | U2 | R-1 spike: library choice, evidence, DESIGN.md amendment proposal | 30 | 31–42 | R-1 evidence | — | M |
| 38 | `graph-canvas` | U2 | interactive graph, keyboard model, motion | 33, 37 | 43, 48–51 | — | R-1 accepted | M |
| 52 | `graph-playback` | U2 | historical playback (last in U2) | 38 | 43, 48–51 | — | — | M |
| 43 | `policy-pages` | U4 | policy editor, explanation, simulator UI, publication report | 37, 40 | 48–51 | — | — | M |
| 48 | `knowledge-pages` | U4 | knowledge browser and detail, proposals, management paths view | 37, 34, 35 | 43, 49–51 | — | — | M |
| 49 | `capability-center-and-install-wizard` | U4 | inventory, owner import, install wizard, ceiling and helper views, key-enrollment screen | 37, 25, 42 | 43, 48, 50, 51 | — | — | M |
| 50 | `audit-and-retention-pages` | U4 | audit page (legacy and chained), retention settings | 37, 09, 57 | 43, 48, 49, 51 | — | — | S |
| 51 | `control-room-attention-and-search` | U4 | Control Room, attention centre, global search | 37, 28, 30 | 43, 48–50 | — | — | M |
| 53 | `spike-nft-containment` | R-2 | evidence on supported Linux platforms | — | any | R-2 evidence | Linux VMs available | M |
| 54 | `spike-helper-and-skill-runner-platforms` | R-3, R-9 | evidence on socket activation, sandbox and filesystem view | — | any | R-3, R-9 evidence | Linux VMs available | M |
| 55 | `spike-safety-attestation` | R-10 | scratch implementation reproducing `04b` vectors, parser, route state, locking, adversarial tests | — | any | R-10 evidence | — | L |
| 56 | `spike-systemd-unit-identity` | R-8 | evidence on effective unit identity | — | any | R-8 evidence | Linux VMs available | S |
| 44 | `helper-adrs` | H1 | ADRs for the three identities and for helper attestation | — | any | ADRs | 54 and 55 evidence (or unproven items recorded) | M |
| 47 | `three-identities-helper-and-skill-runner` (may be split 47a/b/c) | H1 | helper, attestation, skill runner, installer migration, hub enrollment | 20, 22, 44 | — | helper protocol, SafetyIdentity | **O-30 answered; R-3, R-9, R-10 accepted** | L |
| 45 | `policy-permit-autonomy` | M8 | PermitIssuer, ceiling, guardrails, `unit.restart`, `container.restart` | 32, 40, 47 | — | — | **R-8 accepted; H1 complete** | L |
| 46 | `containment` | M9 | `firewall.block_source` with kernel timeout, verified and unverified paths, drills | 45, 34, 47 | — | — | **R-2, R-10 accepted** | L |

## 3. Recommended launch order

Waves run top to bottom. Within a wave, items joined with ∥ can run in parallel.

1. **Wave A, foundation:** 08 ∥ 13. Research spikes 53 ∥ 54 ∥ 55 ∥ 56 may start here and run
   throughout; they change no product code.
2. **Wave B:** 09.
3. **Wave C:** 10 ∥ 11.
4. **Wave D:** 12 ∥ 14 ∥ 16 ∥ 17 ∥ 18 ∥ 19 ∥ 57. Then 15, after 14: both edit `engine/src/actions/{executor,writer,mod}.rs`.
5. **Wave E, protocol bump:** 20. This is a coordinated hub and agent upgrade.
6. **Wave F:** 21, then 22, then 23.
7. **Wave G:** 24, then 25 ∥ 39.
8. **Wave H:** 26. After it: 27 ∥ 28 ∥ 29.
9. **Wave I:** 30 ∥ 31 ∥ 34 ∥ 36 ∥ 41 ∥ 40. 40 needs 27 and 39; 41 needs 20 and 25. 58 once the
   parity suite is green.
10. **Wave J:** 32 ∥ 35 ∥ 37 ∥ 42 ∥ 33.
11. **Wave K, UI pages:** 43 ∥ 48 ∥ 49 ∥ 50 ∥ 51. Then 38, then 52.
12. **Wave L, privilege boundary:** 44, once the 54 and 55 evidence exists; then 47 (or 47a, 47b,
    47c in order), once O-30 is answered and R-3, R-9 and R-10 are accepted.
13. **Wave M, autonomy:** 45, once R-8 is accepted; then 46, once R-2 and R-10 are accepted.

## 4. Integration contracts that must exist before dependants start

| Contract | Owner | First consumers |
|---|---|---|
| `Transaction`, `CommitError`, `CommitReceipt`; `advisories.kind` CHECK removed (new advisory kinds are Rust-only) | 08 | 09, 10, 11, 15, 16, 19, 26 |
| effect lifecycle and lease fencing | 15 | 16, 26 |
| wire 1.2 (complete RW-42 list, frozen golden frames) | 20 | 21, 22, 25, 41, 47 |
| `ExecutionId`, `Authorization`, plan hash v2 | 22 | 23, 26, 45 |
| capability contract types | 24 | 25, 31, 39 |
| trace schema (Rust and web fixtures) | 26 | 28, 30, 38, 52 |
| SSE contract | 28 | 30, 36, 51 |
| ContextSource port | 31 | 32, 35 |
| visibility categories | 37 | 38, 43, 48–51 |
| evaluator API | 39 | 40, 43, 45 |
| helper protocol and SafetyIdentity | 47 | 45, 46, 48, 49 |

A wire change after prompt 20 has merged is a coordinated protocol decision (RW-42). It is never a
silent edit by a later prompt.

## 5. Milestone coverage

Every milestone has a prompt, and the research gates have spikes:

| Milestone | Prompts |
|---|---|
| F1 | 08–12 |
| F2 | 14–16 |
| F3 | 13 |
| F4 | 17–19, 57 |
| M1 | 20–23 |
| M2 | 24–25 |
| M3 | 26–29, 58 |
| M4 | 31–32 |
| M5 | 34–35 |
| M6 | 36 |
| M7 | 39–40 |
| P1 | 41–42 |
| H1 | 44, 47 |
| M8 | 45 |
| M9 | 46 |
| U1 | 30 |
| U2 | 33, 38, 52 |
| U3 | 37 |
| U4 | 43, 48–51 |
| Research | 53 (R-2), 54 (R-3, R-9), 55 (R-10), 56 (R-8) |

The remaining research items need no prompt of their own:
- R-1 is prompt 33.
- R-4 is a step in 16.
- R-5 is a step in 17.
- R-6 is a measurement in 08 and 20.
- R-7 became a test in 25.

Deferred items (`06`, "Deferred and after-v1") get no prompts.
