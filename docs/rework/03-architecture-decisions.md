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
| 09 | Agent action journal, crash windows, OutcomeUnknown, availability states | PROPOSAL |
| 10 | Policy evaluator: most-specific wins, locked Deny, incomparable fallback | DECISION D-05, D-19, D-23, D-26 / PROPOSAL (formalisation) |
| 11 | Authorization types and their enforcement in Rust | PROPOSAL (corrects the earlier "compile-time seal" claim) |
| 12 | Host-local unattended ceiling | DECISION D-03, D-18 / PROPOSAL (format) |
| 13 | L3 containment in v1 (M9): firewall.block_source first | DECISION D-04, D-17, D-22, D-28 |
| 14 | Knowledge: typed facts and relations over existing inventory | DECISION D-02 / PROPOSAL |
| 15 | One journal: experience, change stream and SSE replay | DECISION D-02 / PROPOSAL (sequence model) |
| 16 | One SQLite file; synchronous=FULL from F1 | FACT + PROPOSAL |
| 17 | Protocol compatibility: exact major.minor | DECISION D-20 / PROPOSAL (detail) |
| 18 | Superseded — merged into RW-15 | — |
| 19 | Use-case layer in the hub | PROPOSAL |
| 20 | Durable Operator conversations | DECISION (pack D-23) / PROPOSAL |
| 21 | UI: shared trace schema, timeline then graph, presentation profiles, longer motion — all v1 | DECISION D-08, D-10, D-12 |
| 22 | Structured approval surfaces | PROPOSAL |
| 23 | Autonomy guardrails and target binding | DECISION D-07 / PROPOSAL |
| 24 | Step-up authentication | PROPOSAL |
| 25 | Experience retrieval and resolution summaries | DECISION D-02 |
| 26 | Capability packages and UI installation | DECISION D-11, D-16 / PROPOSAL (format) |
| 27 | Audit hash chain | DECISION D-14 / PROPOSAL (format) |
| 28 | Configurable retention and defaults | DECISION D-09, D-21 |
| 29 | Actor identity ready for multi-user | DECISION D-13 / PROPOSAL |
| 30 | Startup recovery order | PROPOSAL |
| 31 | Schema compatibility and rollback | PROPOSAL |
| 32 | Legacy actions: retained and projected, never rewritten | PROPOSAL |
| 33 | UI mutation and state safety rules | PROPOSAL |
| 34 | Stable target identity | PROPOSAL |
| 35 | Effect delivery semantics (at-least-once) | PROPOSAL |
| 36 | Execution identity across Action → Step | PROPOSAL |
| 37 | Management-path confirmation for containment | DECISION D-28 / PROPOSAL (procedure) |
| 38 | Agent-side mutation rate limiter | DECISION D-27 / PROPOSAL (format) |
| 39 | Narrow first-party privileged helpers | DECISION D-24 / PROPOSAL |
| 40 | Step graph: parentage plus typed edges | PROPOSAL |
| 41 | Resolution summary provenance per field | PROPOSAL |

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
  - Plan hash v2 = SHA-256 over a canonical encoding of: `plan:v2`, contract id + version,
    implementation hash (skills: manifest **and** entry-script bytes), canonical target identity
    (RW-34), canonical args, `execution_id` (RW-36: the ActionId before M3, the step id after).
  - The approve request carries the plan hash the UI displayed. A mismatch is refused.
  - At dispatch: authorization unexpired; plan hash recomputed and equal; operation `Running`;
    linked incident not `Resolved`/`Dismissed` unless the step says otherwise; contract enabled; agent
    availability `Known` (otherwise wait, RW-09); rate limits; preconditions.

## RW-09 Agent action journal, idempotency, OutcomeUnknown, availability

- **FACT.**
  - On a dropped link the hub marks the action `Failed` while the agent keeps running it; the late
    reply is discarded (`hub/src/agent_server/session.rs:117-146`).
  - The agent only logs `action_id` and does not deduplicate (`agent/src/requests/skills.rs:110`).
  - The runner starts each child in its own process group with `kill_on_drop(true)`
    (`runner/src/exec/spawn.rs:54,62`). `kill_on_drop` only runs when the handle is dropped, not
    when the agent process is killed.
  - The shipped agent unit uses `Restart=always` with systemd's default `KillMode=control-group`
    (`deploy/systemd/domovoi-agent.service:25`). INFERENCE from systemd semantics: leftover children
    in the unit's cgroup are killed before a restart. Manual runs have no such guarantee.
  - Restart catch-up reads the live skill list, which is empty for a host that has not reconnected
    (`engine/src/actions/executor.rs:48,329-332`). STATIC: not reproduced.
- **Decision (PROPOSAL).**
  - **Identity.** The idempotency key is an opaque `execution_id` that never changes after dispatch
    (RW-36).
  - **Journal records.** The agent journal has an **epoch**: a random id created with the journal
    and reported in `hello`. Records, each fsynced:
    1. `accepted{execution_id}`, written before the reply and before spawning;
    2. `started{pid, proc_start_time}`, written immediately after spawn;
    3. `finished{result}`.

    A second request with a known `execution_id` never spawns again. It returns the recorded
    state.
  - **`action_status(execution_ids, expected_epoch)` answers:**
    - `finished(result)`;
    - `running`, only from the agent instance that spawned the child and still holds it;
    - `not_found`, only when the epoch matches and the id is inside the journal retention window;
    - `outcome_unknown{reason}`.
  - **Mapping to outcomes:** `not_found` → `NotRun` (the request never reached the journal). An
    `accepted` record without `finished`, after an agent restart → `OutcomeUnknown`: the agent
    cannot prove whether the process ran. Crash-window table: `04` §15.2.
  - **No indefinite `Running`.** After an agent restart, a `started` record without `finished` is
    never reported as `running`.
    - If `pid` is alive and its `/proc/<pid>/stat` start time equals the recorded one (orphan,
      possible only outside systemd or a container), the agent kills that process group, records
      `abandoned`, and reports `outcome_unknown`.
    - Otherwise it reports `outcome_unknown` directly.
  - **`OutcomeUnknown`** is terminal for the execution. It blocks unattended execution of the same
    contract on the same target until a human resolves it or a verification step establishes the
    state.
  - **Host availability:** `Known(catalogue) | Unknown | Absent(since) | Incompatible(reason)`.
    - `Unknown`: no session since hub start. Steps wait, and are never refused for this reason.
    - `Absent`: steps wait up to `agent_wait_limit`, then become `Unavailable`.
    - `Incompatible`: protocol mismatch (RW-17). Steps become `Unavailable` with "agent upgrade
      required".
  - **Outcome vocabulary** (single definition in `04` §3.2):
    - `NotRun`: positive evidence of never starting, or expiry before dispatch.
    - `Unavailable`: could not be attempted.
    - `Failed`: attempted and reported failure.
    - `OutcomeUnknown`: may have run, result unknowable.

## RW-10 Policy evaluator: most specific rule wins, plus locked Deny (D-05, D-19, D-26)

- **DECISION D-05.** The original "explicit deny always wins" is intentionally replaced for ordinary
  rules:
  - a strictly more specific matching rule overrides a broader one, even a broader `Deny`;
  - at equal specificity, `Deny` wins;
  - host ids only in v1, host tags after v1.
- **DECISION D-19.** Incomparable conflicting maximal rules resolve
  `Deny > RequireApproval > Allow`. Publication shows each such conflict and requires an explicit
  acknowledgement. Runtime and simulator use the same function.
- **DECISION D-26.** A `Deny` rule may be marked **locked** (default off). A matching locked Deny is
  a hard prohibition: no narrower rule overrides it.
  - Several matching locked Denies produce one `Deny` that lists every locked rule as a reason.
  - Creating, editing, unlocking or removing a locked rule needs step-up authentication, a
    simulation preview of the affected decisions, and an audit entry (`policy.lock_changed`).
- **DECISION D-23.** `Deny` means "not offered for approval". There is no approval bypass; the owner
  must change the policy. `RequireApproval` means "a human may approve".
- **Formalisation (PROPOSAL).**
  - A rule's scope is a product of dimensions: host, capability, target.
  - A is strictly more specific than B iff A ⊆ B on every dimension and A ⊊ B on at least one.
    This is a partial order; there is no numeric score.
  - Evaluation order:
    1. locked Denies;
    2. ordinary resolution;
    3. hard limits.
  - Full algorithm, decision table, publish-time validation, trace and tests: `04` §7.
- **Hard limits** are applied after rule resolution and are not rules:
  - host-local ceiling (RW-12);
  - `Destructive` never `Allow` in v1 (D-06);
  - targets outside pinned subjects ∪ rule-named targets (D-07);
  - unknown or stale safety-critical facts;
  - containment management-path requirements (D-28);
  - pause and circuit breaker.
- **Pure and deterministic.** `domovoi-engine::policy::evaluate` does no I/O.

## RW-11 Authorization types and their enforcement in Rust

- **Correction (FACT).**
  - `HumanApproval` is defined in `domovoi-core` (`core/src/action/approval.rs:54`) with fields
    `user`, `channel`, `at`, and with `pub fn new` and `pub fn restore`.
  - Typed confirmation is part of `ApprovalChannel::Web { typed_confirmation }` (`approval.rs:35-38`).
  - There is no `Default` and no `Deserialize`.
  - Call sites are restricted by a **line-based text scan** in `xtask arch-check`
    (`xtask/src/arch/scan.rs`, `rules.rs:169-175`), not by the compiler.
  - `PlanHash` v1 = SHA-256 over `plan:v1\n{skill}\n{manifest_hash}\n{host_id}\n{canonical args}`
    (`approval.rs:16-27`).
- **Why no cross-crate seal.** Rust privacy is per crate. The engine executes and cannot depend on
  the hub (R1). So these types live in `domovoi-core`, and their constructors are `pub`.
- **Decision (PROPOSAL): `HumanApproval` stays the authenticated-human fact.**
  - Its fields are unchanged: `user`, `channel` (incl. typed confirmation), `at`.
  - Plan and session binding live in the **execution detail**, written in the same transaction as
    the approval:
    - `approved_plan_hash` (the hash the approval surface displayed and the request carried);
    - `approval_session_ref` (opaque session id hash, or chat interaction id);
    - `approval_client` (IP for web, platform user for chat).
  - **Proof of exact plan.**
    1. The approve request carries `plan_hash`.
    2. The operation writer accepts it only if it equals the execution's stored `plan_hash_v2`,
       which was computed at proposal from the immutable intent.
    3. At dispatch, `plan_hash_v2` is recomputed from the current registry and the resolved target
       identity (RW-34).

    Any difference (contract version, implementation hash including script bytes, target identity,
    args) means the execution is `Refused` (before dispatch) with "plan changed; new approval required".
- **Other types (core).**
  - `PolicyPermit { decision_id, rule_ids, policy_version, plan_hash, issued_at, expires_at,
    ceiling_hash }`.
  - `enum Authorization { Human(HumanApproval), Policy(PolicyPermit) }`.
  - No `Default`, no `Deserialize`.
- **Who constructs.**
  - `HumanApproval::new`: `domovoi-hub/src/auth/`.
  - `PolicyPermit::issue`: `domovoi-hub/src/policy/`, behind the `PermitIssuer` port (core trait,
    hub implementation, called by the operation writer).
  - `restore` for both: `domovoi-store/src/`.
- **Enforcement (CI, not runtime).**
  1. `clippy.toml` `disallowed-methods` for the four constructors. Clippy resolves paths, so
     renamed imports are caught.
  2. An arch rule allowing `#[expect(clippy::disallowed_methods)]` only under the allowed paths.
  3. The existing text scan.
  4. An arch check that `domovoi-testkit` is only ever a dev-dependency.
- **Persistence.** `executions.authorization_kind` plus the variant's columns and the binding
  columns above; the store rebuilds the value with `restore`.
- **Honesty.** These controls stop careless code paths. They are not a runtime boundary against a
  compromised hub. Runtime limits are the plan hash, the agent's local checks (RW-12, RW-38) and the
  audit chain (RW-27). Threat table: `04` §8.4.

## RW-12 Host-local unattended ceiling (D-03, D-18)

- **DECISIONS.**
  - D-03: owner-controlled config on the host; the hub cannot modify or widen it; the UI shows it
    read-only; hub policy may only narrow it; the agent enforces it independently; owner-signed
    bundles after v1.
  - D-18: full scope —
    - exact capability id;
    - explicit targets;
    - max rate;
    - declared constrainable args;
    - `max_ttl` and `max_active` for containment;
    - mandatory management-path exclusions.

    No allow entry means no policy-authorized execution.
- **Format.** `04` §8.3.
- **What the agent receives.** `authorization_kind = human | policy`. This is a **label set by the
  hub, not proof**.
  - **`policy`-labelled requests** must pass:
    - the complete ceiling;
    - the mutation rate limiter (RW-38);
    - catalogue, manifest, args and `allow_risk` checks.
  - **`human`-labelled requests** must pass:
    - catalogue, manifest, args and `allow_risk` checks;
    - the mutation rate limiter.

    They do **not** pass the ceiling.
- **Accepted v1 limitation (stated plainly).** A compromised hub can label a request `human` and so
  bypass the ceiling. It is then limited only by `allow_risk`, the local catalogue and the rate
  limiter. The ceiling is not cryptographic proof of owner authorization.
- **Planned (deferred, see `06` Deferred).** Owner-signed approvals and policy bundles close this
  gap.

## RW-13 L3 containment in v1 (D-04, D-17, D-22, D-28)

- **DECISIONS.**
  - D-04: `Contain` is a general v1 policy class; L3 ships as M9; offline permits are after v1.
  - D-17: the first and only v1 containment capability is `firewall.block_source`. `container.pause`
    and `unit.stop_temporary` are **deferred** until independent expiry and recovery are proven
    (acceptance checklist in `04` §9.3).
  - D-22: every containment expires without depending on the main agent staying alive.
    Connectivity-affecting containment also needs an independent watchdog. A capability cannot be
    enabled for unattended use until expiry, recovery, watchdog, outcome-unknown handling and drills
    are implemented.
  - D-28: a host cannot enable unattended containment until its management paths are
    owner-confirmed and current.
- **Mechanism (PROPOSAL).**
  - `firewall.block_source` adds an element to a dedicated nftables set with a kernel `timeout`.
  - It goes through a narrow first-party helper (RW-39).
  - A 30-second confirmation watchdog (`04` §9.1) removes the element if the hub's post-apply
    reachability confirmation does not arrive.
- **Triggering.** Only a deterministic detector's typed signal names the target, and the target must
  be the pinned subject. Model output may investigate and recommend; it never triggers or targets
  automatic containment.

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
  startup integrity check, nightly `VACUUM INTO` keeping 7 copies, `pre-migrate-*` never pruned (C).
- **Decision.** Switch to `synchronous=FULL` **in F1**, together with the unit of work and the audit
  chain.
  - **Why then.** Approvals, audit and the hash chain must survive power loss from F1 on, not only
    once the execution journal exists.
  - **Cost.** One extra fsync per commit. That is milliseconds, at a write rate of a few commits per
    second at most (INFERENCE from current workload).
- **Also:**
  - startup `quick_check`;
  - backup verification (open the copy and run `quick_check`);
  - `domovoi restore`;
  - pre-migrate pruning to the last 3 (D-21).
- No vector database, graph database, broker or workflow engine.

## RW-17 Protocol compatibility: exact protocol version (D-20)

- **FACT.**
  - `PROTOCOL_VERSION` is 1.1 (`proto/src/version.rs:17`).
  - Today the hub accepts the same minor or one behind, and the agent treats a reject as final
    (`version.rs:22`).
  - A release-number gate also exists (`MIN_AGENT_VERSION` / `MIN_HUB_VERSION` 0.0.5).
  - `#[serde(other)]` at `messages/handshake.rs:246`, `messages/work.rs:110,207` discards unknown
    values.
- **DECISION D-20.**
  - The hub accepts an agent only when protocol **major and minor both match**. Older and newer
    minors are rejected.
  - Application patch releases with the same protocol version are compatible.
  - There is no previous-minor adapter and no legacy execution path.
  - A minor change is a coordinated upgrade: hub first, then agents through the supported install
    procedure. Agents not yet upgraded cannot reconnect or execute.
  - An incompatible agent stays visible as `Incompatible` with "upgrade required". It is not shown
    as offline, and its history and inventory are kept.
  - Agent self-update stays out of scope (INV-3).
- **Decision (PROPOSAL).**
  - The release-number gate is removed from compatibility. A separate **revoked-build list** (hub
    config) may refuse specific agent builds for security reasons. It is deferred unless needed, and
    it is distinct from protocol compatibility.
  - `reject{reason: protocol_mismatch, hub_protocol, agent_protocol}` is final on the agent, which
    logs an upgrade instruction.
  - `features` remains, for optional build or platform capabilities (e.g. nftables helper present),
    never for older protocol generations.
  - Tolerant decoding with `Open<T> = Known(T) | Unknown(String)` (custom `Deserialize` that keeps
    the raw value) still matters: within one protocol version, enums in data (contract ids, effect
    classes, journal kinds) can carry values newer than a reader's build only through skill or
    package content, and a bad value must not drop a session.
  - Catalogue by hash plus paged `get_catalog`.
  - New messages `run_capability` and `action_status`.
  - Golden frames for the current protocol are committed; a minor bump replaces them deliberately.

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
    postcondition check. Docker write access for the former needs its own helper ADR (D-24, RW-39); until then `container.restart` is not available unattended.

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

## RW-26 Capability packages and UI installation (D-11, D-16)

- **FACT.** Skills are owner-placed files in `skills_dir` (`config/src/agent.rs:52`). The hub cannot
  write files on a host. Agent self-update is out of scope (INV-3, ADR 0003).
- **DECISION D-16.** v1 trusts:
  - first-party packages signed by the Domovoi release key;
  - owner-signed packages, once the owner installs their public key locally on the agent.

  There are no third-party registries in v1. The hub cannot add or replace agent keys.
- **Format and lifecycle (PROPOSAL, `04` §11.2).**
  - A `.tar.zst` archive.
  - A **detached** signature file whose Ed25519 signature covers a canonical signing message: format
    version, package name, version, archive SHA-256, key id.
  - The agent verifies signature, digest, identity, compatibility and path containment **before
    extracting anything**.
  - A rollback must also be signed by a currently trusted key and compatible. Having been installed
    before is not trust.
- **Trust boundary.**
  - An owner-signed package runs whatever its scripts do, as the agent user, within `allow_risk`.
  - Signing a package is equivalent to the owner placing those files on the host themselves. Its
    risk is the owner's key custody and their review of the package content.
- **Install never enables or authorizes.**

## RW-27 Audit hash chain (D-14)

- **Decision (PROPOSAL; one scheme for new and migrated installations).**
  - **Anchor.** The migration that introduces the chain writes one `audit_anchor` row:
    - `install_id`;
    - `legacy_last_id` (highest pre-chain row id; 0 on a fresh install);
    - `legacy_count`;
    - `legacy_digest` = SHA-256 over the canonical serialization of every pre-chain row, in id
      order (SHA-256 of the empty string on a fresh install).

    Historical rows are never rewritten.
  - **Chain.**
    - The first chained row's `prev_hash` = SHA-256(`"domovoi-audit-anchor-v1"` ‖ install_id ‖
      legacy_last_id ‖ legacy_count ‖ legacy_digest).
    - Each row: `hash = SHA-256("domovoi-audit-v1" ‖ seq ‖ prev_hash ‖ canonical_json(entry))`,
      computed inside the unit-of-work transaction.
  - **Verification** (`domovoi audit verify`, and startup over the last N rows, default 10 000)
    reports two parts separately:
    1. **legacy unchained history**: count and digest match. A mismatch shows that legacy rows
       changed, without saying which.
    2. **chained history**: the first broken link, if any.

    The UI's audit page shows the same two parts.
  - **Pruning.** Writes a chained `checkpoint` row: first retained seq, its `prev_hash`, pruned
    count. Verification starts at the newest checkpoint. Pruning legacy rows records `legacy_pruned`;
    the legacy digest is then reported as "not verifiable (pruned)".
  - **Restore.** A restored database verifies as it was at backup time. The restore itself is the
    first new chained row (`audit.restored{backup_head_hash}`). Off-host head hashes newer than the
    backup show the expected divergence.
- **Threat model.** Detects accidental corruption, edits by tools that do not recompute the chain,
  and mid-chain deletion. It does not stop an attacker who rewrites the whole database and recomputes
  it.
- **Cheap anchor (v1).** The head hash appears in the daily digest and in `domovoi audit head`.
  Stronger anchoring is deferred (`06` Deferred).

## RW-28 Configurable retention (D-09, D-21)

- **DECISION D-21.** Defaults, all configurable:

  | Data | Default |
  |---|---|
  | signals | 30 d |
  | incident timeline raw events | 90 d |
  | operation journal raw events | 180 d |
  | raw execution outputs | 30 d |
  | operations and resolution summaries | retained |
  | audit | retained; pruning only via configured checkpoints |
  | retracted fact content | purged after 30 d |
  | conversations | 180 d |
  | notifications and notification state | 30 d |
  | nightly backups | 7 |
  | pre-migrate backups | last 3 |

- **What deletion guarantees.** Deletion and FTS interaction, backup interaction and structure versus
  raw data: `04` §14.
  - Deleted data stays in backups until those backups rotate out.
  - Deletion removes data from the live database, FTS and model context in the same transaction.

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

  Nothing is rejected because a host is still `Unknown`. A host reported `Incompatible` (RW-17)
  turns its waiting steps `Unavailable` with "agent upgrade required"; its history is kept.

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

## RW-34 Stable target identity

- **Decision (PROPOSAL).** Plans bind a canonical `TargetRef`, resolved at planning time and
  re-validated by the agent immediately before execution:

  | Kind | Canonical identity | Display | Behaviour |
  |---|---|---|---|
  | host | `HostId` plus the agent's enrolled public-key fingerprint | host name | re-enrolment changes the fingerprint, so the plan is invalid |
  | container | `container:{host}/{container_id}` (full runtime id) | name | restart keeps the id (valid); recreate changes the id (refused: `target_changed`); rename keeps the id (valid, display updated) |
  | systemd unit | `unit:{host}/{unit_name}` plus `fragment_path` and SHA-256 of the unit file content at planning time | unit name | changed unit file or a missing unit → refused |
  | network source | `ipnet:{canonical}`: IPv4 dotted, IPv6 RFC 5952, host bits zeroed, explicit prefix | same | prefix must satisfy ceiling bounds |

- Policy rules name targets the way the owner thinks of them (container or unit names). Matching
  resolves the name to the current identity at evaluation time. The permit and plan hash bind the
  resolved identity.
- An inventory refresh never changes an approved plan. It can only make the plan's identity no
  longer resolve, which leads to refusal and a request for new approval.

## RW-35 Effect delivery semantics

- **Decision (PROPOSAL).**
  - **Lifecycle.** `pending → claimed{lease_until, runner_id, attempt} → done | pending{not_before}
    (retryable) | dead{reason} (permanent or attempts exhausted) | cancelled`.
  - **Leases.** An expired lease makes the effect claimable again. Completion happens in the owning
    writer's transaction, together with the state change.
  - **Recovery.** On restart all `claimed` effects revert to `pending`, except `Dispatch`, which is
    reconciled (RW-09).
  - **Delivery guarantee.** At-least-once, never exactly-once.
    - If a remote service accepts a notification and the hub crashes before recording success, the
      notification is sent again.
    - Remote idempotency is used where the platform supports it. Discord's message-create `nonce`
      with `enforce_nonce` is believed to deduplicate within a short window (INFERENCE; verify during
      F2).
    - Otherwise duplicates are possible. Messages carry a stable short id, so a duplicate is
      recognisable.
  - **State recording.** Notification state is recorded only after a confirmed success response. A
    state read error is an error, never default state.
- Detail: `04` §4.1.

## RW-36 Execution identity across the Action → Step transition

- **Decision (PROPOSAL).**
  - The agent-facing idempotency key is an opaque UUID `execution_id`.
  - **In F2/M1 (before operations exist):** `execution_id = ActionId`, and plan hash v2 includes
    `execution_id`.
  - **From M3:** a new Execution step gets `execution_id = step_id`, generated when the step is
    inserted.
  - **The key never changes once dispatched.** Legacy actions keep their `ActionId` and finish on the
    legacy path. No legacy action is re-dispatched under a new id.
  - Agent journal entries are keyed by `execution_id` regardless of origin. The hub's `executions`
    table and the legacy `actions` table are the only two sources, and an id never appears in both
    (UUIDv7 uniqueness plus a check at insertion).
- **Why.** M1 can ship idempotency and reconciliation using objects that exist then, and M3 does not
  have to touch the agent contract.

## RW-37 Management-path confirmation for containment (D-28)

- **DECISION D-28.**
  - Unattended containment on a host requires owner-confirmed, current management-path facts:
    - LAN and VPN ranges;
    - hub address;
    - the agent's current management connection;
    - protected management services.
  - Unknown, stale, conflicting or unavailable information means containment is refused.
  - The agent enforces its mandatory local exclusions independently; the hub cannot remove or widen
    them.
  - The UI shows the configured paths, their provenance and freshness, and which containment
    capabilities are blocked by missing information.
  - This does not affect monitoring or other human-approved capabilities.
- **Procedure (PROPOSAL).**
  1. During host onboarding or from the host's Network/Security tab, the owner enters trusted
     ranges and protected services, or accepts discovered suggestions.
  2. The hub runs a reachability probe (hub → agent link, and owner-specified checks).
  3. The owner presses **Confirm current**.

  Facts are `user`-sourced with `last_verified_at`; freshness defaults to 30 days, configurable.
  Facts become `stale` immediately when the agent reports interface or route changes, or when the
  hub sees the agent from a new address.
- Pack Q0.5 is resolved by this decision.

## RW-38 Agent-side mutation rate limiter (D-27)

- **DECISION D-27.** Applies to all mutating executions, human- or policy-labelled. It is separate
  from the ceiling.
- **Decision (PROPOSAL).**
  - **Config.** Agent `[mutation_limits]`:
    - `per_hour` (global, default 30);
    - optional `per_capability = { "container.restart" = 6 }`.
  - **Counters** are derived from the agent journal's durable `accepted` timestamps for mutating
    contracts. A hub restart cannot reset them.
  - **Clock.** If wall time moves backwards, entries count until their monotonic age (recorded per
    entry) exceeds the window.
  - **Failure.** If the journal is unreadable, mutations are refused (fail closed); reads still work.
  - **Refusal.** `refused{rate_limited, scope, retry_after}`, shown on the approval card and the step.

## RW-39 Narrow first-party privileged helpers (D-24)

- **DECISION D-24.** The agent does not get general Docker socket write access. Docker write
  capabilities stay unavailable for unattended use until the helper ADR and its implementation exist.
- **Decision (PROPOSAL).**
  - Privileged operations go through small first-party helper programs, shipped as first-party
    packages:
    - `domovoi-docker-helper` supports `restart|pause|unpause <container-id>` only;
    - `domovoi-nft-helper` supports `add|del|list` on the `inet domovoi` table's block sets only.
  - **Privilege path.** The agent invokes a helper through the runner (argv) via a root-owned
    `sudoers` rule restricted to the exact helper path, or via a file capability (`CAP_NET_ADMIN`
    for the nft helper). The agent user is not in the `docker` group.
  - **Helper checks.**
    - The helper reads its own allowlist (root-owned file).
    - It refuses any operation, table or target outside it.
    - It never accepts free-form flags.
  - Each helper needs its own ADR (privilege analysis, file ownership, package trust, update path).
  - **Tests** prove refusal of unrelated operations:
    - `docker_helper_refuses_exec`;
    - `docker_helper_refuses_unlisted_container`;
    - `nft_helper_refuses_foreign_table`.

## RW-40 Step graph: parentage plus typed dependency edges

- **Decision (PROPOSAL).**
  - `parent_step_id` is grouping and presentation only.
  - Ordering and causality use typed edges in `step_edges(from_step, to_step, kind)`, directed from
    dependent to prerequisite. Kinds:
    - `depends_on`: cannot start before `to` finishes;
    - `caused_by`: created from the output of `to`;
    - `verifies`: a Verification step pointing to an Execution step;
    - `retry_of`: a new Execution step pointing to the earlier one;
    - `answers`: a reply pointing to a Question step.
  - **Acyclic by construction.** Edges may only point to steps already committed in the same
    operation (checked in the writer). Investigation cycles are new steps with `cycle + 1`, never
    graph cycles.
  - **Retention.** Steps and edges are kept as long as their operation; only raw outputs and raw
    events are pruned (D-21). The graph and timeline are always reconstructable from the same trace
    contract (`04` §13).

## RW-41 Resolution summary provenance per field

- **Decision (PROPOSAL).**
  - Summaries are stored as append-only field revisions:
    `resolution_summary_fields(operation_id, field, revision, value, source{code|model|user}, actor,
    created_at)`.
  - Code-owned fields (`outcome`, `verified`, `subjects`, `contracts_executed`, `hosts`) accept only
    `source=code`.
  - Narrative fields (`problem`, `cause`, `fix`) take `model` drafts and `user` edits or
    confirmations. The latest revision is current; history is kept.
  - FTS indexes current narrative values. Search results and model context show each field's source.
