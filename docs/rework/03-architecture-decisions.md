# 03 — Architecture decision records (rework)

Numbered `RW-NN` so they do not collide with `docs/adr/00NN`. Each one becomes a real ADR in
`docs/adr` when its milestone starts (repo rule: a rule that blocks a design gets an ADR).

Status values: **Proposed** (my recommendation, needs your approval), **Needs input** (you must
choose), **Settled** (already agreed in the pack and confirmed by evidence).

Evidence references point to `investigations/*.md` (letter = report) and `01-repository-reality.md`.

---

## RW-01 Evolve the existing workspace; no greenfield rewrite — Proposed

- **Context.** ~56k LOC, clean crate graph (A §1), strong safety core: single-writer actions
  (ADR 0019), `HumanApproval` sealing, plan hash, agent revalidation, argv-only runner, ~900 tests,
  5 gate files (F §1).
- **Problem.** The target needs new aggregates (operation, policy, knowledge) and protocol changes.
  Rewrite or evolve?
- **Alternatives.** (a) Greenfield v2 beside v1. (b) Evolve crate by crate behind the existing ports.
- **Decision.** (b). New concepts are added as new modules/aggregates, existing paths are wrapped
  (strangler), and an old mechanism is removed only after its replacement passes the same gate tests.
- **Rationale.** Nothing in the evidence shows the foundations are wrong; the gaps are missing
  aggregates and missing durability, not a bad structure. A rewrite would throw away the gate
  tests that encode the hard lessons.
- **Consequences.** Some legacy names survive for a while (`ActionRequest`, `ReadTool`). Each
  milestone must say what it deprecates.

## RW-02 Stabilise and release v0.1 before the rework starts — Needs input

- **Context.** Pack doc 10 says v0.1.0 should ship the current product. The investigations found
  release-relevant defects in the current product (01 §11): stale approvals re-run after restart,
  approval cards that do not show what will run, a password-free path to arbitrary hub execution
  through the CLI LLM provider settings, audit writes outside the state transaction, verification
  lost on restart, a protocol minor-version rule that strands agents.
- **Alternatives.** (a) Fix these as milestone M0 and release v0.1, then rework. (b) Fold the fixes
  into the rework and release later. (c) Release now, fix later.
- **Recommendation.** (a). Most M0 fixes are also prerequisites of the rework (transactions,
  outcome-unknown, protocol tolerance), so nothing is wasted.
- **Your call:** whether v0.1 must ship before rework work starts, or the two run in parallel.

## RW-03 One transactional unit of work for state + journal + audit — Proposed

- **Context.** Today the action writer commits the status change and then appends audit in a
  separate statement whose failure is only logged; the incident manager writes signal, incident
  and timeline one at a time (C, A §3). The loop guard is rebuilt from audit rows that may be
  missing.
- **Problem.** The target makes the journal the source of truth for orchestration, UI live
  updates, experience and audit. Every one of those breaks if a state change can commit without
  its event.
- **Alternatives.** (a) Keep separate writes, add repair jobs. (b) Event sourcing (state derived
  only from events). (c) Unit of work: a store port method that takes a state change plus the
  events and audit rows it implies and commits them in one SQLite transaction.
- **Decision.** (c). A `domovoi-core` port (e.g. `Store::commit(Batch)`) implemented by
  `domovoi-store` and `InMemoryStore`; writers build the batch, the store commits atomically.
  Audit failure fails the transition. Not event sourcing: current-state rows remain the read model.
- **Rationale.** Atomicity is the cheapest way to make "notifications and UI derive from committed
  state" true (pack 02, lesson 6). Event sourcing would add replay/versioning cost for no extra
  guarantee at this scale.
- **Consequences.** Ports change (needs a repo ADR: it touches R1/R2 only in that the port is new;
  no new crate edge). Every writer migrates. `synchronous=FULL` (RW-16) makes commits durable.

## RW-04 The Operation is the durable orchestration aggregate — Proposed

- **Context.** Today there is no entity for "a remediation as a whole": incident lifecycle and
  action lifecycle are separate machines, triage state is stringly-typed timeline rows, the triage
  queue and verification timers live in memory (A §4–5). Pack D-24 says Operation is first class
  and distinct from Incident.
- **Decision.** Add an `Operation` aggregate with its own single writer (copy ADR 0019) and a pure
  transition function in `domovoi-engine`:
  `decide(&OperationState, OperationEvent, now) -> (OperationState, Vec<Effect>)`.
  - Origin: `Incident(id) | User(conversation, principal) | Schedule(id) | Operation(parent)`.
  - An incident may have many operations; an operation references 0..n incidents and 1..n targets.
  - Existing triage becomes the first operation kind (`Investigate`), behaviour-preserving.
  - Effects (call model, collect evidence, evaluate policy, dispatch step, schedule timer, notify)
    are persisted as pending rows in the same transaction as the transition (outbox), then
    performed by an effect runner that reports results back as events. On restart the runner
    re-reads pending effects.
- **Rationale.** This is the incident-manager pattern the repo already trusts, applied to work
  instead of alerts. It makes resume, budgets and the UI trace fall out of one log.
- **Consequences.** `incident_events` triage kinds are superseded by operation events; the triage
  queue and `recheck.rs` sleep are removed once the operation path is live.

## RW-05 Operation state is coarse; phases are step labels, not control states — Proposed

- **Context.** Pack 04 defines eight phases and lets paths loop/branch, and also says phases are
  "not a rigid workflow". Encoding phases as the state machine contradicts dynamic investigation;
  encoding nothing makes resume ambiguous.
- **Decision.** Two levels.
  - `OperationStatus`: `Queued, Running, WaitingApproval, WaitingUser, WaitingAgent, WaitingTimer,
    Paused{reason: Budget|Pause|Error}, Succeeded, Failed, Escalated, Cancelled`. Terminal states
    are final; a new cycle on the same incident is a new operation linked to the old one.
  - `Step` (append-only list, each with id, `kind`, `phase` label, status, inputs/outputs refs,
    correlation id, parent step): kinds `ReasoningTurn, EvidenceRequest, Assessment, Plan,
    PolicyDecision, Approval, Execution, Verification, Notification, Question, Note`.
    Step status: `Pending, Running, Succeeded, Failed, Refused, NotRun, Unavailable,
    OutcomeUnknown, Skipped`. (This is the vocabulary pack D-20 asks for, made exhaustive.)
  - `cycle` is a counter on the operation, incremented when a failed verification sends it back
    to reasoning. Budgets (turns, tool calls, evidence requests, executions, cycles, wall time,
    tokens) are persisted on the operation and shared across cycles.
- **Rationale.** The orchestrator stays deterministic (it decides on events, not on model text);
  the model chooses what to do next inside the bounds; the graph UI renders steps and parent links.
- **Consequences.** The pack's phase table becomes documentation of step kinds; the UI graph is
  derived from steps (RW-21).

## RW-06 The model never causes an effect directly; every model turn is checkpointed — Proposed

- **Context.** Lesson #110: cached triage proposals had to be re-validated into actions. Today
  follow-up proposals use a `PROPOSALS:` text protocol (A §6).
- **Decision.** A reasoning turn's output is a typed, schema-validated message (meta-tool calls,
  RW-07). Deterministic code turns it into `EvidenceRequest`/`Plan` steps after validating
  capability id, version, target scope, args and budget; anything invalid becomes a `Refused` step
  with the reason fed back to the model. Each turn's inputs (context refs) and outputs are persisted
  before any effect. Resume after restart re-issues the last unanswered model call (reads only,
  cost bounded by budget). Plan steps are re-validated again at dispatch time, not just at proposal.
- **Consequences.** The text protocol and the cache-of-proposals path are removed.

## RW-07 Capability contracts are hub-owned and typed; agents advertise availability only — Proposed

- **Context.** Pack 03 puts effect, risk, scope and constraints on a descriptor registered by the
  provider. Today the hub already takes `risk` straight from the agent's skill summary, and a skill
  declared `read` runs unattended when the model asks (D §4, E §2; verified
  `domovoi-engine/src/triage/tools.rs:149-158`, `domovoi-agent/src/requests/skills.rs:140`).
  `Hello.capabilities` is sent empty and never read. `ReadTool` is a second compile-time catalogue
  wired in five places.
- **Problem.** If the provider defines its own risk and scope, a buggy or compromised provider
  defines its own authority.
- **Alternatives.** (a) Provider-declared descriptors (pack). (b) Hub-owned contracts; providers
  declare only that they implement contract `id@version`, with an implementation hash and local
  availability. (c) Closed compile-time set only.
- **Decision.** (b), with skills as the one data-defined family:
  - Built-in contracts (today's ReadTool entries, future container/unit operations) are typed in
    `domovoi-core` with id, version, input schema, effect class (`Read, Mutate, Contain,
    Destructive`), reversibility, idempotency, timeout, output limit, verification hint.
  - Skills map to `skill.<name>@<manifest-hash>`. Their effect comes from the owner-authored local
    manifest (that is the owner's trust boundary, unchanged), but the hub applies a **floor**: the
    owner can raise a skill's class in the hub, never lower it below the manifest; unknown or
    unparseable classes are treated as `Destructive`.
  - Agents advertise `(contract id, versions, impl hash, availability, local ceiling)`.
  - The model sees a **closed meta-tool surface**: `list_capabilities(scope)`,
    `describe_capability(id)`, `request_evidence(id, target, args)`, `propose_plan(steps)`,
    `ask_user(question)`, `finish(assessment)`. The set of meta-tools is a closed enum, so INV-1
    and `model_sees_only_read_tools` keep holding: no meta-tool approves or executes.
- **Rationale.** Keeps "available ≠ authorized" structural. New built-in contracts still need
  code review, which is right for anything that mutates.
- **Consequences.** Third-party/integration providers (router APIs etc.) are hub-side adapters
  implementing hub-owned contracts with hub-held credentials; no agent-pushed contracts.

## RW-08 One execution boundary for every consumer, bound to an immutable plan — Proposed

- **Decision.**
  - A single `execute(AuthorizedStep)` path in the engine, used by operations, the Operator, and
    any manual "run" in the UI. Nothing else may call `AgentLink::run_skill`/`run_capability`
    (arch-check rule like INV-1).
  - `AuthorizedStep` can only be built from a `Step` plus an `Authorization`:
    `Human(HumanApproval) | Policy(PolicyPermit)`. Reads within budget use a third variant
    `ReadBudget` minted by the engine for `Read`-class contracts only.
  - **Plan hash v2** covers: contract id+version, implementation hash (for skills: manifest **and
    entry-script bytes**, E §5), target, canonical args, operation id, step id. The web approve
    request carries the hash the user saw; mismatch → refuse.
  - Every authorization has an expiry; dispatch re-checks expiry, that the operation is still
    `Running`, the incident (if any) still open, and preconditions. Restart catch-up obeys the
    same checks (fixes the stale-approval re-run, E risk 6).
- **Consequences.** `ActionRequest` becomes the persisted form of an `Execution` step; existing
  rows migrate as legacy steps.

## RW-09 Outcome-unknown, idempotency keys and an agent action journal — Proposed

- **Context.** A dropped link fails the action while the agent keeps running it; the agent does
  not dedupe by action id; there is no `OutcomeUnknown` (D §2, F). This is the precondition for
  resume, retries and any autonomy.
- **Decision.**
  - Step id is the idempotency key. The agent keeps a small on-disk journal
    (`received → started → finished(result)`), refuses a second start of the same id, and returns
    the recorded result instead.
  - New protocol request `action_status(ids)`; on reconnect/restart the hub asks about every
    step it holds in `Dispatched`. Answers: `finished(result)`, `running`, `unknown` (agent has no
    record → it never started, safe to mark `NotRun`), `journal_lost` → `OutcomeUnknown`.
  - `OutcomeUnknown` is terminal for that step, blocks automatic re-execution of that contract on
    that target until a human resolves it or a verification step proves state.
  - Old agents (no journal feature) keep today's behaviour: dispatched-at-restart → `OutcomeUnknown`
    (not `Failed`).

## RW-10 Deterministic policy evaluator — Proposed (precedence details Needs input)

- **Decision.** Pure function in `domovoi-engine::policy` (no I/O):
  `evaluate(&PolicySet, &Request, &Facts) -> Decision { outcome, matched_rules, constraints, trace }`.
  - Rule: `scope` (all hosts | host ids | host tags), `capability` (id | effect class |
    category), `constraints` (target patterns, max rate, time window, requires_reversible, TTL),
    `outcome` (`Deny | RequireApproval | Allow`).
  - Precedence: any matching `Deny` → Deny. Else the most specific matching rule decides
    (capability id > category > effect class; host id > host tag > all). Several equally specific
    `Allow` rules combine by **intersection** of constraints. No match → default table:
    `Read → Allow (budgeted)`, `Mutate → RequireApproval`, `Contain → RequireApproval`,
    `Destructive → RequireApproval + typed confirmation` and **cannot** be `Allow` in v1 (enforced
    in the evaluator, not just the UI).
  - Policy set is versioned; each decision stores the version and trace. The simulator calls the
    same function (pack D-28).
  - Missing/stale safety-critical facts → the rule's conditions evaluate to "unknown" → never Allow.
- **Needs input:** whether host-tag scope is needed in v1 or host ids suffice; whether `Contain`
  exists at all in v1 (see RW-13).

## RW-11 `PolicyPermit` beside `HumanApproval`, both compile-time sealed — Proposed

- **Context.** INV-1 is enforced by a text scan for `HumanApproval::new(` (E: evadable by a
  renamed import or macro).
- **Decision.** Both types take a zero-sized seal token whose constructor is private to one
  module: `HumanApproval` → `domovoi-hub/src/auth/`, `PolicyPermit` → `domovoi-hub/src/policy/`.
  Keep the text scan as a second line. `PolicyPermit` is minted only from an evaluator `Allow`
  decision plus the agent ceiling check (RW-12) plus the circuit breaker (RW-23). It records rule
  ids, policy version, plan hash v2, expiry. No `Deserialize` on either type.
- **Consequences.** INV-1 wording changes from "only a human approves" to "only a human or an
  owner-authored policy rule authorizes, never the model". The gate test
  `no_approval_without_human` stays; a new gate `no_permit_without_policy_allow` is added.

## RW-12 Autonomy needs a host-local ceiling the hub cannot raise — Needs input

- **Context.** `RunSkill` carries no authorization proof; a compromised hub or anyone who can
  write `hub.db` can run any local write skill (E §2). Adding autonomy on the current wire format
  makes that silent. Pack D-12 wants local agent enforcement; D-27 wants UI-managed config.
- **Alternatives.**
  - (a) Agent config `[policy] unattended = ["skill.restart-plex", "container.restart"]`: the
    agent refuses any `Policy`-authorized dispatch for a contract not listed. Set on the host,
    shown read-only in the UI. Simple, strong, a little less convenient.
  - (b) Owner-signed policy bundles: the owner holds a signing key (passkey/WebAuthn or a key on
    their device), the hub ships signed rules, the agent verifies. Fully UI-managed, much more
    machinery (key custody, rotation, recovery).
  - (c) Trust the hub.
- **Recommendation.** (a) for v1, (b) later if users want it. (c) rejected.
- **Also:** the dispatch frame carries `authorization_kind` so the agent can apply different
  ceilings to human-approved and policy-authorized work.

## RW-13 Defer offline emergency authorization and L3 containment past v1.0 — Needs input

- **Context.** Offline permits need monotonic budgets, persisted nonces, revocation, hash-chained
  local logs and hard-coded management-path exclusions (E §5.6). Firewall/quarantine actions can
  cut the agent's own control link (D §9). The pack itself makes L3 conditional for v1.
- **Recommendation.** v1.0 ships L0–L2 only. The policy model keeps the `Contain` class so L3 can
  be added without reshaping, but no `Contain` capability ships and offline permits are not built.
- **Your call:** accept the deferral, or keep "one narrow L3 capability in v1" (cost: roughly two
  more milestones and a drill suite).

## RW-14 Knowledge starts as typed facts and relations over existing ids — Proposed

- **Context.** Pack 07 proposes `entities`, `relationships`, `knowledge_facts`. Baselines already
  model containers/units/ports; `digest_facts` is a metric sample table whose name will clash (C).
- **Decision.**
  - `subject` is a typed reference string, not a row in a generic entities table:
    `host:<id>`, `container:<host>/<name>`, `unit:<host>/<name>`, `entity:<uuid>` (user-defined
    things such as "VPN", "NAS pool"). A small `entities` table exists only for `entity:` subjects.
  - `facts(id, subject, predicate, value_json, source{user,discovered,integration,inferred,imported},
    status{current,stale,disputed,superseded,retracted}, observed_at, valid_until,
    last_verified_at, supersedes, evidence_ref, created_by, created_at)`. Predicates are a typed
    enum in `domovoi-core` with a value schema each; no free-text predicates in v1.
  - `relations(id, from, kind, to, source, status, observed_at)` with a closed `kind` enum.
  - No numeric confidence. `source` + `status` is the provenance.
  - Rename `digest_facts` → `metric_samples` first.
  - Retraction removes the row from FTS and context retrieval; backups are documented as an
    exception (C: pack ignored nightly/pre-migrate backups).
- **Rationale.** Smallest model that gives provenance and supersession without a second
  inventory that drifts from baselines.

## RW-15 Experience is the journal; no third log — Proposed

- **Decision.** Experience = operation steps/events + incident timeline (typed kinds, no free-text
  kinds) + resolution summaries indexed in FTS. `audit_log` stays the security record (who
  authorized what), written in the same transaction (RW-03), gains denied/refused attempts and
  approval context (IP/session), and becomes hash-chained. No separate `experience_events` table.
- **Consequences.** Incident timeline retention (90 days today, contradicting ARCHITECTURE.md
  "incidents kept") must be decided (open decision O-9).

## RW-16 Stay on one SQLite file; no vector, graph DB, broker or workflow engine — Settled

- Confirmed by C: target scale ~10^4 graph nodes, <1M rows/year. Add: `synchronous=FULL` once
  operation state is persisted (outcome correctness beats a few ms per commit), startup
  `quick_check`, backup verification and a restore command.

## RW-17 Protocol evolves additively with feature flags — Proposed

- **Decision.**
  - Fix the version rule: reject only on major mismatch; a newer agent with the same major is
    accepted and told which features the hub supports (today a newer minor is refused and the
    agent stops retrying for good, D §1).
  - `hello` carries a `features` set (`journal`, `run_capability`, `catalog_v1`, ...). The hub only
    uses a feature both sides list.
  - Tolerant decoding: unknown enum variants decode to `Unknown(String)` instead of failing the
    frame (D §8).
  - Catalogue by hash + paged `get_catalog` (1 MiB frame cap).
  - New: `run_capability{step_id, contract, version, target, args, plan_hash, authorization_kind}`,
    `action_status`.
  - Golden frame fixtures per release in CI.
- Old agents: a hub-side adapter presents their skills/ReadTools as capabilities. Policy-authorized
  execution and `OutcomeUnknown` reconciliation require the new features; old agents stay
  human-approval-only.

## RW-18 Live updates come from the journal sequence — Proposed

- SSE v2: each event carries the journal sequence as `id`; clients reconnect with `Last-Event-ID`
  and receive the gap (bounded; beyond it they get `reset` and refetch). Topics: incidents,
  operations (per id), hosts, approvals. Today's stream has no ids/replay and drops events for slow
  clients silently (B §2).

## RW-19 A use-case layer in the hub shared by web, chat and Operator — Proposed

- Hub transports (HTTP handlers, Discord interactions, Operator) call application services
  (`approve_step`, `start_operation`, `cancel_operation`, `edit_fact`, `set_policy`...). The chat
  authorizer stops importing `crate::api::*` (A §2). Enforced by an arch-check rule on hub module
  edges.

## RW-20 Operator conversations are durable server-side records — Proposed

- Conversation and Operation are separate aggregates; a conversation can start or attach to an
  operation. History moves from `sessionStorage` to the store. Replies stream over SSE v2. The
  Operator never has its own executor (pack D-23 confirmed).

## RW-21 UI: trace timeline first, graph canvas second; profiles later — Proposed

- **Context.** Pack 14/15 make an interactive graph the dominant element, with zoom, pan, minimap,
  playback; four presentation profiles; motion up to 500 ms. DESIGN.md caps motion at 120–180 ms,
  forbids inline style (most graph libraries need it), and the bundle has ~15 kB headroom (B §5–7).
- **Decision.**
  - One trace schema (steps + parent links + statuses) feeds every view.
  - Ship the operation detail first with a structured step tree/timeline (keyboard- and
    screen-reader-friendly, cycles grouped). The graph canvas follows after a spike chooses a
    library (lazy-loaded chunk, CSP check, DESIGN.md amendment for the canvas).
  - Presentation profiles after the v1 core pages; meanwhile one detailed presentation with
    collapsible sections. When added: profiles only hide/collapse, a test proves P0 and audit
    elements exist in every profile.
  - Motion stays within DESIGN.md tokens; doc 15's longer durations are rejected unless you
    choose otherwise (open decision O-12).

## RW-22 Approval surfaces render the structured plan, not model prose — Proposed

- Every approval surface (web, Discord, Operator) renders, from deterministic code: contract,
  target, args, effect class, reversibility, expiry, plan-hash prefix. Model text is secondary and
  labelled. Discord buttons name the target. (Highest-ranked current risk, E risk 1.)

## RW-23 Autonomy guardrails — Proposed

- In v1 a `PolicyPermit` additionally requires: target equals the operation's pinned subject (the
  incident's host/container/unit) unless the rule names other targets explicitly; per-rule and
  global rate limits; a global **pause autonomy** switch (UI + chat command, human-only); a circuit
  breaker that suspends a rule after N failed/`OutcomeUnknown` executions; shadow mode (decisions
  recorded, nothing executed) as the default state of every new rule.

## RW-24 Step-up authentication for anything that can execute or change trust — Proposed

- Settings that can cause process execution or widen trust require password re-entry: LLM CLI
  `command`/`args` (today they do not, E risk 3), policy rules, unattended ceilings shown in UI,
  webhook secrets, allowed chat users. Implement as a declarative list checked by a test.
