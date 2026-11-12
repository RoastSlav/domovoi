# 07 — Implementation prompt index (DRAFT)

**Status: draft. The detailed prompts (08+) will be written after you approve the direction and
answer `99-open-decisions.md`.** Writing them now would bake in decisions you have not made
(RW-02, RW-12, RW-13 especially).

Each prompt = one branch, one PR, one Conventional Commit scope. Every prompt will follow the
template in §3. "∥" = can run in parallel with the listed prompts.

## 1. Sequence

| # | Prompt (branch name) | Milestone | Depends on | ∥ with | Scope summary |
|---|---|---|---|---|---|
| 08 | `store-unit-of-work` | M0 | — | 10, 11, 12 | core port + SQLite/InMemory impl + contract tests; no callers yet |
| 09 | `atomic-action-writes` | M0 | 08 | 13 | action writer + audit in one tx; audit failure fails transition |
| 10 | `atomic-incident-writes` | M0 | 08 | 09 | incident manager batch commit; `version` column |
| 11 | `approval-freshness` | M0 | — | 08 | expiry + incident-open check at dispatch and catch-up; catch-up waits for agent |
| 12 | `structured-approval-cards` | M0 | — | 08 | web + Discord render plan; approve carries plan hash |
| 13 | `settings-step-up` | M0 | — | any | declarative sensitive list incl. `llm.cli.*`; test |
| 14 | `durable-verification-timers` | M0 | 08 | 15 | timers table + scheduler; remove in-memory sleep |
| 15 | `triage-restart-safety` | M0 | 08 | 14 | queue reload at start; persisted admission; fail-closed; per-call deadline; prompt cap; host-scoped similar |
| 16 | `protocol-tolerance` | M0 | — | any | major-only reject, features set, tolerant enums, golden frames |
| 17 | `ops-readiness` | M0 | — | any | `/readyz`, metrics auth, FULL sync, quick_check, `domovoi restore`, delivery-after-send |
| 18 | `crash-harness` | M0 | 08 | — | `CrashingStore`, restart helper, sweep over 09/10/14 scenarios |
| 19 | `agent-action-journal` | M1 | 16 | 20 | agent journal, dedupe, `action_status` responder |
| 20 | `outcome-unknown` | M1 | 09 | 19 | status, hub reconcile, old-agent fallback |
| 21 | `plan-hash-v2` | M1 | 19, 20 | — | script bytes, target, step id; Authorization enum; compile-time seal |
| 22 | `single-execution-boundary` | M1 | 21 | — | one execute path + arch rule |
| 23 | `capability-contracts` | M2 | 16 | 19–22 | core types, built-ins, skill adapter + floor |
| 24 | `capability-registry` | M2 | 22, 23 | — | registry, catalogue feature, replace PolicyLink, inventory UI |
| 25 | `operation-aggregate` | M3 | 08, 22, 24 | — | tables, writer, pure decide, effects, simulator |
| 26 | `investigate-operation` | M3 | 25 | 27 | triage behind flag, parity suite |
| 27 | `sse-v2` | M3 | 25 | 26 | journal ids, replay, topics; web live layer |
| 28 | `operations-ui` | M3/U1 | 26, 27 | — | shell slots, Operations list/detail timeline, Playwright smoke |
| 29 | `meta-tools-and-plan-validation` | M4 | 26 | 31, 33 | closed meta-tools, validator, remove text protocol |
| 30 | `layered-context` | M4 | 29 | 31, 33 | statuses, caps, cycles, shared provider loop |
| 31 | `knowledge-store` | M5 | 08 | 29, 33 | facts/relations, rename metric_samples, discovery backfill |
| 32 | `knowledge-ui` | M5 | 31 | 30 | browser, detail, edit, proposals |
| 33 | `operator-conversations` | M6 | 27 | 29, 31 | aggregate, streaming, panel + workspace |
| 34 | `policy-evaluator` | M7 | 23 | 25+ | pure evaluator + table/property tests |
| 35 | `policy-shadow-and-ui` | M7 | 34, 26 | — | versions, shadow decisions, editor, simulator, step-up |
| 36 | `policy-permit-autonomy` | M8 | 30, 35 | — | permit seal, ceilings, guardrails, pause, first L2 contracts, drills |
| 37+ | UI U2 items, graph spike, Capability Center, search | U2 | per item | — | written after M3 |

## 2. Integration contracts between parallel prompts

- **08 → all writers:** the `commit(Batch)` signature and error type are frozen by 08; 09/10/14/15
  only consume it.
- **19 ∥ 20:** wire shapes of `action_status` request/response frozen in 16's golden frames before
  either starts.
- **26 ∥ 27:** trace schema (operation, step, effect event JSON) frozen in 25 with golden fixtures
  under `crates/domovoi-proto` or `domovoi-core` tests and mirrored in `web/src/test/fixtures`.
- **29/30 ∥ 31 ∥ 33:** only touch their own aggregates; shared surface is the context-builder
  retrieval port defined in 30 (knowledge supplies an implementation in 31 after 30 merges).

## 3. Prompt template (every prompt 08+ will contain)

1. Objective (one paragraph).
2. Relevant decisions (RW ids, repo ADRs, invariants) with links.
3. Exact scope — files/modules expected to change.
4. Out of scope — explicit list; "no unrelated refactoring; report out-of-scope bugs in the PR".
5. Inspect first — files to read before coding; "verify paths, types and signatures; do not assume
   this prompt's paths are current".
6. Expected changes, step by step.
7. Interfaces and contracts (signatures, wire shapes, events), frozen where other prompts depend.
8. Database changes (additive migration, N-1 test).
9. Error and recovery behaviour, stated per failure (store error, link drop, restart mid-step).
10. Security requirements (seals, arch rules, step-up, redaction).
11. Tests required, by name.
12. Acceptance criteria (checkable).
13. Migration/compatibility (old agents, old DB, feature switch).
14. Docs to update (book page, ARCHITECTURE.md only if stated, ADR to add).
15. Completion report format: changed files, criteria met with evidence, tests added, deferred items,
    out-of-scope findings.

Prompts follow `AGENTS.md`: branch named after the change, one-line Conventional Commit subjects,
no mention of prompts or AI in committed files, scoped checks only.
