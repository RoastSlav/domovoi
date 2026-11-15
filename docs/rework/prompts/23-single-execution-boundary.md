# 23 - Single execution boundary

| | |
|---|---|
| Branch | `single-execution-boundary` |
| Milestone | M1 |
| Scope | One `execute(AuthorizedExecution)` path, one `collect_evidence` read boundary, and the architecture rules that keep both single |
| Depends on (must be merged into `overhaul-investigation` before implementation) | 22 |
| Parallel with | none; 22 runs before 23, and 24 waits for 23 |
| Produces | `AuthorizedExecution`, `execute()` and `ExecuteError` in the engine; `Evidence`, `EvidenceRequest`, `EvidenceScope`, `EvidenceError` and `collect_evidence`; four architecture-rule constants with their scans; the `single_execution_boundary` gate |
| Gate | none beyond 22 merged |
| Suggested model | medium |

## 1. Objective

Make "there is exactly one way to run something and exactly one way to read something from a host" true in
the code and enforceable in CI, without changing what the hub does:

1. **One execution boundary.** `execute(AuthorizedExecution)` in the engine is the only function that
   decides to dispatch and the only caller of `AgentLink::run_skill`. `AuthorizedExecution` exists only as
   that function's precondition and is never persisted (`04 §8.1`).
2. **One read boundary.** `collect_evidence` is the only caller of `AgentLink::run_tool`, for the model,
   profile collection and the Operator alike (`03 RW-08`). It applies the registry checks that exist at
   M1 — enablement, availability, host scope — and is where prompt 25's registry replaces the structural
   ones.
3. **Transports stop at the use case.** A hub transport may call an `ActionHandle`/service method; it may
   not name `AgentLink`, hold one, or call a link method.
4. **The two rules are enforced the way this repository enforces rules:** constants in
   `xtask/src/arch/rules.rs`, line-based scans in `xtask/src/arch/scan.rs`, unit tests beside the existing
   scan tests, a gate test, and the ADR that a `rules.rs` change requires.

This prompt is a **consolidation**, not a feature. It adds no capability, no authority, no column, no
message and no endpoint. Its only new behaviour is that a bypass which exists today becomes a failing
check.

## 2. Scope and non-goals

This prompt owns:

- `domovoi-engine`: the `execute` module (`AuthorizedExecution`, `execute()`, `ExecuteError`), the
  `evidence` module (`Evidence`, `EvidenceRequest`, `EvidenceScope`, `EvidenceError`, `collect_evidence`),
  the call-site changes in `actions/executor.rs`, `actions/recheck.rs`, `triage/tools.rs`,
  `triage/context.rs`, `triage/mod.rs` and `lib.rs`.
- `domovoi-hub`: `profile/{mod,collect}.rs` read through `Evidence`; `agent_server/mod.rs` narrows
  `link()`; `api/mod.rs` reads host state through `AgentServer`; `runtime/mod.rs` builds the two boundaries
  once.
- `xtask`: four rule constants and their scans, with unit tests.
- `ARCHITECTURE.md`: the sentence that states both boundaries, in the section that already describes the
  action flow.
- One ADR in `docs/adr/`: required because `xtask/src/arch/rules.rs` changes (`xtask/src/arch/git.rs:82-95`).

This prompt does **not** own, and must not implement:

| Owner | Excluded work |
|---|---|
| 22 | `ExecutionId`, `Authorization`, `PolicyPermit`, `PlanContract`, `PlanTarget`, `ImplHash`, plan hash v1 and v2, the approval binding columns, the authorization constructors and their scan rules. All of them are **consumed** here, not restated |
| 24 | capability contract types, the contract index, built-ins, the skill adapter. `collect_evidence` gains no contract type |
| 25 | the capability registry: availability states, the catalogue, target identity resolution, replacing `PolicyLink`, `AgentLink::run_capability` |
| 26 | `operations`/`steps`/`executions`, the operation writer, `decide`, the effect runner, `CollectEvidence` as a real effect |
| 27, 28, 29 | Investigate-operation parity, SSE, the hub use-case layer and its module-edge rule |
| 31 | meta-tools, the plan validator, `ContextSource` |
| 45 | `PermitIssuer`, permit issuance, the autonomy ceiling |

Consequences:

- **No new authority, no new constructor.** `execute` reads the authorization the action already carries
  and refuses anything else. Do not add a `HumanApproval`/`PolicyPermit`/`Authorization` constructor, an
  approve endpoint, a permit, or a second way to reach `Approved`.
- **No second execution path.** Do not add a service method, a hub command, a scheduled task or an admin
  endpoint that runs a skill. The rules in section 7 exist to make that structural; the implementation
  must not need an exception to them.
- **No database migration.** There is nothing to store: `AuthorizedExecution` is never persisted, and the
  evidence boundary stores nothing. The index row for this prompt has no migration, and this brief keeps
  it that way deliberately, not by omission.
- **No protocol change.** `PROTOCOL_VERSION` stays where prompt 20 leaves it (1.2), and no message, field
  or golden frame changes. `run_skill`, `run_tool` and `run_check` already exist on the wire.
- **No behaviour change beyond the checks listed in section 5.4.** Every user-visible string, order of
  audit entries, and error text stays what it is today; the suites in section 10 prove it.

## 3. Drafting, launch and dependency

**Drafting (this document).** Written from `03 RW-08`, `RW-09`, `RW-36`, `04 §1`, `§2`, `§3.2`–`§3.4`,
`§8.1`, `§15.1`, and the merged implementation in this checkout, plus the **document** for prompt 22. The
two documents were drafted in the same batch, so this one cites 22's names as written, not as merged.

**Launching (later).** Implementation starts only when prompt 22 is merged into
`overhaul-investigation`, and the model reads the merged code — not the names in this brief. The launch
order is 20, then 21, then 22, then 23, one at a time (`07 §3`, `02 §13.1` EF-3). 22 and 23 touch
overlapping areas (`crates/domovoi-engine/src/actions/*` and `crates/domovoi-core/src/action/*`); 22 goes
first and this brief is written to land on top of it.

**What exists, what this prompt adds, what a later prompt owns.**

| Layer | Now (verified in this checkout) | This prompt | Later |
|---|---|---|---|
| Dispatch decision | `actions/executor.rs:248-352` decides, `writer.rs:333-356` applies; `run_skill` at `executor.rs:334` | the decision body moves behind `execute(AuthorizedExecution)`; `execute` is the only `run_skill` caller | 26 moves both into the operation writer and the effect runner |
| Authorization at dispatch | the row carries the approver, restored by the store (`repos/actions.rs:59`, `request.rs:276`) | `AuthorizedExecution` is built from that row and is the precondition of `execute` | 26 sets `execution_id = step_id`; 45 adds permits |
| Reads | `triage/tools.rs:205` and `triage/context.rs:157` call `run_tool`; `profile/collect.rs:51` calls it from the hub | one `collect_evidence`, one caller of `run_tool` | 25 adds the registry checks inside it; 31 adds `ContextSource` beside it |
| Transport reach | `AgentServer::link()` is public (`agent_server/mod.rs:137`); `api/mod.rs:104/116/125` use it | `link()` is crate-private; `AgentAdmin` reads through `AgentServer` | 29 formalises the use-case layer and its module edges |
| Enforcement | `scan.rs` covers process spawning, shells, pure-crate I/O, file size, crate docs and approval constructors | four constants and their scans: `run_tool`, `run_skill`/`run_capability`, `run_check`, and naming `AgentLink` in a transport | 29 adds the hub module-edge rule |

**Dependency on 22's document.** This brief needs four things from it and takes them as given:
`ExecutionId` (`= ActionId` at M1, `ExecutionId::from_action`/`as_action`), `Authorization` with its two
arms, `ActionRequest::plan_hash_for_approval()` as the single family selector, and `ApprovalError`
carrying `PlanChanged` and `NotYetSupported`. If 22's merged code settles a different name or a different
selector, follow the merged code and report the difference; do not add a compatibility path.

## 4. Verified current state (re-check before editing)

Read these before changing anything. If one now differs materially, record the difference before editing
and stop if it changes the design.

| Area | File | State when this was written |
|---|---|---|
| link port | `crates/domovoi-core/src/ports/agent_link.rs` | `trait AgentLink` `:138` with `run_tool` `:140`, `run_skill` `:148`, `run_check` `:156`, `presence` `:165`, `availability` `:170`, `skills` `:181`; `HostAvailability { Known, Unknown, Absent, Incompatible }` `:92-110`. **No `run_capability`** |
| engine deps | `crates/domovoi-engine/src/actions/mod.rs` | `ActionDeps` `:67` holds `link: Arc<dyn AgentLink>` `:77`; `Shared::commit_action` `:155` is the one action write; `ActionService::spawn` `:253` starts the writer and executor joined |
| executor | `crates/domovoi-engine/src/actions/executor.rs` | `serve` `:51`; catch-up `:112`; `claim` `:147`; `run` `:248` does freshness `:265`, host wait `:268`, pool `:287`, re-freshness `:297`, revalidate `:300`, loop guard `:310`, hash `:314`, `ask_dispatch` `:315`, then `run_skill` `:334`, complete `:343`, verify `:345` |
| writer | `crates/domovoi-engine/src/actions/writer.rs` | 489 lines. `WriteCommand` `:38` (`Approve` `:39-45`, `Dispatch` `:63-67`); `run` `:108`; `process` `:149`; `approve` `:270`; `dispatch` `:333-356`; `transition` `:456` |
| pre-dispatch checks | `crates/domovoi-engine/src/actions/freshness.rs`, `host_wait.rs`, `recheck.rs` | `freshness::check` `:42-57` reads the incident; `wait_for_host` `:31-52` polls `deps.link.availability`; `recheck::verify` `:56-100` calls `run_check` `:73-81` |
| model read path | `crates/domovoi-engine/src/triage/tools.rs` | `call` `:62`, `run` `:77`, `bounded` `:104`, `execute` `:117`, `agent` `:191` — deadline `:201`, `deps.link.run_tool` `:205`, truncation text `:209-213`; `RunReadSkill` risk check `:176-186`; `prefetch` `:218` |
| other read paths | `crates/domovoi-engine/src/triage/context.rs`, `triage/chat.rs`, `triage/mod.rs` | `context.rs:148-157` checks `link.presence` then `run_tool` for docker events, on failure only `tracing::debug`; `chat.rs:11` imports `tools::{Scope, cut, run}`, so the Operator path is the same `run`; `TriageDeps` `:131` holds `link` `:137` |
| hub profile | `crates/domovoi-hub/src/profile/mod.rs`, `profile/collect.rs` | `ProfileDeps.link: Arc<dyn AgentLink>` `mod.rs:141`; `ask` `collect.rs:46-53` is the only `run_tool` call in the hub, redacting at `:54` and appending `AGENT_CUT` `:30` |
| hub transport leak | `crates/domovoi-hub/src/api/mod.rs` | `impl AgentAdmin for AgentServer` uses `self.link()` at `:104` (presence), `:116` (skills), `:125` (logs, a `WsAgentLink` inherent method, not on the trait); `ApiDeps.actions_service: ActionHandle` `:156` is the intended seam |
| hub approve/deny | `crates/domovoi-hub/src/api/actions.rs`, `auth/interactions/buttons.rs` | web `:132-133` approve, `:165-166` deny; chat `:170-171`, `:196-197`; both go through `ActionHandle` |
| the wrapper | `crates/domovoi-hub/src/agent_server/mod.rs`, `skill_policy.rs` | `pub fn link(&self) -> Arc<WsAgentLink>` `mod.rs:137`; `PolicyLink` `skill_policy.rs:144`, `wrap` `:154`, forwarding impl `:160-195` |
| composition root | `crates/domovoi-hub/src/runtime/mod.rs` | one `policy_link` built at `:223`, then handed to actions `:230`, triage `:242` and profile `:278`; `TriageBuild.link: Arc<dyn AgentLink>` `:413` |
| evidence today | `crates/domovoi-core/src/ports/uow/effects.rs` | `EffectKind::CollectEvidence` `:51` (wire name `collect_evidence`), an effect **kind** only. There is **no** engine read-boundary function anywhere: `grep collect_evidence` finds nothing outside that enum |
| gate tests | `crates/domovoi-engine/tests/gates.rs`, `crates/domovoi-core/tests/gates.rs`, `crates/domovoi-hub/tests/gates.rs` | engine: `action_writes_go_through_the_unit_of_work` `:19` (a source scan over `src/`), `model_sees_only_read_tools` `:74`; core: `no_approval_without_human` `:67`; hub: `agent_link_requires_completed_handshake` `:33` |
| arch mechanism | `xtask/src/arch/{mod,rules,scan,git,manifest,graph}.rs`, `xtask/src/main.rs` | `arch::run` `mod.rs:30-42` = graph + manifest + `scan::check_tree` (+ `git::check` with `--base`); `scan::check_file` `:61-157` is a **line-based text scan** over `crates/**/*.rs`; test paths exempt `:51-53`; approval scan `:105-118`; attribute rule `:91-97`; size `:132-139`; crate docs `:141-155`; `rules.rs` constants `:166-204`; `git.rs:82-95` requires a new numbered ADR when `rules.rs` changes; wired at `main.rs:68` |
| migrations | `crates/domovoi-store/migrations/` | `0001` … `0010_llm_admission.sql`. **This prompt adds none**; read the directory anyway, because 22 adds one before this branch is cut |

**Facts to carry, not "fix":**

1. `single_execution_boundary` and `no_permit_without_policy_allow` (`04 §8.1` "Added gates") do **not**
   exist in this tree. This prompt adds `single_execution_boundary`; it deliberately does **not** add
   `no_permit_without_policy_allow`, because no permit exists at M1 and a gate over a type nobody can
   build would assert nothing. 45 adds it with `PermitIssuer`. Record that in the PR.
2. `03 RW-08` says the rule is "no other module calls `AgentLink::run_skill` / `run_capability`". There is
   no `run_capability` on the trait in this tree; the scan covers the literal so it starts enforcing when
   24/25 add the method.
3. `03 RW-08` says "the raw link stops being publicly reachable from transports". It is reachable today
   through `AgentServer::link()` (`agent_server/mod.rs:137`) and, in practice, only `api/mod.rs` and
   `runtime/mod.rs` use it. Narrowing it is a visibility change, not a behaviour change.
4. `triage/tools.rs::agent` uses `bounded()` (`:104`) for the deadline and its own truncation text;
   `profile/collect.rs::ask` applies its own redaction and `AGENT_CUT`. The boundary must not take either
   over, or the rendered text changes (section 5.4).

## 5. Requirements and behavioural invariants

### 5.1 The one execution path

`execute(AuthorizedExecution)` lives in a new `crates/domovoi-engine/src/actions/execute.rs` — the engine's
action-writer module, next to `writer.rs` (`03 RW-08`, `04 §8.1`). It is a separate file because
`writer.rs` is already 489 lines and the repository limit is 500 (`scan.rs:132-139`); do not grow
`writer.rs` to hold it.

```rust
// actions/execute.rs
pub struct AuthorizedExecution { /* action: ActionRequest, execution_id: ExecutionId */ }

impl AuthorizedExecution {
    /// The only constructor. Fails when the action has no stored human approval.
    pub(crate) fn of(action: ActionRequest) -> Result<Self, ExecuteError>;
    /// The action as it will be dispatched.
    #[must_use] pub fn action(&self) -> &ActionRequest;
    /// The idempotency key; equals the action id at M1.
    #[must_use] pub fn execution_id(&self) -> ExecutionId;
}

pub(crate) async fn execute(
    shared: &Shared,
    authorized: AuthorizedExecution,
    skill: SkillSummary,
    args: ValidatedArgs,
) -> Result<(), ExecuteError>;

pub enum ExecuteError { /* NotAuthorized, NotYetSupported, Expired, PlanChanged, NotDispatched, Refused(..) */ }
```

Rules:

1. `AuthorizedExecution::of` is `pub(crate)` and is called in exactly one place: `actions/executor.rs`,
   immediately before `execute`, and nowhere else. `execute` takes the value by reference and never builds
   one. No field is public, and the type is never serialised or persisted.
2. `execute` is called from exactly one place: the per-action run task in `actions/executor.rs`
   (`run`, `:248`). It is not a port method, not a trait method and not reachable from a transport.
3. `execute` is the only caller of `AgentLink::run_skill`. It queues the one dispatch transition
   (`WriteCommand::Dispatch { id, plan }`, kept as prompt 22 leaves it), and only when the writer answers
   `Dispatched` does it call `run_skill`, exactly as `executor.rs:315-334` does today.
4. `execute` takes over the tail the executor owns today — `executor.rs:300-352`: the revalidation, the loop
   guard, the hash recomputation for the action's own family (prompt 22's `plan_hash_for_approval()` /
   `compute_v2`), the `Dispatch` commit, `run_skill`, the completion and the verification step — in that
   order, with no step added, removed or reordered. The executor keeps everything before it
   (`executor.rs:248-300`: freshness, the host wait, the pool, the second freshness check, the revalidate
   call), so the incident is still read once, the hash is still recomputed once and the commit is still one.
   `execute` additionally guards the precondition `03 RW-08` names and nothing else checks: the stored
   authorization exists and has not expired. That is a check, not a new phase.
5. There is exactly one dispatch transition. `WriteCommand::Dispatch {` is constructed only in
   `execute.rs`; the writer defines it and matches it (`writer.rs:63-67`, `:200-206`). A second
   construction is a failing gate test, not a code review note.

### 5.2 The one read boundary

`collect_evidence` lives in a new `crates/domovoi-engine/src/evidence/mod.rs` and is the only caller of
`AgentLink::run_tool`.

```rust
// evidence/mod.rs
/// Which host a caller may read. Constructed from the thing the caller works on.
pub enum EvidenceScope { Incident(HostId), Host(HostId) }

pub struct EvidenceRequest { /* scope, call: ReadToolCall, deadline: Timestamp */ }

impl EvidenceRequest {
    pub fn for_incident(incident: &Incident, call: ReadToolCall, deadline: Timestamp) -> Self;
    pub fn for_host(host: HostId, call: ReadToolCall, deadline: Timestamp) -> Self;
    #[must_use] pub fn host(&self) -> HostId;
}

/// Holds the only read-capable handle to agents.
pub struct Evidence { /* link: Arc<dyn AgentLink> */ }

impl Evidence {
    pub fn new(link: Arc<dyn AgentLink>) -> Self;
    /// Enablement, availability and host scope, then one `run_tool`.
    pub async fn collect_evidence(&self, request: EvidenceRequest) -> Result<ToolOutput, EvidenceError>;
}

pub enum EvidenceError { /* NotConnected, Incompatible, Timeout, Transport(..), NotEnabled(..) */ }
```

Rules:

1. **Host scope is a property of the request, not an argument.** The host that reaches `run_tool` is
   `request.host()`, derived from the incident or the host the caller was given; a caller cannot pass a
   different host alongside an incident.
2. **Enablement at M1 is structural.** `ReadToolCall` is a closed enum (`core/src/triage`), so the tools
   that exist are the tools that may run; the one extra rule that exists today — `RunReadSkill` only for a
   skill whose `Risk::Read` and whose args validate (`triage/tools.rs:176-186`) — moves into
   `collect_evidence`. Prompt 25 replaces the structural check with the registry's enablement check
   *inside this function*, never beside it.
3. **Availability is checked here.** `Unknown`, `Absent` and `Incompatible` return before any link call, as
   `EvidenceError::NotConnected` / `Incompatible` — the same `LinkError` variants `run_tool` returns today,
   so no rendered error text changes.
4. **The boundary reads and only reads.** It returns the agent's `ToolOutput` unchanged: no redaction, no
   truncation, no formatting. Redaction and the "[the agent cut this output]" texts stay with the callers
   that own them (`triage/tools.rs:209-213`, `profile/collect.rs:54-58`), because moving them would change
   the text the model and the profile show.
5. **Every read goes through it**: the model and the Operator (`triage/tools.rs`), the triage context
   (`triage/context.rs:157`) and the hub's profile collection (`profile/collect.rs:51`).
   `triage/context.rs:148-151`'s `presence` check may stay (it decides whether to ask at all) but the call
   that follows is `collect_evidence`.
6. **`run_check` is not part of this boundary in this prompt.** Verification re-runs a check after a
   successful run (`recheck.rs:73`), it is not evidence and it is not read-only. It keeps its one call
   site, and section 7's rule names it so a second site is caught. 26/27 may move it with the verification
   effect; that is recorded as deferred, not done here.

### 5.3 Transport → use case → engine

1. A hub transport module (`api/`, `auth/`, `chat/` gateways) may call: an `ActionHandle` method, a
   manager or read repository, `AgentAdmin`, or a `QueryBoundary`-style read service. It may not name
   `AgentLink`/`WsAgentLink`/`PolicyLink`, hold one, or call `run_tool`/`run_skill`/`run_check`.
2. `AgentServer::link()` becomes `pub(crate)`. `runtime/mod.rs` (the composition root) keeps using it to
   build `PolicyLink` and `Evidence`; nothing else in the hub may.
3. `AgentAdmin for AgentServer` (`api/mod.rs:93-127`) stops reaching the link: `AgentServer` gains inherent
   `host_presence`, `host_skills` and `host_logs` methods that use its own `inner.link`, and the trait impl
   delegates to them. Same values, same errors, same signature of the trait.
4. The engine is reached only through its handles and its two boundaries. `TriageDeps.link` is replaced by
   `TriageDeps.evidence` for reads and a narrow `HostCatalog` accessor for the two facts triage still needs
   (`skills()`, `presence()`); do not keep `Arc<dyn AgentLink>` in `TriageDeps`, because `EXEC-4` forbids the
   type name in `engine/src/triage/`. Whichever accessor shape you choose, `run_tool` must not appear outside
   `evidence/`.
5. `PolicyLink` keeps wrapping `run_skill` and `run_tool` (`skill_policy.rs:160-195`) and is still handed
   to the boundaries in `runtime/mod.rs:223-285`. This prompt does not replace it; 25 does.

### 5.4 Behaviour that must not change

| Behaviour | Why it is at risk | What keeps it |
|---|---|---|
| Tool output text and truncation | moving formatting into the boundary | the boundary returns `ToolOutput` verbatim (5.2 rule 4) |
| Error text a tool call shows | re-mapping link errors | `EvidenceError` reuses the `LinkError` variants and `err.to_string()` (5.2 rule 3) |
| Dispatch order: freshness → host → revalidate → loop guard → hash → commit → send | moving the decision body | the body moves verbatim into `execute`; the executor keeps its phases |
| One audit entry per transition | a second commit | `commit_action` (`mod.rs:155`) stays the only write; `execute` writes nothing itself |
| Chat approve prefix binding | touching the approve path | not touched; 22 keeps `PlanBinding` and the two transports |
| Profile sections and their failures | replacing `ProfileDeps.link` | `profile/*` keeps its own redaction and `ok: false` section text (`collect.rs:62-75`) |

### 5.5 The `no second execution path` invariant

- **Invariant.** After this change there is exactly one code path from a human decision to a running skill:
  approve (writer) → `Approved` → per-action run task → `execute` → `Dispatch` commit → `run_skill`. There
  is no other constructor of `AuthorizedExecution`, no other sender of `WriteCommand::Dispatch`, and no
  other caller of `run_skill`.
- **How the rule keeps it true.** `rules.rs` names the one file allowed to contain `run_skill(`, `execute.rs`
  names the one file allowed to construct `WriteCommand::Dispatch {` (a gate test, since the definition and
  the `match` arm live in `writer.rs`), and `scan.rs` fails `arch-check` for any other occurrence in
  `domovoi-core`, `domovoi-engine`, `domovoi-hub` or `domovoi-store` source.
- **Honesty.** This is a line-based text scan, exactly like the approval rule (`scan.rs:105-118`). It does
  not stop a renamed re-export, a macro, or a fully qualified call split across lines, and the compiler
  enforces nothing here. Do not describe it as a compile-time barrier in any comment, doc or PR sentence.
  What it does do is make the ordinary way of adding a second path fail CI, which is the property the
  milestone needs.

## 6. Data model, interfaces, ownership and call-flow rules

### 6.1 Types, and what owns them

| Type | Crate / module | Owns | Never does |
|---|---|---|---|
| `AuthorizedExecution`, `ExecuteError`, `execute()` | engine `actions/execute.rs` | being the single dispatch path | persist itself, hold a link, or be public outside the crate |
| `Evidence`, `EvidenceRequest`, `EvidenceScope`, `EvidenceError`, `collect_evidence` | engine `evidence/` | being the single read path and the host-scope check | store, redact, format, or expose the link |
| `Authorization`, `PolicyPermit`, `ExecutionId`, plan hash v1/v2 | 22, consumed unchanged | authorization and plan identity | — |
| `AgentLink` | core `ports/agent_link.rs`, unchanged | the transport port | gain a method in this prompt |

No table, no column, no DTO, no endpoint and no message is added or changed.

### 6.2 Call-flow rules

```text
web/API transport ── ActionHandle::approve(Authorization::Human(..), binding, provenance) ──▶ writer (22)
web/API/chat transport ── ActionHandle::deny/expire/… ──▶ writer
approved action ── executor::run ──▶ execute(AuthorizedExecution) ──▶ WriteCommand::Dispatch ──▶ writer
                                                        └── on Dispatched ── AgentLink::run_skill
                        └── verify ──▶ recheck::verify ──▶ AgentLink::run_check
model / Operator / triage context ──▶ collect_evidence(EvidenceRequest) ──▶ AgentLink::run_tool
hub profile service ──────────────▶ collect_evidence(EvidenceRequest) ──▶ AgentLink::run_tool
api host pages ──▶ AgentAdmin ──▶ AgentServer inherent methods ──▶ (inside agent_server) link
```

1. No arrow may be added that skips a box. In particular a transport may not carry a link, and a read may
   not reach `run_tool` except through `collect_evidence`.
2. `execute` is `pub(crate)`; `AuthorizedExecution` has no public field and no public constructor.
3. `Evidence` is `Clone` and cheap: `runtime/mod.rs` builds it once and hands a clone to triage and to
   profile.

### 6.3 Failure and refusal rules

| Situation | Behaviour |
|---|---|
| action has no stored approval at dispatch (`approved_by` is `None`) | `ExecuteError::NotAuthorized`; the action stays as it is and the run stops. Unreachable for an `Approved` row; it is the precondition's own guard |
| authorization is `Policy` | `ExecuteError::NotYetSupported`, nothing written — the same answer prompt 22's writer gives |
| approval expired | the existing `Stale::Expired` rejection path, unchanged; `execute` returns `Expired` |
| plan hash differs at the instant of dispatch | `WriteCommand::Dispatch` answers `Rejected`; `execute` returns `PlanChanged` and does not call `run_skill` |
| writer closing during dispatch | `ExecuteError::NotDispatched`; nothing was sent |
| host `Unknown`/`Absent` at a read | `EvidenceError::NotConnected`, no link call |
| host `Incompatible` at a read | `EvidenceError::Incompatible` |
| `RunReadSkill` for a write skill or invalid args | `EvidenceError::NotEnabled`, the same message the tool returns today |
| store failure inside a boundary | unchanged: the read returns the link/repository error, the dispatch leaves the action approved |

## 7. Architecture rules

These are the rules that must exist after this prompt. All four follow the mechanism that already exists;
none of them is compiler-enforced.

### 7.1 The mechanism, as it really is

`cargo xtask arch-check` runs four checks (`xtask/src/arch/mod.rs:30-42`): a cargo-metadata dependency
graph (`graph.rs`), manifest rules (`manifest.rs`), a **line-based text scan of every `.rs` file under
`crates/`** (`scan.rs::check_tree`, `check_file` at `:61`), and, with `--base`, change-based rules
(`git.rs`: append-only migrations, and the ADR requirement for `rules.rs`). The command is wired at
`xtask/src/main.rs:68`.

A new rule is therefore:

1. one or more `pub const` path allowlists in `xtask/src/arch/rules.rs`, with a doc comment naming the
   invariant and the ADR, in the style of `HUMAN_APPROVAL_ALLOWED_PREFIXES` (`:169`);
2. a block in `scan::check_file` that reports a `Violation` for a non-comment line outside those prefixes,
   with a message that says what to do instead;
3. unit tests in `scan.rs` next to `human_approval_outside_auth_is_rejected`;
4. a numbered ADR in `docs/adr/`, because `rules.rs` changed (`git.rs:82-95`).

### 7.2 The four rules

A rule on a method name also has to allow the files that **declare and implement** that method, or it fails
on the trait itself. One constant holds them; all three method rules use it.

| Constant | Value | Why |
|---|---|---|
| `LINK_DEFINITION_PREFIXES` | `crates/domovoi-core/src/ports/agent_link.rs`, `crates/domovoi-hub/src/agent_server/link.rs`, `crates/domovoi-hub/src/agent_server/skill_policy.rs`, `crates/domovoi-testkit/` | the trait's declarations, the `WsAgentLink` impls, the `PolicyLink` forwarding impls and the fakes. Adding a new implementation is then a deliberate, reviewed edit to this list, which is the review you want |

| Constant (in `rules.rs`) | Allowed prefixes | Scanned literal | Rule id |
|---|---|---|---|
| `RUN_TOOL_ALLOWED_PREFIXES` | `crates/domovoi-engine/src/evidence/` + `LINK_DEFINITION_PREFIXES` | `run_tool(` | `EXEC-1` |
| `RUN_SKILL_ALLOWED_PREFIXES` | `crates/domovoi-engine/src/actions/execute.rs` + `LINK_DEFINITION_PREFIXES` | `run_skill(`, `run_capability(` | `EXEC-2` |
| `RUN_CHECK_ALLOWED_PREFIXES` | `crates/domovoi-engine/src/actions/recheck.rs` + `LINK_DEFINITION_PREFIXES` | `run_check(` | `EXEC-3` |
| `AGENT_LINK_ALLOWED_PREFIXES` | `crates/domovoi-core/src/ports/`, `crates/domovoi-hub/src/agent_server/`, `crates/domovoi-hub/src/runtime/`, `crates/domovoi-engine/src/{evidence,actions}/`, `crates/domovoi-testkit/` | the type name `AgentLink`, the tokens `WsAgentLink` and `PolicyLink` | `EXEC-4` |

Rules that make them usable rather than noisy:

- The scan for `run_tool(`/`run_skill(`/`run_check(` applies only to files under
  `crates/domovoi-core/src/`, `crates/domovoi-engine/src/`, `crates/domovoi-hub/src/` and
  `crates/domovoi-store/src/`. The agent, the runner, the protocol crate and the binary have their own
  `run_skill`/`run_tool` handler names and are out of scope; a rule that also flagged
  `domovoi-runner/src/catalog/mod.rs:118` would be turned off within a week.
- Test paths keep the existing exemption (`scan.rs:51-53`), so tests may build requests and call the
  boundaries directly.
- `AgentLink` is allowed where the trait is *declared, implemented or composed*: `domovoi-core`'s ports
  module (the declaration), `agent_server/` (the `WsAgentLink` and `PolicyLink` impls), `runtime/` (the
  composition root) and the engine's two boundary modules — and nowhere else in the hub. That is the rule
  that stops a transport reaching the link, and it is stronger than a rule on method names because it also
  catches a stored `Arc<dyn AgentLink>` field.

### 7.3 What the implementer must run

```text
cargo xtask arch-check
cargo xtask arch-check --base origin/overhaul-investigation
```

Both must print `arch-check: all rules hold`. The `--base` form is required because this branch changes
`rules.rs` and must therefore add the ADR. Do not weaken, rename or skip an existing rule to make room.

## 8. Files and components expected to change

| Path | Change |
|---|---|
| `crates/domovoi-engine/src/actions/execute.rs` (new) | `AuthorizedExecution`, `ExecuteError`, `execute()`; the dispatch body moved from `executor.rs:300-352` |
| `crates/domovoi-engine/src/actions/mod.rs` | `mod execute;`, crate-private re-export; `ActionDeps` unchanged unless a field is genuinely unused |
| `crates/domovoi-engine/src/actions/executor.rs` | `run` ends by building the action's skill/args and calling `execute`; `run_skill` and the `Ask complete`/`verify` tail move into `execute` |
| `crates/domovoi-engine/src/actions/writer.rs` | no behaviour change; the file must not grow past the 500-line limit |
| `crates/domovoi-engine/src/evidence/mod.rs` (new) | `Evidence`, `EvidenceRequest`, `EvidenceScope`, `EvidenceError`, `collect_evidence`, unit tests |
| `crates/domovoi-engine/src/lib.rs` | `pub mod evidence;` and its re-exports |
| `crates/domovoi-engine/src/triage/tools.rs` | `agent()` delegates to `collect_evidence`; the `RunReadSkill` risk/args check moves into the boundary |
| `crates/domovoi-engine/src/triage/context.rs` | the docker-events read goes through `collect_evidence` |
| `crates/domovoi-engine/src/triage/mod.rs` | `TriageDeps` carries `Evidence` and a narrow `HostCatalog` accessor instead of `link` |
| `crates/domovoi-hub/src/profile/mod.rs`, `profile/collect.rs` | `ProfileDeps.evidence: Evidence` replaces `link`; `ask` calls `collect_evidence` and keeps its redaction and `AGENT_CUT` |
| `crates/domovoi-hub/src/agent_server/mod.rs` | `link()` becomes `pub(crate)`; inherent `host_presence`, `host_skills`, `host_logs` |
| `crates/domovoi-hub/src/api/mod.rs` | `AgentAdmin for AgentServer` delegates to those inherent methods |
| `crates/domovoi-hub/src/runtime/mod.rs` | build one `Evidence` from `policy_link`; pass it to triage and profile; `PolicyLink` still goes to the action service |
| `xtask/src/arch/rules.rs` | the four constants and their doc comments |
| `xtask/src/arch/scan.rs` | the scans, their `Violation`s and their unit tests |
| `docs/adr/00NN-single-execution-boundary.md` (new) | required by `git.rs:82-95`; NN read from `docs/adr/` at branch cut |
| `ARCHITECTURE.md` | one statement of both boundaries where the action flow is described, and the rule ids beside it |
| `crates/domovoi-engine/tests/gates.rs` | `single_execution_boundary` and its scan helper |
| `crates/domovoi-hub/tests/gates.rs` | the transport rule, asserted over `crates/domovoi-hub/src` |

Do not touch: `crates/domovoi-proto/**`, `crates/domovoi-agent/**`, `crates/domovoi-runner/**`,
`crates/domovoi-store/**`, `web/**`, `crates/domovoi-core/src/action/**` (22's area), and any migration.

## 9. Migration (none) and protocol treatment

- **No database migration.** This change stores nothing new: `AuthorizedExecution` never leaves memory,
  the evidence boundary returns a value, and the authorization it consumes is the approval row prompt 22
  already reads. The index row for this prompt has no migration; if a reviewer asks for one, the answer is
  no, and the reason is in this section.
- **No protocol change.** `PROTOCOL_VERSION` is untouched. `run_skill` and `run_tool` already exist on the
  wire; this prompt changes which Rust module calls them, not what is sent. No golden frame, refusal code
  or tolerance rule changes (`03 RW-42`).
- **Generated files.** None. No OpenAPI field changes, so `cargo xtask openapi` and `npm run gen:api` are
  not needed; if a DTO did change, 22 owns that regeneration, not this.

## 10. Security and failure handling

### 10.1 What this change proves, and what it does not

- **It proves**: there is one module that can run a skill and one that can read from a host, that CI fails
  when a second one appears, and that a transport cannot obtain the link that would let it do either.
- **It does not prove**: that the process is uncompromised. A hub that can edit its own source can remove
  the rule; the runtime limits remain dispatch recomputation, the agent's local checks, the ceiling, the
  helper allowlist and the audit chain (`03 RW-11`, `04 §8.4`).
- **It does not add authority.** No approval, permit or authorization value is created, widened or
  restored here. `Authorization::Policy` stays refused (`NotYetSupported`), and `HumanApproval` stays
  hub-auth-only and store-restored (`03 RW-11`).
- **It does not make a read safe by itself.** `collect_evidence` serves the same closed read-tool set the
  model already had, and the same redacted agent output. Its value is that enablement, availability and
  host scope have one owner, which 25 can extend instead of duplicating.

### 10.2 Bypass analysis

| Route | Why it does not bypass the boundary |
|---|---|
| a second caller of `run_skill` | `EXEC-2` fails `arch-check`; the `single_execution_boundary` gate fails too |
| a second `WriteCommand::Dispatch` sender | gate test over `crates/domovoi-engine/src`; only `execute.rs` may construct it outside `writer.rs` |
| a transport holding `Arc<dyn AgentLink>` | `EXEC-4` fails on the type name outside `agent_server/`, `runtime/` and the two engine modules |
| `AgentServer::link()` from an endpoint | `pub(crate)` plus `EXEC-4`; the endpoint uses `AgentAdmin` |
| a read through a repository instead of the agent | not a bypass of this boundary: repositories are not host reads, and the model's catalogue cannot express them |
| a renamed re-export or macro | not caught, and stated as such: this is a text scan (5.5, 7.1) |
| `run_check` used as a read | it is a check re-run with one call site (`EXEC-3`); it returns a status for a declared check, not arbitrary output |

### 10.3 Failure handling

Every row of 6.3 is a refusal that writes nothing. The boundary functions add no retry, no timeout of
their own (callers keep their deadlines), no fallback host and no "best effort" mode. A read that cannot
be performed is an error the caller already displays; a dispatch that cannot be performed is a rejection
with the existing reason text.

## 11. Tests and acceptance criteria

Name tests after the behaviour. Keep the exact names marked **(required)**; `04 §8.1` and later prompts
cite them.

### 11.1 Engine (`crates/domovoi-engine`)

- **`single_execution_boundary`** **(required, gate)**: a source scan over `src/` asserts that `run_skill(`
  appears only in `actions/execute.rs`, `run_tool(` only in `evidence/`, `run_check(` only in
  `actions/recheck.rs`, that `WriteCommand::Dispatch {` is constructed only in `actions/execute.rs`, and
  that `AuthorizedExecution::of(` and `execute(` are called only in `actions/executor.rs`. Model it on
  `action_writes_go_through_the_unit_of_work` (`tests/gates.rs:19-56`), including its comment-line skip.
- `execute_refuses_an_action_without_authorization`: a row with no stored approval returns
  `ExecuteError::NotAuthorized` and nothing reaches the agent.
- `execute_refuses_a_policy_authorization`: `Authorization::Policy` at M1 returns `NotYetSupported` and
  writes nothing (the engine half of 22's rule).
- `execute_refuses_an_expired_approval`: the existing `Stale::Expired` rejection still fires and no
  `run_skill` is attempted.
- `execute_does_not_run_a_skill_the_writer_did_not_dispatch`: a plan that changed between approval and
  dispatch leaves the action `Rejected` and the recording link sees no call.
- `evidence_collect_uses_the_scope_host`: a request built from an incident reads that incident's host, and
  no other host is passed to the link.
- `evidence_collect_returns_not_connected_for_an_absent_host`: `Unknown` and `Absent` return
  `EvidenceError::NotConnected` **without** a link call.
- `evidence_collect_refuses_a_write_skill_as_a_read`: `RunReadSkill` for a `Risk::Write` skill returns
  `NotEnabled` with today's message.
- `the_triage_tool_path_reads_through_the_boundary`: `triage::tools::run` reaches the agent exactly once
  through `Evidence` (a counting link), with the same result text as before.

### 11.2 Hub (`crates/domovoi-hub`)

- **`transports_do_not_name_the_agent_link`** **(required)**: a source scan over `src/api/`, `src/auth/`
  and the chat gateway asserts no `AgentLink`, `WsAgentLink` or `PolicyLink` appears there.
- `api_host_state_is_read_without_the_link`: presence, skills and logs still answer from `AgentAdmin`, and
  the values match what the agent reported (a fake agent, one call each).
- `profile_collection_reads_through_the_evidence_boundary`: the profile page still renders the four fixed
  sections and each failed section still reads `ok: false` with the agent's error text.

### 11.3 Existing suites that must pass unchanged

These are the proof that the consolidation did not change behaviour; run them at each step of section 12.

| Suite | What it protects |
|---|---|
| `crates/domovoi-engine/tests/services/execute.rs` (`:38`, `:115`, `:161`, `:185`, `:249`, `:299`, `:385`) | dispatch, rejection on a manifest change or invalid args, the loop guard, the offline-host wait, the ordered audit trail, redacted and capped output |
| `crates/domovoi-engine/tests/services/freshness.rs` | expiry, incident gone/resolved, and a recording link that holds a run (`:400-438`) |
| `crates/domovoi-engine/tests/services/actions.rs` | the action service's approve/deny/propose/expire flow and its event publishing |
| `crates/domovoi-engine/tests/services/profile.rs` (`:16`, `:96`, `:152`) | what triage sees of the profile note, including the cut tool output |
| `crates/domovoi-engine/tests/services/chat.rs` (`:33`, `:77`) | the Operator path reads the host it chose |
| `crates/domovoi-engine/tests/gates.rs` (`:19`, `:74`) | the writer-only rule and the closed read-tool catalogue |
| `crates/domovoi-hub/tests/hub/profile.rs` | the profile endpoint and its sections |
| `crates/domovoi-hub/tests/hub/interactions.rs` | chat approve/deny through the buttons |
| `crates/domovoi-hub/tests/hub/sessions.rs`, `crates/domovoi/tests/agent_hub.rs` (`:241`, `:268`) | the link still round-trips `run_tool`/`run_skill` when the tests use it directly |
| `crates/domovoi-core/tests/gates.rs` (`:67`) | `no_approval_without_human` still compiles against the signatures 22 leaves |

### 11.4 Acceptance criteria

1. `execute(AuthorizedExecution)` exists in the engine's action-writer module, is `pub(crate)`, and is the
   only caller of `run_skill`.
2. `AuthorizedExecution` has one crate-private constructor used only by `execute`, is never persisted, and
   carries the action and its `ExecutionId`.
3. `collect_evidence` exists, is the only caller of `run_tool`, and serves the model, the Operator, the
   triage context and profile collection.
4. Enablement (structural at M1), availability and host scope are checked inside `collect_evidence`, and
   its errors keep today's text.
5. No hub transport names or holds `AgentLink`; `AgentServer::link()` is not public.
6. Four rule constants and their scans exist, with unit tests, and `cargo xtask arch-check` and
   `cargo xtask arch-check --base origin/overhaul-investigation` both pass.
7. The gate tests `single_execution_boundary` and `transports_do_not_name_the_agent_link` exist and fail
   when the rule is violated (verify by temporarily adding an offending line and reverting it).
8. Every suite in 11.3 passes with no change to its assertions.
9. The diff contains no migration, no wire change, no protocol bump, no new approval, permit or
   authorization constructor, and no change under `crates/domovoi-core/src/action/`.
10. The ADR from `rules.rs` is in the same PR, and `ARCHITECTURE.md` states both boundaries.

## 12. Documentation and architecture updates

- **`ARCHITECTURE.md`**: add, where the action flow is already described (§5.6) and nowhere else, one
  short paragraph: dispatch happens only through `execute(AuthorizedExecution)`, host reads only through
  `collect_evidence`, and both are enforced by `cargo xtask arch-check` rule ids `EXEC-1`–`EXEC-4`.
  State plainly that the enforcement is a text scan. Do not restate 22's plan-hash section.
- **ADR** `docs/adr/00NN-single-execution-boundary.md`: the one execution boundary, the one read boundary,
  the four rules and why they are scans rather than compiler constructs, what they cannot catch, and why
  `run_check` is kept outside the read boundary for now. Required by `xtask/src/arch/git.rs:82-95`.
- **`docs/book`**: no change. Nothing a reader of the product documentation can observe changes; the
  boundaries are internal structure, and inventing a page for them would document a rule, not a feature.
- **`docs/rework/`**: nothing. Implementation prompts do not edit the planning documents. `03`/`04` already
  state both boundaries (`RW-08`, `§8.1`); this prompt implements them and does not rewrite them.

## 13. Implementation order and completion requirements

1. **Read the merged prerequisites.** Prompt 22's merged `Authorization`, `ExecutionId`,
   `plan_hash_for_approval`, `PlanHash::compute_v2` and the writer's approve/dispatch shape. Confirm the
   four facts in section 4 before editing, and report any new difference first.
2. **Engine, execution boundary:** add `actions/execute.rs` with `AuthorizedExecution` and `execute`, move
   the dispatch body from `executor.rs:300-352` verbatim, and point `executor::run` at it. Run
   `cargo nextest run -p domovoi-engine` — the execute, freshness and actions suites must pass untouched
   before any rule is added.
3. **Engine, read boundary:** add `evidence/`, move `triage/tools.rs::agent` onto it, then
   `triage/context.rs`, then the `TriageDeps` field. Run the engine suite, including `services/profile.rs`
   and `services/chat.rs`.
4. **Hub:** `ProfileDeps.evidence`, then `AgentServer::link()` narrowing and the `AgentAdmin` delegation,
   then `runtime/mod.rs` building `Evidence` once. Run `cargo nextest run -p domovoi-hub` and
   `cargo nextest run -p domovoi`.
5. **Rules and ADR:** the four constants, the scans, the scan unit tests, the ADR. Then
   `cargo xtask arch-check` and `cargo xtask arch-check --base origin/overhaul-investigation`.
6. **Gate tests:** `single_execution_boundary` and `transports_do_not_name_the_agent_link`; prove each one
   fails by adding a violating line locally, then revert it.
7. **Docs:** the `ARCHITECTURE.md` paragraph.
8. **Checks for what changed:** `cargo fmt --all`;
   `cargo clippy -p domovoi-engine -p domovoi-hub -p xtask --all-targets -- -D warnings`;
   `cargo nextest run -p domovoi-engine`, `-p domovoi-hub`, `-p domovoi`, `-p xtask`; and
   `cargo nextest run -E 'rdeps(domovoi-engine)'`, because `Evidence` and `execute` are new public items.
   Do not run the whole workspace suite or `cargo xtask ci`: CI does that on the PR. On Windows the hub and
   agent run only under WSL 2; these tests run natively.

Completion requirements:

- Every named test in section 11 exists, with the required names spelled exactly.
- No `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!` or `dbg!` in library code; no new
  `#[allow(...)]`; every new public item documented; `actions/execute.rs` and `evidence/mod.rs` both stay
  well under 500 lines, and `writer.rs` does not grow past it.
- The diff adds no migration, no wire field, no protocol bump, no endpoint, no DTO field, no approval or
  permit constructor, and nothing under `crates/domovoi-core/src/action/`.
- `cargo xtask arch-check` and its `--base` form both printed `arch-check: all rules hold`, and the PR
  says so with the real output.
- Every suite listed in 11.3 passed without an assertion change; if one had to change, the PR says which
  and why, and the reviewer treats it as a behaviour change.
- The completion report below is filled in with real command output, not claims.

## Execution constraints

You are implementing **one** change in the Domovoi repository (a Rust workspace with an embedded React UI).
This document is your complete brief. The planning documents it cites are background: what you need is
written here.

1. **Inspect before relying on anything.** Before you edit, open every file, type and function this
   brief names.
   - Confirm that each one exists and behaves as described.
   - If something differs materially, write down the difference **before** changing code. Examples: a
     different signature, a missing module, behaviour already implemented, or a prerequisite prompt not
     merged.
   - If the difference changes what you would build, stop and report it instead of guessing.
2. **Follow the contracts here exactly.** Do not change public interfaces, wire formats, database
   schemas or security behaviour beyond what this brief specifies.
3. **Stay in scope.** No unrelated refactoring, renaming or reformatting. No new dependencies unless
   this brief names them. Add any new dependency to `[workspace.dependencies]` in the root
   `Cargo.toml` and reference it from crates with `{ workspace = true }`.
4. **Never weaken a security check, a gate test or an arch rule to make something pass.**
5. **Stop and report rather than invent.** Do this when the brief contradicts the code, an
   architectural decision is missing, or a prerequisite does not exist.
6. **Be truthful.**
   - Never claim a test passed unless you ran it and saw it pass.
   - Never present stubbed or partial work as complete.
   - Mark anything unfinished as unfinished.

## Repository rules

These come from `AGENTS.md`, which CI enforces.

- **Crate boundaries.** `domovoi-core`, `domovoi-proto` and `domovoi-engine` do no I/O. Only
  `domovoi-runner` spawns processes, always with argv vectors and never `sh -c`. Dependency edges follow
  `ARCHITECTURE.md` §4. `cargo xtask arch-check` enforces all of this.
- **Approvals.** `HumanApproval::new` is called only in `crates/domovoi-hub/src/auth/`, and
  `HumanApproval::restore` only in `crates/domovoi-store/src/`. The LLM tool catalogue stays closed.
  Nothing but an authenticated human approves an action.
- **Code rules.**
  - No `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!` or `dbg!` in library code.
  - No new `#[allow(...)]`.
  - `thiserror` in libraries; `anyhow` only in `domovoi` and `xtask`.
  - Every public item has a doc comment saying what it is for.
  - Non-test source files stay at or under 500 lines; web source files at or under 250 lines.
- **Data and time.** Never edit a merged migration; add a new one. Choose a migration's number by listing
  `crates/domovoi-store/migrations/` on your branch and taking the next unused number; never assume it
  from a plan or a predecessor. Renumber only your own unmerged file if the branch gained a migration
  first, and describe the migration by its contents, not its number. Time comes from the `Clock` port;
  tests advance the fake clock from `domovoi-testkit` and never sleep. Secrets are
  `secrecy::SecretString` and are never logged, put into prompts or returned from the API.
- **Web.** Read `web/DESIGN.md` before touching `web/`. Build only from the kit in
  `web/src/components/`. Use `useActionFeedback` for every mutation and `describeError` for every error
  message, and tokens only for styling.
- **Tests.** Name tests after the behaviour, not the function. Never delete or weaken `tests/gates.rs`.
  Review every `insta` snapshot you accept.
- **Branch and commits.**
  - Work on a branch named as the brief says.
  - Commit messages are one line in Conventional Commits form, with exactly one scope, for example
    `feat(store): ...`. No body and no trailers.
  - Committed files, commit messages and the PR description must not mention prompts, prompt numbers,
    the build plan, coding agents or AI assistance.
- **Checks to run.** Run them for what you changed only. CI runs the full suite.
  - `cargo fmt --all`
  - `cargo clippy -p <crate> --all-targets -- -D warnings` for each crate you edited
  - `cargo nextest run -p <crate>` for each crate you edited, or `cargo test -p <crate>` if nextest
    is not installed
  - `cargo nextest run -E 'rdeps(<crate>)'` when you changed a public item that other crates use
  - `cargo xtask arch-check` when you added or moved modules or touched a `Cargo.toml`
  - for `web/`: `npm run lint`, `npm run typecheck`, and the vitest files for the changed area
  - On Windows, the hub and agent run only under WSL 2; unit and integration tests run natively.

## Completion report

End with a report in exactly this structure:

1. **Implemented behaviour**: what now works, in observable terms.
2. **Files**: added, changed and removed, each with one line on why.
3. **Implementation decisions**: any choice the brief left to repository convention, and what you chose.
4. **Interfaces**: types, functions, endpoints and messages introduced or changed, with signatures.
5. **Database and protocol changes**: migrations (file names), schema changes, wire changes.
6. **Security**: how each security requirement in the brief is met, and where it is tested.
7. **Tests added**: the test names, and which acceptance criterion each covers.
8. **Commands run**: each exact command and its actual result (pass, fail, counts).
9. **Not run**: tests or checks you did not run, and why.
10. **Deviations**: every place you departed from the brief, and why.
11. **Known limitations and remaining work**.
