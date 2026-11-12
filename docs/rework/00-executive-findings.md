# 00 — Executive findings

Baseline `64117f0`. `docs/design-pack/` is the **product-intent** reference; `docs/rework/` is the
validated **technical direction** (decisions in `03`, contracts in `04`). Owner decisions are D-01…D-15
in `99`; open items O-16…O-24.

Files: `01` repository reality (facts) · `02` pack validation · `03` decisions RW-01…32 · `04` target
architecture · `05` migration · `06` roadmap · `07` prompt index (draft) · `99` decisions and open items ·
`investigations/` evidence snapshots.

## Direction

Evolve the existing workspace (RW-01). Foundation first, inside the rework (no v0.1 release, D-01):
atomic writes with an audit chain (F1), restart-safe recovery and approval freshness (F2), protocol
tolerance (F3), LLM and ops reliability (F4). Then execution journal and reconciliation (M1),
hub-owned capability contracts (M2), the Operation aggregate with a journal-backed live stream (M3),
dynamic investigation (M4), knowledge and experience search (M5), the Operator (M6), policy in
shadow (M7), L2 autonomy (M8), L3 containment (M9), signed packages with UI install (P1), and the UI
track (timeline, graph, profiles, policy/knowledge/capability pages).

## Key architecture choices

- **One owner per aggregate.** Execution is a typed Step of the Operation, written only by the
  operation writer (ADR 0019's action writer becomes its execution module). One dispatch attempt per
  step; retries are new steps.
- **One journal.** Typed operational events for all aggregates, global sequence assigned inside the
  single-writer transaction; it is experience, the change stream and the SSE replay source. Audit is a
  separate hash-chained security record in the same transaction; `effects` is the work outbox.
- **Policy.** Pure evaluator; scope is a partial order over host × capability × target; the most
  specific matching rule wins even over a broader Deny (D-05); ties → Deny; incomparable conflicts →
  Deny > RequireApproval > Allow (O-19). Hard limits (host ceiling, Destructive, containment
  exclusions, pinned targets, unknown safety facts, pause) are applied after rules and cannot be
  overridden.
- **Authorization in Rust.** `HumanApproval` and `PolicyPermit` live in `domovoi-core` (the engine
  must use them and cannot depend on the hub). Construction sites are restricted by path-resolved
  clippy `disallowed-methods`, an arch rule on the `expect` attribute, and the existing text scan —
  CI controls, not runtime security. Earlier "compile-time seal" claim corrected.
- **Host ceiling.** The agent enforces an owner-edited `[unattended]` list (capability, targets, rate,
  constrained args, TTL/active caps) for policy-authorized runs and rate limits all mutations. It
  bounds a buggy or misconfigured hub; it does not stop a compromised hub labelling a run "human".
  Signed approvals/bundles are post-v1.
- **Recovery.** Agent journal with an epoch and fsync-before-start makes `not_found` mean "never
  started"; otherwise `OutcomeUnknown`. Hosts are `Unknown` until they connect; nothing is rejected
  for that reason.
- **Packages.** Signed archives; the agent verifies against locally trusted keys and only installs if
  the owner enabled installs on that host; the hub relays bytes; install never enables or authorizes.

## Current-product findings that drive the foundation (detail in `01` §11)

H: approval cards show model prose without target/args; LLM CLI command changeable without step-up;
approved actions run after restart with no freshness check; catch-up runs before agents reconnect
(static reading, not reproduced); audit writes outside the state transaction; triage queue and
verification lost on restart; no outcome-unknown or agent deduplication. M: spool/notification state
saved before success; admission in memory and fail-open; no model attribution or per-call deadline;
`/healthz` constant; no restore command.

## What changed in this revision

All owner decisions applied; RW-11 rewritten for real Rust visibility; RW-15/RW-18 merged into one
sequence model; Step/Execution ownership made unambiguous; policy formalised with a decision table;
containment, packages, audit chain, retention, actor identity, startup order, schema rollback and
legacy projection got their own decisions; roadmap split into foundation slices; design pack,
lessons document and `web/DESIGN.md` reconciled.

## What is now safe to turn into implementation prompts, and what is not yet safe

**Safe now** (decided by the owner and fully specified, with no open item blocking):
- F1: unit of work, audit chain, atomic action and incident writes, crash harness. Retention numbers
  only affect config defaults.
- F2: recovery order, approval freshness, durable timers, delivery durability.
- F4: LLM reliability, structured approvals, ops readiness.
- M1: agent journal, reconciliation, plan hash v2, `Authorization` enum (human only), constructor
  enforcement, single execution boundary.
- M2: capability contracts and registry.
- M3: Operation aggregate, Investigate parity, journal SSE, use-case layer; also U1 (timeline UI).
- M4: dynamic investigation.
- M5: knowledge and experience search.
- M6: Operator.
- M7: policy evaluator and shadow mode. The incomparable-rule fallback must be configurable until
  O-19 is answered, or written with the recommended default and flagged.
- U3: visibility infrastructure and profiles.

**Not yet safe:**
- **F3, the protocol support window:** needs O-20. The tolerance mechanics themselves are safe.
- **P1, packages:** needs O-16 (trust scope).
- **M8, L2 autonomy:** needs O-18 (ceiling scope), O-23 (whether the owner can approve a denied step)
  and O-24 (Docker write access via ADR).
- **M9, containment:** needs O-17 (first set) and O-22 (watchdog), and depends on M8.
- **U2, graph canvas:** needs the library spike (prompt 33) first.
- **U4 page prompts:** need P1/M7/M5 outcomes.
