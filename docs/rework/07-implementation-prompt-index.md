# 07 — Implementation prompt index (DRAFT)

**Status: draft. No prompts are generated until the architecture (03/04) is reviewed and approved.**
Each row will become one prompt = one branch = one PR with one Conventional Commit scope. Each prompt
names the `ARCHITECTURE.md` sections it updates (D-15).

## 1. Sequence

| # | Branch | Milestone | Depends on | Parallel with | Scope | ARCHITECTURE.md sections |
|---|---|---|---|---|---|---|
| 08 | `store-unit-of-work` | F1 | — | 13 | port, SQLite + InMemory impl, contract suite, `journal`/`effects` tables, `schema_compat` | §6, §9 |
| 09 | `audit-hash-chain` | F1 | 08 | 13 | chain columns, anchor row, verify command, startup verify, `Actor` enum | §9, §15 |
| 10 | `atomic-action-writes` | F1 | 09 | 11 | action writer via unit of work; audit failure fails transition | §5.6, §12 |
| 11 | `atomic-incident-writes` | F1 | 09 | 10 | incident manager via unit of work; timeline into journal; `version` | §5.3, §9 |
| 12 | `crash-harness` | F1 | 10, 11 | — | `CrashingStore`, restart helper, sweep | §17 |
| 13 | `protocol-tolerance` | F3 | — | 08–12 | major-only reject, features, `Open<T>`, unsupported reply, hub ping, golden frames | §8 |
| 14 | `recovery-order` | F2 | 10 | 15 | availability states, startup order, freshness checks at dispatch | §5.6, §7.1 |
| 15 | `durable-timers` | F2 | 08 | 14 | `Wake` effects, verification resume, triage reload at start | §5.5, §7.1 |
| 16 | `delivery-durability` | F2 | 08 | 14, 15 | spool-before-state, notify-after-delivery, read errors | §7.2, §13 |
| 17 | `llm-reliability` | F4 | 08 | 18, 19 | admission persistence, fail-closed, cache, deadline, prompt cap, attribution, redaction, host-scoped similar | §11 |
| 18 | `structured-approvals` | F4 | — | 17 | web + Discord cards, plan hash on approve | §12, §13, §15 |
| 19 | `ops-readiness` | F4 | 08 | 17 | step-up list, `/readyz`, metrics auth, FULL sync, quick_check, backup verify, restore, pruning | §14, §16 |
| 20 | `agent-action-journal` | M1 | 13, 16 | 21 | journal + epoch, dedupe, `action_status`, agent rate limit | §7.2, §8 |
| 21 | `outcome-unknown-reconcile` | M1 | 14 | 20 | status, reconcile on hello, old-agent fallback | §5.6, §12 |
| 22 | `plan-hash-v2-and-authorization` | M1 | 20, 21 | — | plan hash v2, `Authorization` enum, constructor enforcement (clippy + arch rule) | §12, §15 (INV-1) |
| 23 | `single-execution-boundary` | M1 | 22 | 24 | one execute path + arch rule | §4, §12 |
| 24 | `capability-contracts` | M2 | 13 | 20–23 | core contract types, built-ins, skill adapter + floor | §5.6, §6 |
| 25 | `capability-registry` | M2 | 23, 24 | — | availability, catalogue paging, replace PolicyLink, inventory UI | §6, §8, §12 |
| 26 | `operation-aggregate` | M3 | 23, 25 | — | tables, writer + execution module, decide, effects, budgets, simulator, trace golden fixtures | §5 (new subsection), §7.1 |
| 27 | `investigate-operation` | M3 | 26 | 28 | triage behind flag, parity suite, legacy projection | §5.5, §11 |
| 28 | `journal-sse` | M3 | 26 | 27 | SSE ids/replay/reset, `as_of_seq`, `incident_events` copy | §9, §10 |
| 29 | `use-case-layer` | M3 | 26 | 27, 28 | hub services + module-edge rule | §4, §7.1 |
| 30 | `operations-ui-timeline` | U1 | 27, 28 | 31 | shell, Operations list/detail, step tree, inspector, approval card, Playwright | §10 |
| 31 | `meta-tools-and-validation` | M4 | 27 | 30, 34, 36 | closed meta-tools, validator, target resolution, remove text protocol | §11, §15 |
| 32 | `layered-context` | M4 | 31 | 34, 36 | statuses, caps, shared provider loop | §11 |
| 33 | `graph-spike` | U2 | 30 | 31–36 | library choice under CSP/style/bundle; DESIGN.md amendment (no product code merged) | §10 |
| 34 | `knowledge-store` | M5 | 11 | 31, 36 | facts/relations/entities, rename, backfill | §5 (new), §9 |
| 35 | `experience-search` | M5 | 27, 34 | 32 | resolution summaries, FTS, `search_experience`, similar operations UI | §9, §11 |
| 36 | `operator-conversations` | M6 | 28 | 31, 34 | aggregate, streaming, panel + workspace | §5 (new), §10, §13 |
| 37 | `visibility-profiles` | U3 | 30 | 33–36 | categories, `useVisibility`, preferences, P0 test | §10 |
| 38 | `graph-canvas` | U2 | 33 | 37 | graph view, keyboard model, motion | §10 |
| 39 | `policy-evaluator` | M7 | 24 | 26+ | pure evaluator + 04 §7.7 tests | §15 (new policy subsection) |
| 40 | `policy-shadow` | M7 | 39, 27 | — | versions, shadow decisions, simulator, step-up publish | §9, §15 |
| 41 | `packages-agent` | P1 | 13, 25 | 26+ | package format, agent verify/install/uninstall/rollback | §7.2, §8 |
| 42 | `packages-hub-and-wizard` | P1 | 41, 37 | — | index, relay, jobs, audit, install wizard | §8, §10 |
| 43 | `policy-knowledge-capability-pages` | U4 | 37, 35, 40, 42 | — | split per page when written | §10 |
| 44 | `docker-write-adr` | M8 | — | any | ADR only | §15 (ADR reference) |
| 45 | `policy-permit-autonomy` | M8 | 32, 40, 44 | — | PermitIssuer, ceiling enforcement, guardrails, pause, first L2 contracts | §12, §15 (INV-1 wording) |
| 46 | `containment` | M9 | 45, 41, 34 | — | first set (split per capability when written) | §12, §15 |

Section numbers are `ARCHITECTURE.md` headings at baseline 64117f0 (4 layout, 5 domain, 6 ports, 7 runtime, 8 protocol, 9 persistence, 10 API/web, 11 LLM, 12 skills/actions, 13 chat, 14 config, 15 security invariants, 16 observability, 17 testing); re-check when writing each prompt.

## 2. Integration contracts frozen before parallel work

- 08 freezes `Transaction`, `CommitError`, `CommitReceipt`.
- 13 freezes golden frames incl. `features`; 20/21 add `action_status` frames in their first commit.
- 24 freezes contract type shapes used by 25, 31, 39.
- 26 freezes the trace schema (Rust + `web/src/test/fixtures`) used by 28, 30, 38.
- 31 freezes the context retrieval port used by 34/35 (experience and knowledge implementations).
- 37 freezes visibility categories used by every page prompt after it.

## 3. Prompt template (unchanged)

Objective; relevant RW/D decisions and invariants; exact scope; out of scope (no unrelated
refactoring, report outside-scope bugs in the PR); files to inspect first (verify current paths and
signatures); expected changes; interfaces and contracts; DB changes with N-1 test; error and
recovery behaviour per failure; security requirements; named tests; acceptance criteria;
migration/compatibility; docs incl. ARCHITECTURE.md sections; completion report format.
