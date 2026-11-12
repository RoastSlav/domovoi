# 03 — Architecture decision records (rework)

Numbered `RW-NN` so they do not collide with `docs/adr/00NN`. Each one becomes a repository ADR in
`docs/adr` when its milestone starts. Detailed contracts (schemas, state machines, tables, tests)
live in `04-target-architecture.md`; this file records the decision, why, and what was rejected.

**Labels:** FACT (code, tests, documents), INFERENCE (follows from facts, not demonstrated),
PROPOSAL (recommended design), DECISION (approved by the owner; `D-NN` refers to the owner decision
list in `99-open-decisions.md`), OPEN (needs an owner decision).

| RW | Title | Status |
|---|---|---|
| 01 | Evolve the existing workspace | DECISION (D-02 context) |
| 02 | No separate v0.1 release; foundation slices first | DECISION D-01 |
| 03 | Transactional unit of work | DECISION D-02 |
| 04 | Operation aggregate; Execution is a typed Step owned by the operation writer | DECISION D-02 / PROPOSAL (ownership detail) |
| 05 | Coarse operation status, immutable steps, append-only step events | DECISION D-02 / PROPOSAL (detail) |
| 06 | Typed model output, checkpointed reasoning, model-call retry semantics | PROPOSAL |
| 07 | Hub-owned capability contracts | DECISION D-02 |
| 08 | One execution boundary bound to an immutable plan | PROPOSAL |
| 09 | Agent action journal, idempotency, OutcomeUnknown, availability states | PROPOSAL |
| 10 | Policy evaluator: most-specific rule wins | DECISION D-05 / PROPOSAL (formalisation, incomparable fallback OPEN) |
| 11 | Authorization types and their enforcement in Rust | PROPOSAL (corrects the earlier "compile-time seal" claim) |
| 12 | Host-local unattended ceiling | DECISION D-03 / PROPOSAL (format) |
| 13 | L3 containment in v1 (M9), first capability set | DECISION D-04 / OPEN (exact set) |
| 14 | Knowledge: typed facts and relations over existing inventory | DECISION D-02 / PROPOSAL |
| 15 | One journal: experience, change stream and SSE replay | DECISION D-02 / PROPOSAL (sequence model) |
| 16 | One SQLite file; durability settings | FACT + PROPOSAL |
| 17 | Protocol evolution and support window | PROPOSAL (support window OPEN) |
| 18 | Superseded — merged into RW-15 | — |
| 19 | Use-case layer in the hub | PROPOSAL |
| 20 | Durable Operator conversations | DECISION (pack D-23) / PROPOSAL |
| 21 | UI: shared trace schema, timeline then graph, presentation profiles, longer motion — all v1 | DECISION D-08, D-10, D-12 |
| 22 | Structured approval surfaces | PROPOSAL |
| 23 | Autonomy guardrails and target binding | DECISION D-07 / PROPOSAL |
| 24 | Step-up authentication | PROPOSAL |
| 25 | Experience retrieval and resolution summaries | DECISION D-02 |
| 26 | Capability packages and UI installation | DECISION D-11 / OPEN (third-party trust) |
| 27 | Audit hash chain | DECISION D-14 / PROPOSAL (format) |
| 28 | Configurable retention | DECISION D-09 / OPEN (defaults) |
| 29 | Actor identity ready for multi-user | DECISION D-13 / PROPOSAL |
| 30 | Startup recovery order | PROPOSAL |
| 31 | Schema compatibility and rollback | PROPOSAL |
| 32 | Legacy actions: retained and projected, never rewritten | PROPOSAL |
| 33 | UI mutation and state safety rules | PROPOSAL |

---

## RW-01 Evolve the existing workspace

- **FACT.** The crate graph matches ARCHITECTURE.md §4 and is enforced by `xtask arch-check`
  (A §1). Single-writer incident and action services exist (ADR 0011, ADR 0019). The plan hash is
  checked on hub and agent. The runner is argv-only. There are ~900 Rust tests and 5 gate files (F §1).
- **Decision.** Add new aggregates and modules inside the existing crates; wrap old paths and
  remove them only after a parity suite passes. No new crates, databases, brokers or services.
- **Rejected.** Greenfield v2: discards the gate tests that encode the PR lessons.

## RW-02 No separate v0.1 release (D-01)

- **Decision.** The former M0 is not a release. Its contents become the first foundation slices
  (F1–F4 in `06`): unit of work and audit, approval freshness and recovery order, protocol tolerance,
  and the reliability fixes. Nothing is tagged v0.1 before the rework.
- **Consequence.** The product on the branch must still stay usable after every slice (single owner,
  D-01), but no release-quality gate applies until the first rework release.

## RW-03 Transactional unit of work (D-02)

- **FACT.** The action writer commits `update_from` and then appends audit as a separate statement
  whose failure is only logged (`engine/src/actions/mod.rs:127-153`). The incident manager writes
  signal, incident and timeline one at a time (`engine/src/manager/actor.rs`). The loop guard is
  rebuilt from audit rows (`actions/recover.rs`), which may be missing.
- **Decision.** A `UnitOfWork` port in `domovoi-core::ports` commits, in one SQLite transaction:
  typed state writes with expected versions, journal entries, audit drafts, new pending effects, and
  effect completions. The store assigns journal sequence numbers and audit hashes inside the
  transaction. Any failure (conflict, constraint, I/O) rolls everything back and returns a typed
  `CommitError`. Contract in `04` §5.
- **What it does not solve.** The port does not decide who may write which aggregate. Ownership is
  RW-04/RW-05: each `StateWrite` variant is built only by its owning writer module (arch-check
  scan, best effort). A generic `commit` used by two writers on the same aggregate would still race;
  optimistic `version` checks turn that race into a `Conflict` error instead of a lost update.
- **Rejected.** Separate writes plus repair jobs (repairs cannot restore lost audit). Event sourcing
  (current-state rows stay the read model; replay machinery is unnecessary at this scale).

## RW-04 Operation aggregate; Execution is a typed Step

- **FACT.** There is no aggregate for a remediation as a whole. Triage state is stringly-typed
  `incident_events` rows; the triage queue and verification timers live in memory (A §4–5).
- **Decision (D-02).** `Operation` is a first-class aggregate, distinct from `Incident`.
- **Decision on ownership (PROPOSAL).** An Execution is **a typed Step of kind `Execution`**, plus a
  1:1 `execution` detail row (contract, target, args, plan hash, authorization, dispatch and result
  data). Both belong to the Operation aggregate and are written **only by the operation writer**.
  - The ADR 0019 action writer becomes the operation writer's execution module. There is one writer
    task for all operations (as there is one for all actions today), so there is exactly one owner
    and no second source of execution status.
  - v1 allows **one dispatch attempt per Execution step**. Retrying means a new Execution step that
    references the previous one (`retry_of`) and needs its own authorization. Reconciliation
    (RW-09) is not a new attempt; it completes the existing one.
  - Every write to an execution, including chat-token rotation for a waiting approval (FACT today:
    it bypasses the writer, `engine/src/triage/proposals.rs:115`), is a command to the operation
    writer.
  - Incidents stay with the incident manager (ADR 0011). The link between an operation and its
    incidents is owned by the operation writer (`operation_incidents` rows). The incident manager
    reads it but never writes it.
- **Rejected.** Execution as a separate aggregate with its own writer: completion would need a
  cross-writer handshake on every result, which is the race class ADR 0019 removed.

## RW-05 Coarse operation status, immutable steps, append-only step events

- **Decision.** `OperationStatus` is coarse. A Step's intent (kind, contract, target, args,
  plan hash, parent, phase label) is immutable once inserted. Its progress is an append-only
  sequence of journal entries (`StepStarted`, `StepFinished{outcome}`, ...). The `steps.status`
  column is a **derived** projection updated in the same transaction, for queries only. State
  machines, the outcome vocabulary and the transaction boundaries are in `04` §3.
- **Why.** The orchestrator decides on typed events, not model text; dynamic investigation is a
  growing step list; resume reads one log.

## RW-06 Typed model output, checkpointed reasoning, model-call retry

- **FACT.** Follow-up proposals use a `PROPOSALS:` text protocol (A §6). PR #110 had to re-validate
  cached proposals. Provider/model attribution is only in failure text, and failed calls record no
  usage (G). The LLM client has only an idle read timeout (G).
- **Decision.**
  - Model output is meta-tool calls with schema validation (RW-07). Deterministic code turns them
    into steps or `Refused` steps whose reasons are fed back to the model.
  - Each `ReasoningTurn` step persists its context references, model, provider, token usage,
    latency and outcome, **for failed calls too**, before any effect it requests is created.
  - **Model-call retry is separate from execution retry.** A model call is read-only but not
    idempotent: a retry costs money and may give a different answer. Transport, timeout, 5xx and
    rate-limit errors are retried within the operation's turn budget (rate limits honour
    `retry_after`); auth and invalid-request errors are not. After a crash, an unanswered turn is
    re-issued as a new attempt. Only a committed turn result is ever acted on; an answer lost
    before commit is discarded, never replayed from memory.
  - Every call has a hard wall-clock deadline in addition to the idle timeout.
  - Replayed history and provider error text pass through the redactor (G: provider error text is a
    leak channel; fixed for one path in #113).

## RW-07 Hub-owned capability contracts (D-02)

- **FACT.** The hub takes `risk` from the agent's skill summary. A skill declaring `risk = "read"`
  runs when the model asks, with no human step (`engine/src/triage/tools.rs:149-158`,
  `agent/src/requests/skills.rs:140`). `Hello.capabilities` is always empty
  (`agent/src/link/handshake.rs:47`). `ReadTool` is a compile-time catalogue wired in five places.
- **Decision.**
  - Built-in contracts are typed in `domovoi-core`: id, version, input schema, effect class
    (`Read | Mutate | Contain | Destructive`), category, target kind, reversibility, idempotency,
    timeout, output limit, verification hint, and the arguments a host ceiling may constrain.
  - Skills are the data-defined family `skill.<name>`. Their effect class comes from the owner's
    local manifest (unchanged trust boundary), with a hub **floor**: the owner may raise a skill's
    class in the hub, never lower it; an unknown class is treated as `Destructive`.
  - Agents advertise implementation facts only: contract id, versions, implementation hash,
    availability, local ceiling summary.
  - The model sees a closed meta-tool enum: `list_capabilities`, `describe_capability`,
    `request_evidence`, `search_experience`, `propose_plan`, `ask_user`, `finish`. No meta-tool
    approves or executes, so INV-1 and the `model_sees_only_read_tools` gate keep holding.
- **Rejected.** Provider-declared descriptors as in pack 03: a provider would define its own
  authority.

## RW-08 One execution boundary bound to an immutable plan

- **FACT.** The per-host skill narrowing (`PolicyLink`, `hub/src/agent_server/skill_policy.rs`) is a
  wrapper the consumer must be handed; `server.link()` is public and no arch rule prevents bypass (G).
  The plan hash covers the manifest but not the entry script's bytes
  (`core/src/skill/manifest.rs:145-151`). The approve API carries no hash (E §5). Dispatch ignores
  `expires_at` (01 D04).
- **Decision.**
  - One function, `execute(AuthorizedExecution)`, in the engine's operation writer module. Arch
    rule: no other module calls `AgentLink::run_skill` / `run_capability`.
  - Reads get the same treatment: one `collect_evidence` read boundary (registry checks for
    enablement, availability and host scope) is the only caller of `AgentLink::run_tool`, for the
    model, profile collection and the Operator alike. The raw link stops being publicly reachable
    from transports (FACT today: `server.link()` is public, G).
  - Plan hash v2 = SHA-256 over a canonical encoding of: contract id + version, implementation hash
    (skills: manifest **and** entry-script bytes), canonical target, canonical args, operation id,
    step id.
  - The approve request carries the plan hash the UI displayed. A mismatch is refused.
  - At dispatch: authorization unexpired; plan hash recomputed and equal; operation `Running`;
    linked incident not `Resolved`/`Dismissed` unless the step says otherwise; contract enabled; agent
    availability `Known` (otherwise wait, RW-09); rate limits; preconditions.

## RW-09 Agent action journal, idempotency, OutcomeUnknown, availability

- **FACT.** On a dropped link the hub marks the action `Failed` while the agent keeps running it, and
  the late reply is discarded (`hub/src/agent_server/session.rs:117-146`). The agent only logs
  `action_id` and does not deduplicate (`agent/src/requests/skills.rs:110`). Restart catch-up runs
  before agents reconnect, and `revalidate` reads the live skill list, which is empty for a host that
  has not connected yet (`engine/src/actions/executor.rs:48, 329-332`). INFERENCE: valid approvals
  are rejected after a restart; no test reproduces this yet.
- **Decision.**
  - Step id = idempotency key.
  - The agent's journal has an **epoch** (random id created with the journal) and records
    `accepted(step_id)` **durably (fsync) before** acknowledging or starting the process, then
    `started`, then `finished(result)`.
  - `action_status(step_ids, expected_epoch)` answers per id: `finished(result)`, `running`,
    `not_found`, or `journal_lost`.
    - `not_found` proves "never started" only because `accepted` is written durably before start, the
      epoch matches the one recorded at dispatch, and the id is inside the journal retention window.
      In that case the step becomes `NotRun`.
    - An epoch mismatch, a corrupt journal or an id older than the window → `journal_lost` →
      `OutcomeUnknown`.
  - `OutcomeUnknown` is terminal for the step and blocks unattended execution of the same contract on
    the same target until a human resolves it or a verification step establishes the state.
  - Agent availability is `Known(catalogue) | Unknown | Absent(since)`.
    - `Unknown`: no session yet since hub start. Steps wait (`WaitingAgent`) and are never rejected.
    - `Absent`: the session ended. Steps wait up to a configurable limit, then become `NotRun` with a
      reason.
  - Old agents (no `journal` feature): a step left `Dispatched` at restart or on link loss becomes
    `OutcomeUnknown` (not `Failed`).

## RW-10 Policy evaluator: most specific rule wins (D-05)

- **DECISION D-05.** The original "explicit deny always wins" is intentionally replaced. A strictly
  more specific matching rule overrides a broader one, even when the broader one is `Deny`. At equal
  specificity `Deny` wins. Host ids only in v1; host tags after v1.
- **Formalisation (PROPOSAL).** A rule's scope is a product of dimensions: host, capability, target.
  Rule A is strictly more specific than B iff A's scope ⊆ B's scope on every dimension and ⊂ on at
  least one. This is a partial order; there is no numeric score. The full definition, decision table,
  trace format and tests are in `04` §7.
- **Incomparable rules (OPEN, recommended default).** When the most specific matching rules
  conflict and none dominates the others: `Deny` > `RequireApproval` > `Allow`.
- **Hard limits** are not rules and are applied after rule resolution: the host-local ceiling
  (RW-12), `Destructive` never `Allow` in v1 (D-06), mandatory containment exclusions (RW-13), and
  unknown/stale safety-critical facts never yielding `Allow`.
- **Pure and deterministic.** `domovoi-engine::policy::evaluate` does no I/O. The simulator calls the
  same function (pack D-28).

## RW-11 Authorization types and their enforcement in Rust

- **Correction (FACT).** The earlier claim that `HumanApproval` is compile-time sealed is wrong.
  `HumanApproval` is defined in `domovoi-core` (`core/src/action/approval.rs:54`) with `pub fn new`
  and `pub fn restore`. There is no `Default` and no `Deserialize`. The call sites are restricted by a
  **line-based text scan** in `xtask arch-check` (`xtask/src/arch/scan.rs`). Allowed prefixes:
  - `new`: `domovoi-hub/src/auth/` and `domovoi-testkit/`;
  - `restore`: `domovoi-store/src/` and `domovoi-testkit/`
  (`xtask/src/arch/rules.rs:169-175`).
  A renamed import (`use … as X`) or a macro would evade the scan.
- **Why a cross-crate private constructor is impossible.** Rust privacy is per crate and module. A
  constructor in `domovoi-core` cannot be made callable only from a module in `domovoi-hub`. The
  engine (which executes) cannot depend on the hub (R1). So the type must live in `domovoi-core`, and
  its constructor must be `pub`.
- **Decision (PROPOSAL).**
  - `domovoi-core::action`:
    - `HumanApproval` (unchanged fields);
    - `PolicyPermit { decision_id, rule_ids, policy_version, plan_hash, issued_at, expires_at,
      ceiling_hash }`;
    - `enum Authorization { Human(HumanApproval), Policy(PolicyPermit) }`.
    No `Default`, no `Deserialize`.
  - Constructors: `HumanApproval::new`/`restore` (as today), `PolicyPermit::issue`/`restore`.
  - Who constructs:
    - `HumanApproval::new` — `domovoi-hub/src/auth/` after authentication.
    - `PolicyPermit::issue` — `domovoi-hub/src/policy/`, behind the `PermitIssuer` port. The port is
      defined in core and implemented in the hub; the engine's operation writer calls it. It issues
      only after evaluator `Allow` + ceiling check + circuit breaker + target binding.
    - `restore` — `domovoi-store/src/` only.
  - Enforcement, three layers:
    1. `clippy.toml` `disallowed-methods` entries for all four constructors. Clippy resolves paths, so
       aliases do not evade it. Allowed sites carry
       `#[expect(clippy::disallowed_methods, reason = "...")]`.
    2. An arch-check rule permits that `expect` only under the allowed prefixes.
    3. The existing text scan stays as a third line.
    `domovoi-testkit` remains allowed. Arch-check additionally asserts that testkit is only ever a
    dev-dependency.
  - Persisted executions store `authorization_kind` plus the fields of the active variant. The store
    rebuilds them with `restore`.
- **Honesty.** These controls stop accidental or careless code paths in review and CI. They are not
  a runtime security boundary against a compromised hub process. Runtime protection comes from the
  plan hash, the agent's local checks (RW-12) and the audit chain (RW-27). Guarantees per threat are
  in `04` §8.

## RW-12 Host-local unattended ceiling (D-03)

- **DECISION D-03.**
  - Owner-controlled config on the agent's host.
  - The hub cannot modify or widen it. The UI shows it read-only. Hub policy may only narrow it.
  - Human-approved and policy-authorized executions stay distinguishable.
  - The agent enforces the ceiling independently of the hub.
  - Owner-signed policy bundles are planned after v1.
- **Format (PROPOSAL, smallest useful).** The agent config gains `[[unattended.allow]]` entries:
  - `capability` (exact contract id);
  - `targets` (explicit list, or `"*"` written explicitly);
  - `max_per_hour`;
  - for contracts that declare ceiling-constrainable arguments, an `args` table using only those
    keys (e.g. `max_ttl`, `min_prefix_len`);
  - for `Contain` contracts, `max_active` and `max_ttl` are required.
  `[unattended.exclusions]` holds the mandatory management-path list used by containment (RW-13).
  No entry means nothing runs unattended. Full schema: `04` §8.
- **Protocol.** `run_capability` carries `authorization_kind = human | policy`.
  - For `policy`, the agent checks the ceiling (capability, target, args, rate, TTL) and
    `allow_risk`.
  - For `human`, it checks `allow_risk` as today.
  - Agent-side `max_per_hour` applies to **both** kinds for mutating contracts. This bounds the damage
    from a hub that lies about the kind.
  - The agent reports a hash and a summary of the ceiling in `hello` for read-only display and hub
    pre-checks.
- **What it does not protect against** (stated plainly). The ceiling is an enforcement limit, not a
  signature. A compromised hub can label a dispatch `human` and run any skill `allow_risk` permits;
  that is today's exposure, now rate-bounded. Owner-signed approvals and bundles (post-v1) are the
  fix.
- **Rejected for v1.** Signed bundles (key custody, rotation and recovery cost), and capability-name-only
  lists (no target bound, so they are too broad for containment).

## RW-13 L3 containment in v1 (D-04)

- **DECISION D-04.** `Contain` is a general policy class. L3 ships in v1 as milestone M9 and is not
  limited to firewall/IP blocking. Offline emergency permits are excluded from v1.
- **First set (OPEN, recommended):**
  - `firewall.block_source` — temporary inbound block of one external address or a small prefix,
    using an nftables set element with a kernel-enforced timeout;
  - `container.pause` — pause one named container, with a TTL;
  - `unit.stop_temporary` — stop one named systemd unit, with a TTL, excluding a hard-coded list of
    management units.
  Per-capability threat, privileges, lockout analysis, expiry, dead-man, verification, outcome-unknown
  handling and drills: `04` §9. Later candidates (container network isolation, SSH session
  termination, account lock) are deferred because their lockout or recovery stories are weaker.
- **Triggering.** Automatic containment is eligible only when a deterministic detector (check code)
  emitted a typed signal whose structured fields name the target (e.g. a source address), and that
  target is the operation's pinned subject. Free-form model output never triggers or targets
  automatic containment. The model may investigate, explain and recommend; such recommendations go
  through human approval.
- **Prerequisites.** M8 complete (permits, ceiling, target pinning, journal, verification, circuit
  breaker), plus management-path facts and the drill suite.

## RW-14 Knowledge: typed facts and relations over existing inventory

- **FACT.** Baselines already store observed containers, units and ports. `digest_facts` is a
  metric-sample table whose name collides with "facts" (C). Host profiles are generated Markdown
  notes (A §9).
- **Decision.**
  - Baselines remain the owner of observed inventory. Facts and relations **reference** inventory
    subjects through typed refs (`host:<id>`, `container:<host>/<name>`, `unit:<host>/<name>`,
    `entity:<uuid>`). They never copy inventory rows.
  - A small `entities` table exists only for user-defined subjects (e.g. "VPN", "NAS pool").
  - Facts carry `source`, `status`, `observed_at`, `valid_until`, `last_verified_at`, `supersedes`
    and an evidence reference. There is no numeric confidence. Predicates are a closed, typed enum
    in v1.
  - `digest_facts` is renamed to `metric_samples` first.
  - Schemas and lifecycle: `04` §10.

## RW-15 One journal: experience, change stream and SSE replay

- **FACT.** `incident_events` has free-text kinds and is purged at 90 days. `audit_log` is
  trigger-protected but written outside the state transaction (C). SSE has no event ids and drops
  events for slow clients (`hub/src/api/events.rs:76-80`, B §2).
- **Decision (PROPOSAL; D-02 requires no third semantic store).**
  - One `journal` table: typed operational events for incidents, operations, conversations and
    knowledge. It has a global `seq` (SQLite `INTEGER PRIMARY KEY AUTOINCREMENT`, assigned inside the
    unit-of-work transaction).
    - All writes go through the single writer connection (FACT: pool of 1, C §2), so `seq` order
      equals commit order across all writers.
    - The journal is the experience record and also the change stream SSE replays.
    - New incident timeline events are written here. Existing `incident_events` rows are copied once
      with their original timestamps, then the old table is retired (no history is invented: the
      copy is 1:1).
  - `audit_log` remains the separate **security** record (who authorized what, logins, policy and
    settings changes, refusals). It is written in the same transaction and hash-chained (RW-27).
  - `effects` is the **work outbox** (pending side effects). It is not an event store.
  - SSE `id` = journal `seq`. `Last-Event-ID` replays `seq > id` filtered by topic. A gap beyond
    retention sends a `reset` event, and the client refetches snapshots. The live tail is a broadcast
    of "new seq available" plus a read from the table, so a lagging subscriber re-reads instead of
    silently losing events. Details: `04` §6.
- **RW-18 is merged here.**

## RW-16 One SQLite file; durability settings

- **FACT.** WAL, `synchronous=NORMAL`, busy timeout 5 s, one writer connection and 4 readers, no
  startup integrity check, nightly `VACUUM INTO` keeping 7 copies, `pre-migrate-*` files never pruned
  (C §2–3).
- **Decision.** Stay on one file. Use `synchronous=FULL` once execution state lives in the journal:
  a committed dispatch must survive power loss. Add a startup `quick_check`, backup verification
  (open the copy and run `quick_check`), `domovoi restore`, and pruning of `pre-migrate-*` per
  retention. No vector database, graph database, broker or workflow engine (pack Q2.1, Q2.2, Q2.6).

## RW-17 Protocol evolution and support window

- **FACT.**
  - `PROTOCOL_VERSION` is 1.1 (`proto/src/version.rs:17`). The hub accepts only the same minor or one
    behind; a newer minor is rejected, and the rejection is final on the agent (`version.rs:22`).
  - `#[serde(other)]` is used at `messages/handshake.rs:246` and `messages/work.rs:110,207`. It maps
    unknown variants to a unit variant and **discards the original value**.
  - Frame cap is 1 MiB.
- **Decision.**
  - Reject only on a major mismatch or an agent below `MIN_AGENT_VERSION`.
  - `hello` carries a `features` set; a feature is used only when both sides list it.
  - Tolerant decoding with `Open<T> = Known(T) | Unknown(String)`, implemented by a custom
    `Deserialize` that reads the raw string and then parses it, so the original value is kept for
    logs and replies.
  - An unknown message `type` gets an `unsupported{type}` reply; the session continues.
  - Catalogue by hash plus paged `get_catalog`.
  - New messages `run_capability` and `action_status`.
  - Golden frames for every release are kept and decoded in CI.
- **Support window (OPEN, recommended).** Until 1.0, the hub supports agents from the current and
  the previous two minor releases; `MIN_AGENT_VERSION` moves accordingly; older agents get a final
  reject with a clear reason. After 1.0: same major, all minors. Feature-dependent behaviour on old
  agents: `04` §12.

## RW-19 Use-case layer in the hub

- **FACT.** `auth/interactions/*` imports `crate::api::*` (A §2).
- **Decision.** HTTP handlers, Discord interactions and the Operator call application services
  (`approve_step`, `start_operation`, `cancel_operation`, `edit_fact`, `publish_policy`,
  `request_install`, ...). An arch-check module-edge rule inside the hub forbids transports importing
  each other.

## RW-20 Durable Operator conversations

- **FACT.** Chat history lives in `sessionStorage`; replies are a single POST (B §3).
- **Decision.** `Conversation` is an aggregate with its own writer. Messages are stored. Streaming
  tokens are ephemeral SSE events without ids. The committed final message is a journal event. A
  conversation can start or attach to an operation. Host or global scope is explicit per conversation
  (lesson: host/global chat scope). A stale response for a superseded request is discarded by request
  id (lesson: chat races). The composer draft is retained locally until the send is committed
  (lesson: user input retention).

## RW-21 UI: trace schema, timeline and graph, profiles, motion — all v1

- **DECISION D-08.** v1 has both the step timeline/tree and the interactive orchestration graph.
  The timeline is built first. The trace schema is frozen with golden fixtures before either view.
  The graph is the primary visualization on operation detail. The non-canvas outline stays as the
  accessible representation.
- **DECISION D-10.** Detailed, Standard and Essential profiles ship in v1. They are one set of
  components with a category-to-visibility table, not three layouts. P0 (safety, approval, audit)
  content is present in every profile. Mechanism: `04` §13.
- **DECISION D-12.** Longer motion is allowed for visualization and layout transitions. Ordinary
  micro-interactions keep the existing 120/180 ms tokens. Reduced motion is mandatory.
- **Correction.** The earlier claim that doc 15 contradicts DESIGN.md conflated two cases.
  DESIGN.md's tokens govern micro-interactions. Doc 15's longer durations are graph and transition
  motion, which DESIGN.md did not address. DESIGN.md now defines both.

## RW-22 Structured approval surfaces

- **FACT.** Discord Approve/Deny buttons name no skill, host or arguments, and the card text is
  model prose (`engine/src/cards.rs:111-157`, `chat/src/discord/components.rs:34-47`).
- **Decision.** Every approval surface renders, from deterministic code: contract, target, args,
  effect class, reversibility, expiry, plan-hash prefix, and the policy decision with its winning
  rule. Model text is secondary and labelled. Discord buttons name the target. One shared web
  component is used on operation detail (its canonical home), incidents, the Control Room and the
  Operator.

## RW-23 Autonomy guardrails and target binding (D-07)

- **DECISION D-07.** Unattended steps may target only the operation's pinned subjects plus targets
  explicitly named by the matching owner-authored rule.
- **Decision (PROPOSAL).**
  - Pinned subjects are set at creation (from the incident subject, or by the owner in a
    conversation) and changed only by an owner command.
  - Plan validation resolves the model's target string to a canonical `TargetRef` from inventory;
    unresolvable → `Refused`.
  - A target outside pinned ∪ rule-named → the step can only be `RequireApproval`, flagged
    "outside operation scope".
  - The plan hash binds the resolved target.
  - Further guardrails:
    - per-rule and global rate limits;
    - a global **pause autonomy** switch (human-only, from UI and chat);
    - a circuit breaker that suspends a rule after N `Failed`/`OutcomeUnknown` results (default 2
      within 24 h);
    - every new rule starts in shadow mode.
  - First L2 contracts (M8): `container.restart` and `unit.restart` of one named target, each with a
    postcondition check. Docker write access for the former needs its own ADR (O-24).

## RW-24 Step-up authentication

- **FACT.** `is_sensitive` covers secrets, `discord.allowed_user_ids`, `web.trusted_proxies` and
  `webhooks`, but not `llm.cli.command`/`args` (`hub/src/settings/patch.rs:109-116`).
- **Decision.** A declarative list of settings and actions that need password re-entry, checked by a
  test. It covers:
  - LLM CLI command/args;
  - publishing a policy version;
  - enabling a capability;
  - starting an installation;
  - changing retention;
  - webhook secrets and allowed chat users;
  - the pause switch when turning autonomy back **on**.

## RW-25 Experience retrieval and resolution summaries (D-02)

- **Decision.**
  - Every terminal operation gets a `resolution_summary`:
    - deterministic fields filled by code: subjects, contracts executed, verification results,
      outcome;
    - narrative fields (problem, cause, fix) drafted by the model, marked `source = model`, and
      editable or confirmable by the owner.
  - Summaries are full-text indexed.
  - `search_experience(query, scope)` is a host-scoped read meta-tool.
  - The UI shows similar past operations on incident and operation pages.
  - When experience is supplied to reasoning, each item carries its operation id, date, verified
    flag and source, inside a block labelled "historical evidence, not authorization".
  - Past approvals and successes never satisfy policy.
  - Details: `04` §10.

## RW-26 Capability packages and UI installation (D-11)

- **FACT.**
  - Skills are owner-placed files in the agent's `skills_dir` (`config/src/agent.rs:52`).
  - The hub cannot write files on a host today.
  - Agent self-update is out of scope (INV-3, ADR 0003).
- **Decision (PROPOSAL).**
  - A capability package is a signed archive: manifest, entry script(s), package metadata, and an
    Ed25519 signature over the archive digest.
  - The agent installs a package only if:
    1. local config `[packages] allow_install = true`, which is off by default;
    2. the signature verifies against a key in the agent's **local** `trusted_keys`. The first-party
       release key is shipped in the agent config template and can be removed by the owner. The hub
       cannot add keys.
    3. the package is compatible (agent version, platform, contract versions).
  - The hub only **relays** the bytes. A compromised hub can offer packages but cannot make the
    agent accept one not signed by a locally trusted key.
  - Installing never enables or authorizes anything. Enablement (hub) and unattended use (policy +
    host ceiling) stay separate steps.
  - Full lifecycle: `04` §11.
- **OPEN.** Third-party/community packages. Recommendation for v1: first-party signed packages, plus
  the owner adding their own key locally (owner-signed packages). No arbitrary third-party
  registries in v1.

## RW-27 Audit hash chain (D-14)

- **Decision.**
  - Each audit row stores `prev_hash` and `hash = SHA-256("domovoi-audit-v1" ‖ seq ‖ prev_hash ‖
    canonical_json(entry))`, computed inside the unit-of-work transaction.
  - Genesis `prev_hash` = SHA-256 of the installation id.
  - Startup verifies the last N rows (configurable, default 10 000). `domovoi audit verify` checks
    everything.
  - Pruning writes a checkpoint row (first retained seq, its prev_hash), which is itself chained.
- **Threat model (stated plainly).** It detects accidental corruption, out-of-band edits by tools
  that do not recompute the chain, and deletion in the middle of the chain. It does **not** stop an
  attacker who can rewrite the whole database and recompute the chain.
- **Cheap external anchor (PROPOSAL, v1).** The current head hash appears in the daily digest and in
  `domovoi audit head`, so the owner has off-host copies. Stronger anchoring (a transparency log,
  remote append-only storage) is post-v1.

## RW-28 Configurable retention (D-09)

- **DECISION D-09.** Every retention value is configurable. Current defaults are kept where they
  exist. Raw data and durable summaries are separate classes. Interaction with FTS, backups and the
  audit chain: `04` §14. Proposed default values are OPEN in `99`.

## RW-29 Actor identity ready for multi-user (D-13)

- **Decision.**
  - Single owner through v1, with no RBAC.
  - Every audit entry, journal entry and authorization uses an `Actor` enum:
    `User(UserId) | System(component) | Agent(HostId) | Policy(rule_id)`.
  - Every check that a request is allowed goes through one function, `authorize(actor, action)`. In
    v1 it returns allowed for the owner.
  - Roles can later replace that function without changing stored identities.

## RW-30 Startup recovery order

- **Decision.** In order:
  1. Open the store, run migrations, `quick_check`, audit verify.
  2. Load writers (incidents, operations, conversations).
  3. Start the agent server; every host is `Unknown`.
  4. The effect runner resumes non-dispatch effects (model turns, notifications, timers due).
  5. Executions in `Dispatched` are reconciled per host when that host's `hello` arrives (RW-09).
  6. `Approved`/`Permitted` but undispatched executions are re-checked for freshness and dispatched
     only when the host becomes `Known`.

  Nothing is rejected because a host is still `Unknown`.

## RW-31 Schema compatibility and rollback

- **FACT.** Migrations run on every `Store::open` after a `VACUUM INTO` backup. There are no down
  migrations, no restore command, and no tested "new DB, old binary" case (F §3).
- **Decision.** Distinguish four cases:
  1. **Code rollback** is supported only across releases whose migrations are additive. A
     `schema_compat` row records the lowest binary version that can read the schema. An older binary
     reading a newer schema starts only if its version ≥ that value, and it ignores unknown tables
     and columns (sqlx configured to ignore applied migrations it does not know).
  2. **Forward fix** is the default answer to a bad migration.
  3. **Full restore** (`domovoi restore <backup>`) is explicit, never automatic. It loses everything
     written after the backup, and the command says so and requires confirmation.
  4. Agents are unaffected by hub schema rollback. Operations in flight after a restore are
     reconciled via `action_status` against journals that may contain later ids. These are reported
     as "executed after the restored backup" and audited, never silently dropped.

## RW-32 Legacy actions: retained and projected

- **Decision.**
  - Existing `actions` rows are **not** migrated into operation/step records. They stay as they are
    and appear through a read-only projection marked **Legacy** in the Operations list and on the
    incident page.
  - No steps, events or approvals are synthesized.
  - While the legacy executor still exists, pending legacy actions follow today's path.
  - The release that removes it expires any remaining legacy `Proposed`/`Approved` rows with the
    audited reason "legacy action expired at upgrade" (a real event, not invented history).
- **Why.** A projection preserves the original records exactly. Converting them would invent step
  structure, policy decisions and plan-hash v2 values that never existed.

## RW-33 UI mutation and state safety rules

- **FACT.** The mutation-feedback lint is file-level (a file passes if it uses `useActionFeedback`
  anywhere, with `feedback-ok` exemptions). The unsaved-change guard exists only in the notes editor.
  `QueryBoundary` drops refresh errors (B §4, G).
- **Decision (PROPOSAL).**
  - Mutation feedback is checked per call site: every `useMutation` result must reach
    `useActionFeedback`. This is an ESLint rule on the call expression, not on the file.
  - Every form with editable state uses the shared leave guard. A test lists the forms.
  - A failed load or refresh is never rendered as empty. `QueryBoundary` shows stale data with a
    refresh-failed banner (test `refresh_failure_is_visible`).
  - Acknowledge-with-rule creates the rule before closing the incident (lesson #66). The order is
    kept by doing both in one use-case service call.
  - Provider error text shown in the UI passes through `describeError` and the redactor (RW-06).
