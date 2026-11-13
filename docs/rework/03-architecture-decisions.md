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
| 17 | Protocol compatibility: exact major.minor, authenticated durable Incompatible state | DECISION D-20 / PROPOSAL (detail) |
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
| 35 | Effect delivery semantics, lease fencing | PROPOSAL |
| 36 | Execution identity across Action → Step | PROPOSAL |
| 37 | Management paths: helper-owned exclusions, signed attestation, owner confirmation | DECISION D-28 / PROPOSAL (design) |
| 38 | Agent-side mutation rate limiter | DECISION D-27 / PROPOSAL (format) |
| 39 | Three identities: control agent, isolated skill runner, root helper | DECISION D-24, D-29 / PROPOSAL (design) |
| 40 | Step graph: parentage plus typed edges | PROPOSAL |
| 41 | Resolution summary provenance; operation writer owns it | PROPOSAL |
| 42 | Protocol bump schedule: one coordinated v1 wire bump in M1 | PROPOSAL |

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
  `Deny > RequireApproval > Allow`. Publication shows each such conflict, and each equal-scope
  conflict (which resolves to Deny), and requires an explicit acknowledgement. Runtime and simulator use the same function.
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
  - agent running as root (D-29 not yet in effect): no unattended execution.
- **Pure and deterministic.** `domovoi-engine::policy::evaluate` does no I/O.

## RW-11 Authorization types, approval binding and their enforcement in Rust

- **Correction (FACT).**
  - `HumanApproval` is defined in `domovoi-core` (`core/src/action/approval.rs:54`) with fields
    `user`, `channel`, `at` and `pub fn new` / `pub fn restore`.
  - Typed confirmation is in `ApprovalChannel::Web { typed_confirmation }`.
  - There is no `Default` and no `Deserialize`.
  - Call sites are restricted by a line-based text scan in `xtask arch-check`
    (`xtask/src/arch/scan.rs`, `rules.rs:169-175`), not by the compiler.
- **Plan hash versions.** This defines exactly which plan elements each version binds:

  | | v1 (FACT, `approval.rs:16-27`) | v2 (M1) |
  |---|---|---|
  | Format | `plan:v1\n{skill}\n{manifest_hash}\n{host_id}\n{canonical args}` | `plan:v2\n{contract}@{version}\n{impl_hash}\n{target identity}\n{canonical args}\n{execution_id}` |
  | Entry-script bytes | **not bound** (manifest only) | bound (`impl_hash` = SHA-256 over manifest + entry-script bytes) |
  | Target | host only | resolved target identity (RW-34) |
  | Execution identity | not bound | `execution_id` (RW-36) |
  | Recomputed at dispatch | yes (INV-4) | yes, from the current registry and a fresh target resolution |

- **Stages (decision, PROPOSAL).**
  - **F4 (current Action flow, hash v1).**
    - The approval card renders the structured plan.
    - The approve request carries the v1 plan hash shown on the card.
    - The action writer refuses if it differs from the action's stored v1 hash.
    - Dispatch recomputes v1 as today.

    F4 must not claim script-byte or target-identity binding.
  - **M1 (Action flow, hash v2).**
    - New actions compute v2.
    - Approval binding columns are added to the action: `approved_plan_hash`,
      `approval_session_ref`, `approval_client`.
    - The approve request must carry the v2 hash; dispatch recomputes v2.
    - Actions created before the upgrade keep v1 semantics until they finish or expire, labelled
      "v1 binding" in the UI.
  - **M3 (Operation/Step model).**
    - The approval endpoint moves to `POST /operations/{id}/steps/{step}/approve { plan_hash,
      typed_confirmation? }`.
    - The binding columns move to the execution detail; `execution_id = step_id`.
    - The legacy action endpoint stays only for legacy actions (RW-32).
- **Types (core).**
  - `HumanApproval` is unchanged.
  - `PolicyPermit { decision_id, rule_ids, policy_version, plan_hash, issued_at, expires_at,
    ceiling_hash, safety_identity? }` (`safety_identity` required for `Contain`, RW-37).
  - `enum Authorization { Human(HumanApproval), Policy(PolicyPermit) }` (M1 introduces it with only
    `Human` usable).
  - No `Default`, no `Deserialize`.
- **Who constructs.**
  - `HumanApproval::new`: `domovoi-hub/src/auth/`.
  - `PolicyPermit::issue`: `domovoi-hub/src/policy/`, via the `PermitIssuer` port.
  - `restore` (both): `domovoi-store/src/`.
- **Enforcement (CI, not runtime).**
  - `clippy.toml` `disallowed-methods` (path-resolved).
  - An arch rule restricting `#[expect(clippy::disallowed_methods)]` to the allowed paths.
  - The existing text scan.
  - A testkit dev-only check.
- **Honesty.** Not a runtime boundary against a compromised hub. Runtime limits are the plan hash,
  the agent's local checks (RW-12, RW-38), the helper allowlist (RW-39) and the audit chain (RW-27).

## RW-12 Host-local unattended ceiling (D-03, D-18)

- **DECISIONS.** D-03 and D-18 as recorded in `99`. Format: `04` §8.3.
- **Where each part lives.** The mandatory management-path exclusions D-18 lists belong to the same
  host-local, owner-controlled safety configuration. They are stored in the root-owned helper
  `[safety]` section and enforced by the helper (RW-37). The agent `[unattended]` table holds the
  rest of the ceiling.
- **What the agent receives.** `authorization_kind = human | policy`, a hub-set label, not proof.

  | Check | `policy` | `human` |
  |---|---|---|
  | complete unattended ceiling | yes | no |
  | helper-enforced exclusions and containment limits (root-owned `[safety]`, RW-37) | yes | yes |
  | mutation rate limiter (RW-38) | yes | yes |
  | local catalogue, manifest, args, `allow_risk` | yes | yes |
  | helper allowlist, for privileged operations (RW-39) | yes | yes |

- **Process privileges (D-29, RW-39).** The control agent runs as `domovoi-agent`. Skills and packages
  run through the skill runner as `domovoi-skill`, which cannot reach the helper or agent state.
  Privileged operations exist only through the root-owned helper and its owner-edited allowlist.
- **Accepted v1 limitation.** A compromised hub can label a request `human` and bypass the ceiling.
  It is still limited by:
  - the catalogue and manifest checks;
  - `allow_risk`;
  - the rate limiter;
  - the helper allowlist and helper-enforced safety constraints (RW-39).

  The ceiling is not cryptographic proof of owner authorization. Owner-signed approvals and bundles
  are DEFERRED (`06`).

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
  - `firewall.block_source` adds an element with a kernel `timeout` to a set in the dedicated
    `inet domovoi` table, through the root-owned helper (RW-39).
  - The element is first added with a 30 s timeout. After the hub's post-apply reachability
    confirmation it is replaced with the full TTL by an atomic delete + add in one nftables
    transaction, so the block is never absent (`04` §9.1). Without confirmation it expires by itself.
  - Production readiness depends on research R-2.
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

- **FACT** (baseline source):
  - `PROTOCOL_VERSION` is 1.1 (`proto/src/version.rs:17`).
  - `compatible()` accepts the same major and an agent minor equal to or one behind the hub's
    (`version.rs:22-26`).
  - The hub compares protocol versions **before** the challenge (`hub/src/agent_server/handshake.rs:165`),
    so that refusal is sent to an unauthenticated client and is not recorded.
  - The release floor `MIN_AGENT_VERSION` is checked only after the signature verifies. Its refusal
    is recorded with `registry.record_refusal`, which is **in memory** (`handshake.rs:194-212`,
    `registry.rs:142`).
  - The agent checks `MIN_HUB_VERSION` against `welcome.hub_version` (`agent/src/link/handshake.rs:66`).
  - `RejectReason` has `Incompatible { hub }`, `AgentTooOld { minimum }`, and `#[serde(other)] Unknown`
    (`proto/src/messages/handshake.rs:226-248`).
  - The agent treats `Incompatible`, `AgentTooOld`, `HubTooOld` and `Revoked` as final
    (`agent/src/error.rs:93-101`), and its link loop returns the error (`agent/src/link/mod.rs:163-170`).
  - Under the shipped unit (`Restart=always`, `RestartSec=5`), INFERENCE: a final reject makes the
    process exit and systemd restarts it every 5 s, so the agent reconnects endlessly.
- **DECISION D-20.**
  - Exact major.minor match; no backward or forward window.
  - Patch releases with the same protocol interoperate.
  - A mismatched agent is `Incompatible` (not offline) and keeps its history.
  - Operations on it report `Unavailable` with reason `agent_incompatible`.
  - No endless reconnect loop.
  - Coordinated upgrade: hub first, then agents. No self-update.
- **Decision (PROPOSAL).**
  - **Handshake order (hub).**
    1. Receive `hello`.
    2. Look up the agent; refuse `UnknownAgent` / `Revoked` as today. These are not persisted per
       host.
    3. Send the challenge.
    4. Verify the signature with the enrolled key.
    5. Compare protocol versions with the exact rule.
    6. On mismatch, persist the authenticated report.
    7. Reject with the existing `RejectReason::Incompatible { hub }`.

    Nothing about protocol is persisted before step 4.

    INFERENCE from the existing post-challenge `AgentTooOld` path: agents already handle a reject
    after the challenge, so no new reject variant is needed. `Incompatible` is already final on
    every released agent.
  - **Durable record.** The `agents` row (store) gains:
    - `reported_protocol`;
    - `reported_release`;
    - `last_authenticated_at`;
    - `incompatible_since` (null when compatible).

    It is written in a unit-of-work transaction with an audit entry, and only when the values
    change, so a restart loop does not flood writes.

    The UI derives the host state from the row after any restart:
    - `Incompatible` if `reported_protocol != hub PROTOCOL_VERSION`;
    - "Expected incompatible (last reported X, hub requires Y)" if the agent has not reconnected
      since a hub upgrade changed the hub's protocol.

    The in-memory `record_refusal` path is removed.
  - **No reconnect loop (agent, from the release that ships this).** On `Incompatible` the agent
    stays running in an idle incompatible state:
    - no reconnect attempts;
    - a log line on entry and hourly;
    - `domovoi agent status` reports it.

    The process does not exit, so systemd and container restart policies do not loop. Upgrading the
    binary and restarting the service ends the state.

    Agents released **before** this change still exit on the final reject and are restarted every
    5 s by systemd until upgraded. The hub tolerates this: it writes only on change and logs once
    per hour per host.
  - **Release floors.** `MIN_AGENT_VERSION` and `MIN_HUB_VERSION` stop being compatibility
    dimensions. Both checks are removed in F3, and the hub no longer sends `AgentTooOld`. The
    variant stays in the wire enum so it still decodes.

    A future security revocation of specific builds is DEFERRED (`06`). If built, it reuses
    `AgentTooOld { minimum }`, which released agents already treat as final, and it is a separate
    mechanism, never a protocol-compatibility rule.
  - **Tolerant decoding.** `Open<T>` (custom `Deserialize` that keeps the raw string) applies to
    data enums whose values can come from skills or packages (contract ids, refusal codes, journal
    kinds). Protocol message types are fixed per protocol version.
  - **Version bumps and wire changes** are scheduled in RW-42.

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
- **DECISION D-16.**
  - v1 trusts the first-party release key and owner keys installed locally on the agent.
  - No third-party registries.
  - The hub cannot add or replace keys.
- **Sources (PROPOSAL).**
  - **First-party:** the signed index at the configured first-party URL.
  - **Owner-signed:** imported manually. The owner uploads `<pkg>.tar.zst` and `<pkg>.sig.toml` in
    the Capability Center (Import package) or runs `domovoi package import <archive> <sig>` on the
    hub. The hub keeps them in a local package store under its data directory.

  No other discovery exists in v1.
- **Format, limits and verification** are in `04` §11.2:
  - detached signature over a canonical message;
  - archive limits;
  - no links;
  - verification before extraction;
  - the signer `key_id` persisted per installed package, so revocation finds affected packages;
  - rollback requires a currently trusted key.
- **Privileges.**
  - Packages install into `packages_dir` (default `/var/lib/domovoi-packages`), written by the agent
    and read-only to `domovoi-skill`.
  - They execute only through the skill runner as `domovoi-skill` (RW-39): never as root and never
    as `domovoi-agent`.
  - They cannot write the helper, the skill-runner units or `/etc/domovoi/*`, because those paths
    are root-owned and the installer only writes `packages_dir/<pkg>/`.
  - Signing grants execution as `domovoi-skill` only.
  - Helper installation is a separate, owner-controlled OS install step.
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

- **Decision (PROPOSAL).** Plans bind a canonical `TargetRef` resolved at planning time. The agent,
  through the helper where privileged, re-resolves it immediately before execution.

  | Kind | Canonical identity bound to the plan | Changes that invalidate | Supported scope |
  |---|---|---|---|
  | host | `HostId` + enrolled agent key fingerprint | re-enrolment | all |
  | container | `container:{host}/{container_id}` (full id); the name is display only | recreate (new id) | containers visible through the helper |
  | systemd unit | `unit:{host}/{canonical_id}` + `config_hash` = SHA-256 over the unit's `FragmentPath` contents and every `DropInPaths` file, as (path, content) pairs sorted by path | any change to the fragment or a drop-in; an alias now resolving to a different unit | units whose `FragmentPath` is under `/etc/systemd/system`, `/usr/lib/systemd/system` or `/lib/systemd/system`. Generated (`/run/systemd/generator*`) and transient units are not valid mutation targets in v1 |
  | network source | `ipnet:{canonical}`: IPv4 dotted, IPv6 RFC 5952, host bits zeroed, explicit prefix | — | prefix within ceiling bounds |

- **Not bound.** Files a unit references (`EnvironmentFile=`, `ExecStart` binaries) and container
  image content. A change to those does not invalidate an approval. The approval card says what is
  bound.
- **Name resolution.**
  - Container names are validated against Docker's name grammar (`[a-zA-Z0-9][a-zA-Z0-9_.-]+`)
    before resolution.
  - All later operations use the id.
  - Policy rules name targets by name; matching resolves the name at evaluation time; permits and
    plan hashes bind the identity.
- **Lifecycle.** An inventory refresh never changes an approved plan. If identity no longer resolves
  to the bound value, the step is refused with `target_changed` and needs new approval. A container
  restart keeps its id; a rename keeps its id.

## RW-35 Effect delivery semantics and lease fencing

- **Decision (PROPOSAL).**
  - **Lifecycle.** `pending → claimed → done | pending(backoff) | dead | cancelled`.
  - **Claim.** A claim sets:
    - `claim_token` (fresh UUID);
    - `claim_generation += 1`;
    - `lease_until`;
    - `runner_id`;
    - `attempt += 1`.
  - **Fencing.** Every completion, retry-reschedule or dead-letter is conditional:
    `UPDATE effects … WHERE id = ? AND claim_token = ? AND status = 'claimed'`. It runs inside the
    owning writer's unit-of-work transaction together with the state change. Zero rows means a
    stale lease holder: the whole transaction rolls back, the stale result is discarded and logged,
    and no local state changes.
  - **Leases.** An expired lease lets another runner reclaim the effect, with a new token. The old
    holder can no longer commit anything.
  - **Restart.** All `claimed` effects revert to `pending` with a new generation. `Dispatch` effects
    are instead resolved by reconciliation (RW-09).
  - **Remote delivery is at-least-once.** A stale holder may already have sent a message. Duplicates
    to remote systems are accepted and documented. Remote idempotency is used where supported (R-4).
    A stale holder can never commit local state.
  - **Notification state** is recorded only after a confirmed success response. A read error is an
    error.
- **Test.** `stale_lease_holder_cannot_complete_reclaimed_effect`.

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

## RW-37 Management paths: helper-owned exclusions, signed attestation, owner confirmation (D-28)

- **DECISION D-28** as recorded in `99`. Everything below is PROPOSAL. **R-10 stays open** until a
  spike validates it; nothing here is implemented.

- **Ownership:**

  | Concern | Owner | Enforced by |
  |---|---|---|
  | unattended capability ceiling | `/etc/domovoi/agent.toml` `[unattended]` | agent (RW-12) |
  | general mutation rate limiter | agent journal | agent (RW-38) |
  | mandatory management-path exclusions | root-owned `/etc/domovoi/helper.toml` `[safety]` | helper |
  | containment hard maxima (TTL, active count) and active-count accounting | `[safety.containment]` + helper state | helper only. Agent values may only be stricter (`04` §8.3) |
  | effective safety report | computed and signed by the helper | relayed unchanged by the agent |
  | owner-confirmed management paths | hub knowledge | confirmable only after verified helper coverage |

  The agent config has no exclusion list.

- **Invariant.** A management path is owner-confirmed, and can unlock unattended containment, only
  when a **verified, fresh helper attestation** shows it covered. The hub never trusts an
  agent-supplied description of helper state.

### Helper identity and key enrollment

- **Key.**
  - The helper generates an Ed25519 key pair at first start if absent: private key
    `/var/lib/domovoi-helper/attest.key`, root 0600, directory root 0700.
  - It is masked in the agent and skill-runner units.
  - `public_key` = the raw 32-byte Ed25519 public key.
- **Fingerprint and key id.**
  - `fingerprint` = lowercase hex SHA-256 of the 32 raw public-key bytes (64 hex characters). It is
    displayed in groups of four.
  - `helper_key_id` = the first 16 hex characters of the fingerprint. It is a **label only**: it is
    never used to verify a signature or to decide trust.
- **Delivery.**
  - `hello.helper_identity` and `agent_status.helper_identity` carry
    `{ public_key: base64 (standard, padded) of exactly 32 bytes }`. Any other length is rejected.
  - Every `safety_attestation` also carries `public_key`.
  - **The hub computes the fingerprint itself.** It ignores any fingerprint or id the agent sends.
- **Enrollment.**
  1. The hub shows the received key as **unverified**: its fingerprint and "not pinned".
  2. The owner runs the proposed `domovoi helper fingerprint` as root on the host. It prints the
     full fingerprint and the key id, computed from the private key file.
  3. The owner compares the two out of band and confirms with step-up.
  4. The hub **pins the exact 32-byte public key** for that host (`helper_keys(host_id, public_key,
     fingerprint, pinned_at, pinned_by)`), audited as `helper.key_pinned`.

  Nothing is pinned automatically, including on first contact.
- **Verification.** An attestation verifies only when **both** hold:
  - `public_key` in the message equals the pinned key byte for byte;
  - `ed25519::verify_strict(pinned_key, signing_bytes, signature)` succeeds.

  A matching key id with a different key is a mismatch.
- **Key changes.**

  | Event | Behaviour |
  |---|---|
  | `domovoi helper rotate-key` (proposed, root on the host) | new key, generation counter kept |
  | reinstall or lost helper state | new key generated at start |
  | key received ≠ pinned key | host shows "helper key changed: unverified"; every attestation fails; unattended containment blocked; the old pin is kept and never replaced silently |
  | owner re-enrollment | owner compares the new fingerprint locally and confirms (step-up); the pin is replaced; audited `helper.key_rotated` with old and new fingerprints |
  | owner unpin | no pinned key; containment blocked; audited |
  | agent re-enrollment or agent upgrade | helper pin unchanged; a different helper key is a key change as above |
  | host deleted from the hub | pin deleted |

  A compromised agent that substitutes another key only produces a mismatch. It cannot get a key
  pinned without the owner's local comparison.

### Signed report

- **Request.** The hub sends `attest_safety{ nonce }`: 32 random bytes, base64. Each nonce is issued
  for one host, single use, and valid for 60 s. The agent passes the nonce to the helper's
  `safety.attest` operation. The nonce is the helper's only input.
- **Report bytes.** The report is the exact signed byte string. It is UTF-8, LF-terminated
  `key=value` lines in this order. Lines marked `*` repeat zero or more times, sorted bytewise
  ascending, with no duplicates.

  ```text
  format=1
  helper_key_id=<16 hex>
  nonce=<64 hex>
  boot_id=<lowercase uuid from /proc/sys/kernel/random/boot_id>
  generation=<u64 decimal, no leading zeros>
  config_hash=<64 hex: SHA-256 per 04b §3 (effective config incl. mandatory units and defaults)>
  route_state_hash=<64 hex: SHA-256 per 04b §3 (route lines)>
  complete=<true|false>
  incomplete_reason=<token or empty>
  computed_at=<RFC 3339 UTC, seconds, Z>
  cidr=<canonical CIDR>*                  configured management CIDRs
  protected_unit=<canonical unit id>*
  protected_container=<container name, percent-encoded>*
  max_ttl_s=<u64>
  max_active=<u64>
  hub_addr=<canonical IP>*
  link_peer=<canonical IP>|<port>*
  local_addr=<canonical IP/prefix>|<ifname>*
  route=<family>|<table>|<dst canonical CIDR>|<gateway IP or ->|<ifname>|<metric>*
  rule=<family>|<priority>|<selector>|<table>*
  endpoint_route=<endpoint IP>|<table>|<gateway IP or ->|<ifname>|<src IP>*
  ```

  - **Field grammar, canonical values and hash inputs:** `04b`. Every field is strictly
    percent-encoded, so `|` and LF cannot occur inside a value. `protected_container` carries the
    configured container name, not an id; ids are resolved at apply time. `config_hash` and
    `route_state_hash` use their own domain prefixes, and `04b` §4 has the vectors.
  - **Signing bytes.** `"domovoi-safety-attest-v1\n"` ‖ the report bytes.
  - **Message.** `safety_attestation{ public_key, report: base64(report bytes), signature:
    base64(64 bytes) }`.
  - **Strict parsing.** The hub verifies the signature over the received bytes **first**, then
    parses those same bytes with a strict parser. It never re-serializes them. It rejects:
    - unknown, missing, duplicate or out-of-order keys;
    - unsorted or duplicated repeated lines;
    - non-canonical values;
    - CR characters, trailing whitespace, or a missing final LF;
    - `format` ≠ 1;
    - a `nonce` that does not match an outstanding nonce for this host.
- **Hub acceptance.** The hub accepts the report as the host's **verified safety state** only if:
  - the signature verifies with the pinned key;
  - the nonce is valid;
  - `complete=true`.

  It stores the report bytes and the safety identity (below). Requests happen on `hello`, on
  `agent_status`, every 15 min, on the owner's Re-check, and before permit issuance.

### Route coverage

- **What the helper observes** (via netlink, as root):
  - all IPv4 and IPv6 policy rules;
  - every routing table referenced by a rule, plus `main` and `local`;
  - interface addresses and link state.
- **Relevant route state.** Only these lines appear in the report and in `route_state_hash`:
  - `endpoint_route`: the kernel route-lookup result (equivalent to `ip route get`, honouring policy
    rules) for each **management endpoint**:
    - every `hub_addr`;
    - every `link_peer` address;
    - the network address of every configured `cidr`.
  - `route`: every route that one of those lookups selected, plus every default route in any
    referenced table.
  - `rule`: every policy rule that can select one of those tables.
  - `local_addr`: addresses on interfaces named by any `endpoint_route`.
- **What does not count.** Routes and addresses not involved in reaching a management endpoint are
  not attested. Changing them does **not** invalidate confirmation. The docs and UI must not claim
  that "any route change" invalidates.
- **Incomplete data.** If rules, tables, lookups or addresses cannot be read, or a lookup fails,
  then `complete=false` with a reason, and the helper refuses all containment.
- **Coverage check (hub, pure, against verified data only).** Candidate paths come from:
  - hub-observed agent addresses;
  - agent-reported inventory, which is untrusted and used only to propose candidates;
  - owner entries.

  A candidate is **covered** iff its CIDR is a subset of some `cidr` line, or equals a `hub_addr`
  or `link_peer` address, which the helper always excludes. Agent-reported interfaces or routes
  never make anything covered.

### Safety identity and generation

- **Safety identity** = (`fingerprint` of the pinned key, `boot_id`, `generation`, `config_hash`,
  `route_state_hash`).
- **`generation`** is a u64 that counts changes of the **effective safety set**: the config lines and
  route lines of the report, which together produce `config_hash` and `route_state_hash`.
  - Adding or removing a containment element in the nft set does **not** change it.
  - It is incremented only when the newly computed hashes differ from the last persisted ones.
  - A new `boot_id` with identical content does not increment it.
  - Under one key it never decreases and never repeats.
- **Where the identity lives:**

  | Place | Use |
  |---|---|
  | stored management-path confirmation | the identity it was confirmed against |
  | `PolicyPermit` for `Contain` (new field `safety_identity`) | the identity at issuance |
  | execution detail (`safety_identity`) and approval card | for human-approved containment, the identity shown when approving |
  | `run_capability.expected_safety` (wire 1.2; required for `Contain`) | the dispatch guard sent to the agent |
  | helper `nft` request field `expected_safety` | the comparison at apply time |

- **`expected_safety` type.** A tagged value. It is required on every `Contain` request; a missing
  or unknown tag is refused (`refused{malformed_safety}`).
  - `verified{ fingerprint, boot_id, generation, config_hash, route_state_hash }`
  - `unverified{ reason }`, where `reason` ∈ `key_not_pinned | key_mismatch | attestation_missing |
    attestation_stale | signature_invalid | report_incomplete | generation_regressed |
    path_uncovered | confirmation_stale`.

  No fingerprint or identity is ever fabricated for `unverified`.

- **Verified path** (unattended or human):
  - Requires a pinned key, a verified, complete attestation under 5 minutes old, and current
    confirmations.
  - The full identity is bound to the confirmation, the permit or approval, and the dispatch.
  - The helper compares the full identity with its freshly computed one. A mismatch is refused
    `safety_changed`; unknown state is refused `safety_unknown`.
- **Unverified human path** (human only):
  - Requires an authenticated human approval with step-up **and** typed confirmation.
  - The approval card states that management-path coverage cannot be verified, gives the reason,
    and lists every discovered path that is uncovered or unverified as **not protected**.
  - The hub persists `safety_assurance = unverified{reason, unprotected_paths}` on the execution and
    audits `containment.unverified_approved`.
  - The hub never issues a `PolicyPermit` with `unverified`.
  - The agent refuses `unverified` when `authorization_kind = policy`
    (`refused{unverified_requires_human}`).
  - The helper skips only the identity comparison. It still recomputes its set, refuses overlap,
    enforces maxima and mutation limits, and fails closed on unknown state.
  - **Bounded concern.** The owner accepts uncertainty about coverage; helper enforcement is
    unchanged. A compromised agent could send `unverified` on its own, as with any direct call.
    That is the existing residual risk, and it gains nothing beyond what the helper already allows.

- **Normal (hub-authorized) flow:**
  1. The hub gets a fresh verified attestation (≤ 5 min old).
  2. It checks that every confirmed path is still covered.
     - Each confirmation's stored identity must equal the current identity on fingerprint,
       `generation`, `config_hash` and `route_state_hash`.
     - `boot_id` may differ only when all of those match **and** the current attestation is fresh
       and verified. That is the reboot exception for confirmations; it never applies to permits or
       dispatches.
  3. It issues the permit with the full `safety_identity`, including `boot_id`. For human approvals
     it binds the identity shown on the card.
  4. The agent dispatches with `expected_safety = verified{…}` carrying that full identity.
  5. The helper recomputes its current state.
  6. The helper compares: current ≠ `expected_safety` → `refused{safety_changed}`; it cannot
     compute → `refused{safety_unknown}`; otherwise it checks the target against the actual
     current set and the limits, then applies.
- **Changes between the steps:**

  | Change | Result |
  |---|---|
  | `helper.toml` edited after attestation | `config_hash` and `generation` change; dispatch refused `safety_changed`; confirmations become stale until re-attested and re-confirmed |
  | relevant network state changes | `route_state_hash` and `generation` change; same as above |
  | irrelevant network change | no effect |
  | helper restarts (new instance per connection) | no effect if state is unchanged; `generation` is persisted |
  | host reboot | `boot_id` changes; dispatch refused; re-attest needed. Confirmations survive if `generation` and hashes are unchanged |
  | pinned key changes | no verification possible; no permits; the helper compares its own fingerprint, so an old identity is refused |
  | report older than 5 min at issuance | no unattended permit; a human may approve only on the unverified path |
  | permit issued before a reboot | invalid after it (`boot_id` is part of the permit identity); a new attestation and a new permit are needed |
  | generation changes between verification and apply | `refused{safety_changed}`; the step is `Refused`; the hub re-attests and re-evaluates; any new attempt is a new step with new authorization |
  | helper cannot recompute | `refused{safety_unknown}` |

- **Security limitation (stated plainly).** `expected_safety` is a **consistency guard for the
  honest path**, not proof of a hub permit. A compromised agent can omit or fabricate it and call
  the helper directly. What always holds, whatever the request says:
  - the helper recomputes its **actual** set;
  - it refuses any overlapping target;
  - it enforces its own TTL and active-count maxima and mutation limits;
  - it fails closed when state is unknown.

  A hub-signed permit the helper could verify would be a **separate architectural change**. It
  would need a new helper-side trust root, hub signing-key custody and rotation, clock or nonce
  semantics, and a protocol change. It is **not** part of this design; it belongs with the DEFERRED
  owner-signed approvals (`06`).

### Helper state, recovery and concurrency

- **Files** (all root, directory `/var/lib/domovoi-helper` 0700):
  - `attest.key` (0600): Ed25519 private key.
  - `state` (0600): LF lines `format=1`, `key_fingerprint=<64 hex>`, `generation=<u64>`,
    `config_hash=<64 hex>`, `route_state_hash=<64 hex>`, `boot_id=<uuid>`, then
    `checksum=<64 hex SHA-256 of all preceding bytes>`.
  - `state.lock` (0600): an empty lock file.
- **Serialization.** Every helper instance (one per connection) takes `flock(LOCK_EX)` on
  `state.lock` and holds it across the whole sequence:
  1. read and validate `state`;
  2. recompute config and route lines;
  3. compare;
  4. increment and write if changed;
  5. sign the attestation, **or**, for an apply, compare `expected_safety` and run the nft
     transaction.

  Concurrent instances therefore cannot compute conflicting next generations, and helper state
  cannot change between the comparison and the nft transaction. A kernel route change during that
  short window is an accepted, documented race; the next attestation sees it.
- **Atomic write.**
  1. Write `state.tmp`; `fsync` it.
  2. `rename(state.tmp, state)`; `fsync` the directory.
  3. A leftover `state.tmp` is deleted under the lock at start.
  4. `rename` is atomic, so a reader sees either the old or the new complete file.
  5. A checksum mismatch, a missing field or an unknown `format` means **corrupt**.
- **State conditions:**

  | Condition | Behaviour |
  |---|---|
  | key and state valid, `key_fingerprint` matches the key | normal |
  | state missing, key present | fail closed: attestation `complete=false`, `incomplete_reason=state_missing`, all containment refused; no silent reset |
  | state corrupt (checksum, parse, `format`) | fail closed (`state_corrupt`) |
  | state `key_fingerprint` ≠ key | fail closed (`state_key_mismatch`) |
  | key missing, state present | fail closed (`key_missing`); the helper **never** generates a key on its own |
  | both missing | fail closed (`uninitialized`) until `domovoi helper init` |
  | write interrupted | old file intact (atomic rename); generation not advanced; retried next request; never decremented |

- **Explicit local actions.** These are proposed commands, run as root on the host:
  - `domovoi helper init` (run by the installer): creates the key and `state` with `generation=1`
    if neither exists. It refuses if either exists.
  - `domovoi helper recover`: moves the old key and state to `/var/lib/domovoi-helper/retired/<ts>/`,
    then creates a **new key** and fresh `state` with `generation=1`. Recovery is always a key
    rotation, because generation continuity cannot be proven after state loss.
  - `domovoi helper rotate-key`: also creates a new key and state with `generation=1`.
- **Hub view.**
  - A recovered or rotated helper presents a new public key, so the host shows **"helper key
    changed: re-enroll"** and containment stays blocked until owner enrollment.
  - The hub stores the highest verified `generation` per pinned key. A report with a lower
    generation under the same key is rejected as `generation_regressed`, for example after an old
    state file was restored. Containment is blocked and the owner is alerted.
  - Helper fail-closed reasons show on the host page with the matching local action.

### Owner workflow, semantics and enforcement

- **Workflow.**
  1. The owner edits `[safety]` on the host as root.
  2. The owner presses Re-check.
  3. The hub shows covered and uncovered candidates against the verified state.
  4. The owner confirms covered paths only (step-up). A confirmation records the safety identity.
  5. Uncovered paths show the exact `helper.toml` line to add.

  The hub never writes `helper.toml`.
- **Semantics:**

  | Condition | Behaviour |
  |---|---|
  | helper cannot compute its state | all containment refused (`safety_unknown`); attestation `complete=false` |
  | no verified fresh attestation, key not pinned, key mismatch, invalid signature, incomplete report, generation regressed | unattended containment blocked; human approval only on the **unverified human path** (step-up, typed confirmation, "not protected" list); the helper still enforces its actual set |
  | candidate path uncovered | cannot be confirmed; unattended containment blocked; human approval as in the row above, with the path listed as not protected |
  | confirmation stale (identity changed, other than `boot_id`, or older than 30 d) | unattended containment blocked until re-confirmed |
  | target overlaps the actual helper set | helper refuses regardless of policy, permit or approval |
  | monitoring, unrelated human-approved work | unaffected |

- **Helper enforcement at apply time,** independent of anything in the request:
  1. recompute the effective set;
  2. compare with `expected_safety` when present;
  3. canonicalise the target;
  4. refuse overlap;
  5. enforce protected units and containers;
  6. enforce `max_ttl` and `max_active` (the active count is helper-owned, from its nft set);
  7. enforce helper mutation limits.
- **Residual risk.** A compromised control agent can call the helper directly. It still cannot:
  - block an address inside the helper's actual effective set;
  - exceed the helper's maxima;
  - make the hub believe a path is protected when it is not.

### Tests

- **Key exchange and enrollment:**
  - `helper_public_key_exchange_and_fingerprint`
  - `key_id_never_used_for_verification`
  - `first_contact_does_not_pin_key`
  - `key_substitution_by_agent_is_detected`
  - `key_rotation_blocks_until_reenrolled`
  - `unpin_blocks_containment`
  - `agent_reenrollment_keeps_helper_pin`
- **Signature and report parsing:**
  - `helper_attestation_signature_verifies`
  - `attestation_rejected_for_wrong_key`
  - `attestation_rejected_for_other_host`
  - `helper_attestation_rejects_forged_report`
  - `helper_attestation_rejects_replayed_nonce`
  - `report_parser_rejects_duplicates_unsorted_and_noncanonical`
- **Agent trust:**
  - `agent_cannot_fabricate_effective_safety_set`
  - `hub_rejects_unverified_safety_report`
- **Confirmation and coverage:**
  - `owner_cannot_confirm_uncovered_management_path`
  - `new_management_path_requires_local_helper_configuration`
  - `changed_helper_safety_set_invalidates_confirmation`
  - `stale_attestation_blocks_unattended_containment`
- **Route state:**
  - `nondefault_route_change_for_endpoint_changes_generation`
  - `irrelevant_route_change_keeps_generation`
  - `route_replacement_and_deletion_detected`
  - `multiple_tables_and_rules_observed`
  - `multiple_interfaces_and_gateways_observed`
  - `vpn_tunnel_route_to_hub_observed`
  - `incomplete_route_data_fails_closed`
- **Races between attestation, permit and apply:**
  - `config_change_between_attestation_and_apply_refused`
  - `network_change_between_attestation_and_apply_refused`
  - `reboot_between_permit_and_apply_refused`
  - `generation_change_between_permit_and_apply_refused`
- **Helper enforcement:**
  - `helper_refuses_target_overlapping_exclusion_even_if_agent_requests`
  - `helper_refuses_overlap_without_expected_safety`
  - `helper_fails_closed_when_safety_state_unknown`
  - `helper_attestation_key_not_readable_by_agent_or_skill`
- **Unverified human path:**
  - `human_containment_with_no_pinned_key_uses_unverified_path`
  - `human_containment_with_expired_attestation_uses_unverified_path`
  - `human_containment_with_invalid_signature_uses_unverified_path`
  - `human_containment_with_incomplete_routes_uses_unverified_path`
  - `unattended_containment_refused_in_every_unverified_case`
  - `agent_refuses_unverified_with_policy_label`
  - `exclusion_overlap_refused_even_on_unverified_human_path`
  - `malformed_expected_safety_refused`
- **Helper state:**
  - `generation_state_missing_with_key_fails_closed`
  - `generation_state_corrupt_with_key_fails_closed`
  - `generation_state_key_mismatch_fails_closed`
  - `generation_never_decreases_under_same_key`
  - `concurrent_attestations_serialize_generation_updates`
  - `interrupted_state_write_never_reuses_generation`
  - `helper_state_recovery_requires_explicit_local_action`
  - `key_rotation_invalidates_previous_pin`
  - `hub_rejects_generation_regression`
  - `containment_membership_change_keeps_generation`
- **Encoding (04b vectors):**
  - `config_hash_matches_vectors`
  - `route_state_hash_matches_vectors`
  - `equivalent_toml_same_config_hash`
  - `report_rejects_invalid_encoding_vectors`
- **Freshness:**
  - `permit_from_before_reboot_refused`
  - `confirmation_survives_reboot_only_when_identity_otherwise_equal`
- **Limits:**
  - `agent_limit_stricter_than_helper_applies`
  - `agent_limit_above_helper_is_capped`
  - `helper_active_count_is_authoritative`
- **Approval and unrelated work:**
  - `human_approved_containment_shows_uncovered_paths`
  - `monitoring_unaffected_by_missing_management_paths`

## RW-38 Agent-side mutation rate limiter (D-27)

- **DECISION D-27.** Applies to every mutating execution, human- or policy-labelled. It is separate
  from the ceiling.
- **Decision (PROPOSAL).**
  - **Config.** `[mutation_limits] per_hour` (default 30) and optional `per_capability`.
  - **Entries.** Each `accepted` journal record for a mutating contract carries `execution_id`,
    `wall_time`, `boot_id` (from `/proc/sys/kernel/random/boot_id`; FACT, already read in
    `agent/src/facts.rs:70`) and `mono_ns`.
  - **Age.** Wall time is never trusted for age.
    - **Same boot:** age = `mono_now - mono_ns`.
    - **Entry from an earlier boot:** age is unknown. The entry counts until the current boot's
      monotonic clock shows one full window elapsed since the agent first loaded it after that boot.
      Pre-reboot entries are therefore retained for at least a full window. A reboot can never open
      an unrestricted window.
  - **Wall-clock rollback** has no effect, because ages come from monotonic time.
  - **Duplicates.** Counting is by distinct `execution_id`; a re-sent request counts once.
  - **Corrupt or incomplete state.**
    - Records are length-and-checksum framed. A torn tail record is discarded. This is safe, because
      `accepted` is fsynced before the reply and spawn, so a torn `accepted` was never acknowledged
      or started.
    - Any other corruption: refuse all mutations (fail closed; reads still work) and report
      `journal_corrupt`.
    - Recovery is a local owner action, `domovoi agent journal reset` (root on the host). It starts a
      new epoch and is logged locally and reported to the hub. A reset can at most let a window's
      allowance be used again, and only by local owner action; it never removes the limit.
  - **Refusal.** `refused{code: rate_limited, scope, retry_after}`, shown on the approval card.
- **Tests.**
  - `rate_limit_survives_hub_restart`
  - `rate_limit_holds_across_reboot`
  - `wall_clock_rollback_does_not_reset_limit`
  - `duplicate_execution_counts_once`
  - `corrupt_journal_refuses_mutations`
  - `torn_tail_record_is_discarded_safely`

## RW-39 Three identities: control agent, isolated skill runner, root helper (D-24, D-29)

- **FACT** (baseline source).
  - `install.sh` installs the agent unit running as root (`install.sh:97-106`).
  - Skill manifests may set `run_as` (`core/src/skill/manifest.rs:124-126`). The documented example
    is already `run_as = "domovoi-skills"` (`manifest.rs:79`).
  - The runner switches the child to that uid and primary gid, clearing supplementary groups. This
    works only when the agent is root (`runner/src/exec/mod.rs:40-43`, `exec/spawn.rs:19-34,63-64`).
  - `skills_dir` defaults to the root-owned `/etc/domovoi/skills` (`config/src/agent.rs:51,264`).
  - The agent key and spool live in `state_dir` (`/var/lib/domovoi-agent`, `agent.rs:48`).
- **Problem with the previous revision.** Skills and packages were to run as `domovoi-agent`, the
  same uid the helper accepts by `SO_PEERCRED`. A malicious package could connect to the helper
  directly, skipping plan-hash, approval, ceiling and rate-limit checks. A peer-uid check
  distinguishes users, not processes. **Superseded.**
- **Decisions.**
  - D-24: no general Docker socket write.
  - D-29: the agent runs unprivileged, and privileged operations go only through one narrow
    root-owned helper.
- **Design (PROPOSAL).** Three OS identities:

  | Component | User | Groups | Can reach |
  |---|---|---|---|
  | control agent (`domovoi agent run`) | `domovoi-agent` | `domovoi-agent`, `systemd-journal` | hub link; helper socket; skill-runner socket; its `state_dir` (0700) |
  | skill runner (`domovoi skill-runner`) | `domovoi-skill` | `domovoi-skill` only | read-only package and skill files; its own private tmp; nothing else Domovoi owns |
  | helper (`domovoi helper serve`) | root | — | the allowlisted privileged operations |

  - **Skill runner service.** It is how an unprivileged agent starts skills under another identity.
    - **Socket.** `domovoi-skill-runner.socket` listens on `/run/domovoi-skill-runner.sock`
      (`SocketUser=root`, `SocketGroup=domovoi-agent`, `SocketMode=0660`) with `Accept=yes`.
    - **Service.** Each connection starts one `domovoi-skill-runner@.service` instance with:
      - `User=domovoi-skill`, `Group=domovoi-skill`, `SupplementaryGroups=` empty;
      - `NoNewPrivileges=yes`, `ProtectSystem=strict`, `ProtectHome=yes`, `PrivateTmp=yes`,
        `PrivateDevices=yes`;
      - **Filesystem view.** `ProtectSystem=strict` alone makes `/etc` read-only but still readable,
        and `ReadOnlyPaths=` does not hide anything. So the unit masks whole trees with
        `TemporaryFileSystem=` and re-exposes only skill content with `BindReadOnlyPaths=`:
        - `TemporaryFileSystem=/etc/domovoi:ro /var/lib:ro /run:ro`. These are empty tmpfs mounts:
          the hub, agent and helper config, agent and helper state, and every socket under `/run`
          become invisible and cannot be enumerated.
        - `BindReadOnlyPaths=/etc/domovoi/skills /var/lib/domovoi-packages`. systemd applies binds
          after the temporary filesystems, so only these two trees reappear, read-only.
        - `InaccessiblePaths=/var/lib/domovoi-helper` as a second layer (already hidden by the
          `/var/lib` tmpfs).
      - `RestrictSUIDSGID=yes`, `CapabilityBoundingSet=` empty, `AmbientCapabilities=` empty,
        `RuntimeMaxSec=` = the agent's maximum skill timeout + margin.
      - **Known limitation.** A static unit exposes **all** owner-placed skills and installed
        packages read-only, not only the invoked one. Skill and package content is code, not
        secret: packages are signed artifacts, and the docs and install checks forbid secrets in
        skill directories. Per-invocation narrowing (for example template instances started per
        package with `BindReadOnlyPaths=/var/lib/domovoi-packages/%i`) is part of R-9.
        Unrelated Domovoi config, credentials, agent state and helper config are invisible.
    - **Not root.** The runner itself runs as `domovoi-skill` and never as root, so it cannot raise
      privileges for a skill.
    - **Request.** The agent sends one typed request per connection: `{execution_id, source:
      skills_dir | package{name, version}, skill, impl_hash, args (validated, canonical),
      env (allowlisted names only), timeout, max_output_bytes}`.
    - **Validation by the runner.**
      1. It resolves the skill only inside `skills_dir` or `packages_dir/<name>/<version>/`. There
         are no paths in the request.
      2. It parses the manifest and recomputes `impl_hash` (manifest + entry-script bytes). It
         refuses on mismatch, which closes the gap between the agent's check and execution.
      3. It re-validates the args against the manifest.
      4. It refuses any manifest whose `run_as` names a user other than `domovoi-skill` (O-30).
      5. It builds the argv through `domovoi-runner`. There is no shell (R3/R4) and no executable
         path from the request.
    - **Output.** Stdout and stderr are streamed back over the connection with the existing caps and
      redaction. The result frame carries the exit status and the truncation flag.
    - **Cancellation and timeout.**
      - The runner enforces the timeout with the existing group kill.
      - The agent cancels by closing the connection. On EOF the runner kills the process group and
        exits.
      - `RuntimeMaxSec` is the systemd backstop.
    - **Agent restart.** The connection drops, the runner kills the child, and systemd stops the
      instance. No orphan survives. That gives the "process killed, result unknown" outcome of
      crash window 3 in `04` §15.2.
    - **What a compromised skill cannot reach.**
      - The helper socket and skill-runner socket: wrong group, and `/run` is masked.
      - The agent's key, spool, journal and credentials: `state_dir` is 0700, and `/var/lib` is
        masked except the packages tree.
      - Any file under `/etc/domovoi` other than `skills/`: masked, so it can be neither listed nor
        read.
      - The helper config, helper state and attestation key: masked.
      - The package and skill content: read-only to it.
      - It also cannot replace the executable being run: the files are owned by root (skills) or
        `domovoi-agent` (packages), with no write bit for others.
    - **What a compromised skill can still do.** Anything the `domovoi-skill` user can do on the
      host: outbound network, reading world-readable files. It could also reach the hub's network
      port, but it holds no agent key, so it cannot authenticate as the agent.
  - **Helper.**
    - `domovoi helper serve` runs as root, started by `domovoi-helper.socket` at
      `/run/domovoi-helper.sock` (`SocketGroup=domovoi-agent`, 0660) and a sandboxed
      `domovoi-helper@.service`.
    - It checks `SO_PEERCRED` uid == `domovoi-agent` as **one layer only**. The real isolation is
      that skills never run as that uid.
    - Requests are typed. Every spawn goes through `domovoi-runner`.
    - It never offers `run_as`, `sudo`, `systemd-run` or any command execution.
  - **Packages directory.** Installed packages live in `packages_dir` (default
    `/var/lib/domovoi-packages`): owned by `domovoi-agent`, directories 0755, files 0644 (entry
    scripts 0755). The agent writes it; `domovoi-skill` can only read it. Owner-placed skills stay
    in the root-owned `skills_dir`.
  - **Skill-runner status.** The agent probes the runner (RW-42 `skill_runner_status`):
    - `absent`: socket path missing;
    - `unsupported{reason}`: no systemd or required directives on this platform, detected at agent
      start;
    - `error{reason}`: connect fails, or a `health` request fails or returns an error;
    - `ok{version}`: `health` succeeds.

    The probe runs at start, every 60 s and after any execution transport error. Changes are sent
    with `agent_status`. Anything other than `ok` makes every skill `Unavailable(skill_runner_*)`;
    an execution already in flight when the runner fails ends `OutcomeUnknown`.
  - **Helper-enforced constraints.** These do not depend on the caller being honest. They come from
    `/etc/domovoi/helper.toml` (root-owned) and local OS state:
    - enabled operation groups;
    - allowed targets by canonical identity (container id, canonical unit id + config hash);
    - protected units and containers;
    - mandatory management-path exclusions and the dynamic set (RW-37);
    - containment `max_ttl` and `max_active`;
    - per-operation argument validation;
    - timeouts and output caps;
    - a helper-side mutation rate limit per group (`[limits] mutations_per_hour`), persisted in a
      root-owned state file under `/var/lib/domovoi-helper/`.
  - **Not helper-enforced.** The helper does **not** verify human approval or policy authorization.
    It has no verifiable authorization input in v1. Those checks happen in the agent (plan hash,
    `allow_risk`, ceiling, rate limiter) and the hub.
  - **Layers, kept distinct:**
    1. **Package isolation:** skills run as `domovoi-skill`, with no route to the helper or to agent
       state.
    2. **Root-operation allowlisting:** the helper allowlist bounds which privileged operations and
       targets exist at all.
    3. **Agent-side authorization:** plan hash, `allow_risk`, ceiling and rate limiter.
    4. **Helper-enforced safety:** the list above.
    5. **Residual risk:** a compromised control agent (`domovoi-agent`) can call the helper directly.
       It is then bounded only by layers 2 and 4, not by approval or policy. Owner-signed
       approvals the helper could verify are DEFERRED (`06`).
  - **Operation groups (v1):**

    | Group | Operations | Why privileged |
    |---|---|---|
    | `docker.read` | list, inspect, logs (bounded), stats, resolve name → id | socket access is root-equivalent |
    | `docker.write` | `restart` by allowlisted container id only | D-24 |
    | `systemd.write` | `restart` by canonical unit id + config hash, allowlisted, never protected | unit control needs root |
    | `nft` | `add`, `replace_ttl` (atomic delete + add in one transaction), `del`, `list` on `inet domovoi` sets only; exclusions and limits enforced | CAP_NET_ADMIN |
    | `smart.read` | `smartctl -a` on allowlisted devices | raw disk access |
    | `kernel.read` | bounded kernel messages | `dmesg_restrict` |
    | `net.read` | listening sockets with owning process | other users' processes |
    | `safety.attest` | signed attestation of the effective safety set for a hub nonce (RW-37) | lets the hub verify what the helper enforces without trusting the agent |

    `docker.pause`/`unpause` and unit stop are **not** v1 operations. They belong to the deferred
    `container.pause` / `unit.stop_temporary` candidates (D-17, `04` §9.3). There is no emergency
    variant either: an owner who needs to pause a container does so directly on the host.
  - **Fresh-install default.** Only the read groups and `safety.attest` are enabled, with no write
    targets. `docker.write`, `systemd.write` and `nft` stay unavailable until the owner enables the
    group and its allowlist in `helper.toml` **and** the corresponding milestone prerequisites (M8,
    M9) are met. Examples: `04` §8.3.
  - **Install and update.** Only through the owner-controlled OS path (`install.sh`, root-run
    `domovoi upgrade`, distro package, image). Packages and skills can never install or modify the
    helper, the skill-runner units or `/etc/domovoi/*`: those paths are root-owned, and the package
    installer only writes `packages_dir/<pkg>/`.
  - **Failure modes.**
    - Helper `absent` or `error` → helper capabilities are `Unavailable(helper_missing|helper_error)`;
      privileged checks report "helper unavailable".
    - Skill runner absent → all skills are `Unavailable(skill_runner_missing)`. They do **not** fall
      back to running as `domovoi-agent`, because that would reopen the helper and the agent state
      to package code.
  - **Migration (workstream H1).**
    1. The installer creates `domovoi-agent` and `domovoi-skill` and moves `state_dir` ownership
       (0700).
    2. It creates `packages_dir`.
    3. It installs the helper and skill-runner units and a fresh-install `helper.toml`.
    4. It switches the agent unit to `User=domovoi-agent`.
    5. `domovoi agent selftest` reports lost access and skills affected by O-30.

    Root installs keep working until the owner reruns the installer. The UI labels them
    "privilege boundary not in effect", and they get no unattended execution (hard limit, `04` §7.3).
  - **Platforms.** systemd with socket activation is required for the skill runner and the helper.
    Container deployments, Unraid and hosts without systemd are research item R-9; until it
    concludes, those platforms run without skills, or keep the legacy root agent with the boundary
    labelled absent.
- **Tests.**
  - `skill_cannot_list_or_read_etc_domovoi`
  - `skill_cannot_read_agent_or_helper_state`
  - `skill_cannot_see_run_sockets`
  - `skill_runner_status_absent_ok_error_unsupported`
  - `skill_runner_status_change_reported`
  - `skill_user_cannot_connect_to_helper_socket`
  - `skill_user_cannot_read_agent_state`
  - `skill_user_cannot_write_package_files`
  - `skill_runner_refuses_impl_hash_mismatch`
  - `skill_runner_refuses_path_outside_skill_roots`
  - `skill_runner_kills_child_on_agent_disconnect`
  - `skills_unavailable_without_skill_runner`
  - `helper_rejects_wrong_peer_uid`
  - `helper_refuses_unlisted_container`
  - `helper_refuses_unlisted_unit`
  - `helper_refuses_protected_unit`
  - `helper_refuses_foreign_nft_table`
  - `helper_refuses_unknown_operation`
  - `helper_has_no_pause_operation`
  - `fresh_install_enables_read_groups_only`
  - `helper_mutation_limit_enforced`
  - `package_install_cannot_write_helper_paths`
  - `missing_helper_reports_unavailable`

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

## RW-41 Resolution summary provenance and ownership

- **Decision (PROPOSAL).**
  - Summaries are append-only field revisions belonging to the Operation aggregate:
    `resolution_summary_fields(operation_id, field, revision, value, source{code|model|user},
    actor, created_at)`.
  - **The operation writer is the only writer.** The Knowledge UI and use-case services submit
    `EditSummaryField{operation_id, field, value, expected_revision}` commands to the operation
    writer. It validates the field, rejects `user` edits to code-owned fields, checks the revision,
    and commits the revision, journal entry and FTS update in one transaction.
  - Code-owned fields: `outcome`, `verified`, `subjects`, `contracts_executed`, `hosts`.
  - Narrative fields: `problem`, `cause`, `fix`.
  - Search results and model context show each field's source.
- **Tests.** `code_owned_summary_fields_reject_user_edits`, `summary_edit_goes_through_operation_writer`,
  `stale_summary_edit_rejected`.

## RW-42 Protocol bump schedule: one coordinated v1 wire bump

- **Problem.** Under D-20, every protocol minor change is a coordinated fleet upgrade. Planned wire
  additions span F3, M1, M2, P1, M8 and M9.
- **Decision (PROPOSAL).** Exactly one protocol bump before v1, **1.1 → 1.2 in M1**, freezing the v1
  wire contract. Later milestones add capability contracts carried by that wire, not new messages.
- **The 1.2 wire contract (complete list):**
  - `hello` extensions:
    - `features`;
    - `journal_epoch`;
    - `catalog_hash`;
    - `ceiling_hash` + `ceiling_summary`;
    - `skill_runner_status` (`absent | unsupported{reason} | error{reason} | ok{version}`, RW-39);
    - `helper_status` (`absent | ok{ops} | error{reason}`);
    - `runs_as_root`;
    - `helper_identity{ public_key: base64 of 32 raw bytes }` (the hub computes the fingerprint and
      pins only through owner enrollment, RW-37);
    - `boot_id` (already reported in facts today).
  - `run_capability{execution_id, contract, version, target, args, plan_hash, authorization_kind,
    expires_at}` and its result.
  - `refused{code: Open<RefusalCode>, detail, retry_after?}`.
  - `action_status{execution_ids, expected_epoch}` and its answers.
  - `get_catalog{page, page_size}`.
  - `transfer_begin{transfer_id, kind, size, sha256}`, `transfer_chunk{transfer_id, seq, bytes}`,
    `transfer_end{transfer_id}`, a generic chunked byte stream used by package installation (P1).
  - `agent_status{helper_status, helper_identity, skill_runner_status, runs_as_root, ceiling_hash,
    ceiling_summary}` for runtime changes (agent → hub, sent on change).
  - `attest_safety{ nonce: base64 32 bytes }` (hub → agent).
  - `safety_attestation{ public_key: base64 32 bytes, report: base64 report bytes, signature: base64
    64 bytes }` (agent → hub, all produced by the helper; the report format is RW-37).
  - `run_capability.expected_safety`, required for `Contain`, refused when missing or malformed:
    `verified{ fingerprint, boot_id, generation, config_hash, route_state_hash }` or
    `unverified{ reason }` (human only; RW-37).
  - The `safety_attestation` report is the line format in RW-37, encoded per `04b`.
  - `unsupported{type}` reply.
  - `ping` from the hub.
- **Later milestones use only capabilities over this wire:**
  - M2 uses `get_catalog`.
  - P1 uses `transfer_*` plus capabilities `agent.package.install`, `agent.package.uninstall` and
    `agent.package.list`.
  - M8 and M9 use `run_capability` and `action_status`.
  - M9 containment uses the capabilities `firewall.block_source`, `firewall.confirm_block` and
    `firewall.list_blocks`.
- **F3 makes no wire change.** It is hub and agent logic within 1.1 (exact rule, handshake order,
  durable state, idle state, floors removed).
- **Consequence.**
  - **Upgrade at M1:** every agent must be upgraded together with the hub.
  - **Additional bumps:** if a later milestone discovers a wire change it cannot avoid, that is a
    deliberate 1.3 coordinated upgrade, recorded in the roadmap and release notes before work
    starts. None is planned.
  - **Golden frames:** committed for 1.1 (F3) and 1.2 (M1).
- **Rejected.** A bump per milestone (five coordinated fleet upgrades before v1, each with its own
  compatibility risk).
