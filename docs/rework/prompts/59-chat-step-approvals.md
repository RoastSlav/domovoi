# 59 — Approve and deny an operation's proposed actions from chat

| | |
|---|---|
| Branch | `chat-step-approvals` |
| Milestone | M3 |
| Depends on (must be merged) | 27 (step endpoints, `Notify` handler, `APPROVAL_POINTER`), 26 (operation writer, `ExecutionRecord`, `executions` table), 28 (merged before 27; this prompt adds no SSE code). Prompts 26, 27 and 28 are written but may not be merged when this brief is read: **every name taken from them is "introduced by prompt NN — verify in code before use"**. Prompt 18 (structured approvals, merged) and 22 (plan hash v2 and `Authorization`, merged) are real code |
| Must merge before | 58 (`remove-legacy-triage-path`): 58 deletes the legacy proposals that today give every Discord card its Approve and Deny buttons |
| Can run in parallel with | 30, 29 (order-flexible, never concurrent; this prompt touches no `web/` source) |
| Produces (shared contracts) | `OperationHandle::rotate_chat_token`; an extra `chat_token: Option<&str>` parameter on `OperationHandle::authorize`; `OperationError::BadToken`; `ExecutionError::NeedsTypedConfirmation`; `ExecutionRecord::{risk, chat_token_hash}` and the two new `executions` columns; `EventKind::ExecutionChanged`; `domovoi_engine::chat_token::{mint_token, token_hash, chat_token_ok}` (moved out of `triage`); `CardLinks.steps`, `WaitingStep`, `step_offers_chat` in `domovoi_engine::cards` |
| Consumes | the structured approval card, its `plan_prefix` and the Discord custom ID of prompt 18; `PlanBinding::Prefix`; `chat_authorization`/`chat_approval` in `crates/domovoi-hub/src/auth/approval.rs`; the single-use chat token of the legacy action path (`chat_token_hash`); the operation writer and `OperationRepo` of prompt 26; `Notify` and `report_view` of prompt 27 |
| Launch gate | none beyond the dependencies |
| Suggested model | large: it touches the approval path (core record, writer, hub, cards, notify) and must keep INV-1 and INV-6 intact |

## 1. Objective

After this change, a Discord card for an incident shows **real Approve and Deny buttons** for every Execution step the investigation proposed, exactly as it shows them today for a legacy action:

- the button labels name the skill and the host (`Approve restart_container on nas`), the card lists the plan block (skill, host, risk, every argument, `Plan <8 hex>`, expiry), and the custom ID carries a 6-hex prefix of the step's **v2** plan hash (prompt 18's format, unchanged);
- a press authorizes the step through `OperationHandle::authorize` with a **human** `Authorization` built by the hub's auth layer, a single-use chat token and the plan-hash prefix; a press of Deny refuses the step through `OperationHandle::deny`;
- destructive steps, steps whose risk is not known, and steps whose arguments do not fit the card never get chat buttons: they keep a web link (INV-6, fail closed);
- the 27 text pointer ("approve on the Operations page") appears only when at least one waiting step could not get buttons.

It does **not** add an approval path other than the button, a chat command, a model tool, a web screen, a wire change or a policy decision. It does not remove anything from the legacy action path (58 and the removal prompt do that). Discord is the only chat platform in the code (`crates/domovoi-chat/src/` holds only `discord/`); the work is platform-neutral because it lives in the shared `Card` and the hub's interaction authorizer, so no Discord source file changes.

## 2. Architectural context

Everything needed is here; the cited documents are background.

**RW-11 / D-02 (approval stays human-only, ADR 0007, INV-1).** Only an authenticated human approves. In chat the human is a platform user on the allowlist (`[discord] allowed_user_ids`) who presses a button; the hub's interaction authorizer (`crates/domovoi-hub/src/auth/interactions/`) acts as the admin account and records the chat user in the approval. `HumanApproval::new` is called only in `crates/domovoi-hub/src/auth/` (the helpers `chat_authorization` and `chat_approval` in `approval.rs`); this prompt calls those helpers and adds no new constructor. Thread text never approves.

**RW-22 / prompt 18 (what the approver sees).** The card is built by deterministic code from the stored plan, never from model prose. A Discord button's `custom_id` is limited to 100 characters, so an approval carries the first 3 bytes (6 hex) of the plan hash as a staleness check (`PlanBinding::Prefix`); the **single-use chat token** is the authenticator. Prefix binding is not a runtime boundary against a compromised hub: the runtime limits stay the dispatch-time recomputation of the v2 hash, the agent's local checks and the audit log. Say so honestly in docs; never claim script or target binding for the prefix alone.

**RW-36 (identity).** A step's id is its execution id, and an execution id never equals an action id. The card's approve button carries an `ActionId` field; for a step it holds `ExecutionId::as_action(step id)` (existing conversion, `crates/domovoi-core/src/id.rs:98-104`). The hub tells the two apart by looking the id up as an execution first and as a legacy action second; the two id spaces are disjoint, so one lookup never shadows the other.

**RW-08 / ADR 0022 (one authorization path per kind).** A step becomes `Authorized` only through `OperationHandle::authorize` (prompt 26). Prompt 27 calls it from one place (the web route). After this prompt there are exactly two call sites in production code: `crates/domovoi-hub/src/api/operations.rs` (web, `PlanBinding::Full`, no token) and `crates/domovoi-hub/src/auth/interactions/buttons.rs` (chat, `PlanBinding::Prefix`, token required). Every other mention is a test or a definition. A gate enforces the list.

**The single-use chat token (existing mechanism, legacy path).** `mint_token()` creates 244 random bits as hex; `token_hash(token)` is SHA-256; only the hash is stored (`actions.chat_token_hash`, a `BLOB UNIQUE` column of `0001_init.sql:125`); the plain token travels once, in the `TriageCompleted { tokens }` event, to the card builder, and from there in the button's custom ID. The writer's `approve` checks it for a chat approval before the plan binding, compares in constant time (`actions/approvals.rs:45-65`), and clears the stored hash in the same commit that approves or denies (`actions/writer.rs:357, 382`) or expires the action (`actions/expire.rs:33, 115`). A prefix mismatch returns before the token is touched, so the token survives. **This prompt reuses all of that for steps and changes none of it for actions.**

**Decisions made here (do not revisit).**

1. **The token is stored per step, on the `executions` row**, as a new column `chat_token_hash` (mirror of the legacy column). The legacy `actions.chat_token_hash` cannot hold it: the step has no `actions` row (RW-32/RW-36 forbid inventing one). A separate token table was rejected: the `executions` row is already written only by the operation writer, which gives one commit for "authorize and spend the token".
2. **The step's risk is stored on the `executions` row**, as a new nullable column `risk`, written once at proposal from the skill the writer validated. INV-6 needs a risk at press time and at card-build time; the card builder and the hub must not depend on a live agent session (a host that disconnected would silently turn a write skill into "unknown" or, worse, a changed manifest into a stale answer). The v2 plan hash binds the implementation hash, which covers the manifest and therefore the declared risk, so a skill that changes its risk after proposal changes the hash and dispatch refuses it. A `NULL` risk (a row written before this change) means unknown, which means **web only with typed confirmation** (fail closed).
3. **Typed confirmation is enforced in the core record**, not only in the route: `ExecutionRecord::authorize` refuses an approval whose channel is not `Web { typed_confirmation: true }` when the stored risk is not `Read` or `Write`. This is the equivalent of the legacy `ActionError::DestructiveNeedsTypedConfirmation`, and it makes a chat approval of a destructive or unknown-risk step impossible whatever a caller does (defence in depth for INV-6). Prompt 27's route check stays; this adds the lower layer.
4. **Deny needs no token**, exactly as the legacy deny (`actions/writer.rs:370-392` takes no token). Authentication is the allowlist and the admin-account check of the interaction authorizer; a deny only ever makes the system do less.
5. **Tokens are minted where the card is made, by `Notify`** (prompt 27's handler), one per waiting step that may have chat buttons, rotated through a writer command so only the hash is stored. `Notify` is delivered at least once, so a redelivery mints and rotates again; the newest token is the one on the newest card. A card refresh caused by another event (ack, mute, a later step) carries no token and therefore shows the web link for waiting steps: the same behaviour the legacy path has today (`delivery.rs:297-345` builds refresh cards from an empty token map). It is a known limitation, listed in section 14.
6. **The card refreshes when a step is decided.** The operation writer publishes a new domain event `ExecutionChanged` after a commit that moves an Execution out of "awaiting approval" (approved, denied, expired, cancelled, refused). The notifier turns it into a card update (buttons disappear). Without it a decided step's buttons would stay on the Discord card.
7. **Chat can approve only `Read` and `Write` steps whose arguments fit the card.** The builder and `Notify` share one pure predicate, `step_offers_chat`, so the card and the pointer text never disagree.
8. **Two token helpers move** out of `crates/domovoi-engine/src/triage/proposals.rs` into a neutral module `crates/domovoi-engine/src/chat_token.rs`, because prompt 58 deletes the legacy proposals (and `triage`'s exports) and prompt 59's step path needs the token primitives afterwards.

## 3. Repository context

Verify every item first. Marks: **(existing, path:line)** at the time of writing, **(introduced by prompt NN)** or **(new)**.

### 3.1 The chat approval path today (legacy actions)

- `custom_id(incident, &CardButton)` builds `dv1:a:{inc}:{action}:{token}:{prefix4}` (exactly 100 characters for a 32-byte token) and `parse_custom_id` parses it **(existing, `crates/domovoi-chat/src/discord/ids.rs:60-130`)**. `components.rs:34-70` labels buttons `Approve {skill} on {host}` from the card's `plans` entry with the same `action` id. **Nothing in `domovoi-chat` changes**: a step's button is an ordinary `CardButton::Approve { action, token, plan_prefix }`.
- `CardButton::{Approve{action, token, plan_prefix}, Deny{action, token}, OpenInWeb{action}}` and `CardPlan { action, skill, host, risk, args, plan_short, expires_at }` with `ARGS_BUDGET = 500` and `args_text()`, `Card.plans` (`MAX_PLANS = 3`) **(existing, `crates/domovoi-core/src/card.rs`)**.
- `build_card(incident, report, actions, host, links, latest)`, `CardLinks { web_url, tokens: BTreeMap<ActionId, String> }`, `waiting`, `plan_for`, `waiting_plans_and_buttons` **(existing, `crates/domovoi-engine/src/cards.rs:27-34, 86-170, 175`)**. The rule: a `Proposed` action with `Risk::Read | Risk::Write`, a token and `args_text().is_some()` gets `Approve` (with `plan_prefix` = first 3 bytes of `plan_hash_for_approval()` as hex) and `Deny`; every other waiting action gets `OpenInWeb`.
- Hub: `InteractionAuthorizer::button`, `approve`, `deny`, `load`, `approval_reason`, constants `WEB_ONLY`, `STALE_CARD` **(existing, `crates/domovoi-hub/src/auth/interactions/buttons.rs:19-22, 48-70, 74-112, 152-214`)**. `approve` loads the action, refuses `Risk::Destructive` with `WEB_ONLY`, turns `plan_prefix` into `PlanBinding::Prefix` (3 bytes, else `STALE_CARD`), builds `(authorization, provenance) = chat_authorization(admin, platform, platform_user, now)` and calls `deps.actions_service.approve(action, authorization, Some(token), binding, provenance)`.
- `chat_authorization`, `chat_approval`, `web_authorization`, `web_approval` **(existing, `crates/domovoi-hub/src/auth/approval.rs`; do not edit: the only `HumanApproval::new` call sites)**.
- Token primitives: `mint_token`, `token_hash`, `PROPOSER` **(existing, `crates/domovoi-engine/src/triage/proposals.rs:21-36`, re-exported at `triage/mod.rs:49`)**; the check `chat_token_ok(chat_token, &ActionRequest)` and the constant-time `same` **(existing, `crates/domovoi-engine/src/actions/approvals.rs:45-65`)**, which imports `crate::triage::token_hash`.
- Delivery: tokens reach the card through `EventKind::TriageCompleted { outcome, tokens }` (`events.rs:46-52`) → `Delivery::event` (`crates/domovoi-hub/src/runtime/delivery.rs:81-95`) → `Delivery::card(incident, tokens)` which lists the incident's actions and calls `build_card` with `CardLinks { web_url, tokens }` **(`delivery.rs:297-345`)**. `Delivery` holds `store: Arc<Store>`; `ActionRepo::list(&*self.store, ..)` is how it reads actions, so it can read `OperationRepo` the same way once `Store` implements it (prompt 26).
- The notifier plans a card update on `ActionChanged` (`crates/domovoi-engine/src/notify/plan.rs:230-239`); `EventKind` is exhaustive in `plan.rs` and `crates/domovoi-hub/src/api/events.rs:30-60` (28 may have replaced the latter: handle whichever mapping exists).
- Legacy tests that show the expected behaviour and that must stay green: `crates/domovoi-hub/tests/hub/interactions.rs` (`approve_with_valid_token_approves_once`, `reused_token_is_rejected`, `chat_approve_with_old_button_without_prefix_is_refused`, `chat_approve_with_wrong_prefix_is_refused_and_keeps_the_token`, `deny_and_details`; helpers `proposed`, `approve`, `press`, `chat`, `status_of`), `crates/domovoi-engine/tests/services/{approve.rs,plan_binding.rs}`, `crates/domovoi-hub/tests/gates.rs:773-840` (`destructive_action_cannot_be_approved_from_discord`, `discord_message_text_can_never_approve`).

### 3.2 What prompts 26 and 27 provide (introduced by prompt 26 / 27 — verify every name)

- `ExecutionRecord { execution, operation, step, host, contract, contract_version, check, target, args, plan_hash (v2), expires_at, approval (private), ... }`, `ExecutionRecord::{propose, authorize, approval, state, restore}`, `ExecutionParts`, `ExecutionError { AlreadyAuthorized, Expired, PlanChanged, NotYetSupported }`, `ExecutionState` (`AwaitingApproval` when the step is `Pending`, has no approval and its `Approval` step is `Pending`) **(26, `crates/domovoi-core/src/operation/execution.rs`)**. `ExecutionRecord::authorize(authorization, provenance, &binding, now)` mirrors `ActionRequest::approve`: policy → `NotYetSupported`, `binding.matches(&plan_hash)` else `PlanChanged`, an earlier approval → `AlreadyAuthorized`, `now >= expires_at` → `Expired`. **Check in step 1 that a `PlanBinding::Prefix` is accepted** (the legacy `ActionRequest::approve` accepts it and records the stored hash as the displayed one, `transition.rs:95-101`); if 26 rejects a prefix, make it accept one with the same rule (this is the one permitted edit of that method besides 6.1).
- `OperationHandle::{authorize(op, step, Authorization, PlanBinding, ApprovalProvenance), deny(op, step, HumanApproval), ...}`; `OperationError { NotFound, Decide(DecideError), Execution(ExecutionError), HostUnavailable, Conflict, StaleEffect, Store, Unavailable }`; `DecideError::StepNotOpen`; the operation writer (`crates/domovoi-engine/src/operations/writer/`), which runs `ExecutionRecord::authorize` then `ExecutionAuthorized`, audits `execution.approved` / `execution.denied` / `execution.expired` in the same commit; `OperationRepo::{get, load, for_incident, execution(ExecutionId), ..}`; `OperationSnapshot { operation, version, steps, edges, executions, incidents }`; the `Wake{purpose "approval_expiry"}` effect; `OperationDeps` (ports only, no bus today) **(26)**.
- 27: the `Notify` handler and `report_view(snapshot, turn, &dropped)`, `APPROVAL_POINTER`, `ManagerHandle::attach_triage(incident, outcome, BTreeMap::new())`; the web routes `approve_step` / `deny_step` and `operation_error(OperationError) -> ApiError` in `crates/domovoi-hub/src/api/operations.rs`; the gate `step_approval_is_the_only_route_to_authorized`; tests `no_chat_token_is_minted_for_a_step` and `report_tells_the_reader_where_to_approve` (both change here, section 12.4); `ApiDeps.operations: Option<OperationHandle>` and `ApiDeps.operation_repo: Arc<dyn OperationRepo>`; `InvestigateDeps { triage }` with `triage.shared().deps.catalog`.
- 26's rule that no `INSERT`/`UPDATE` of `executions` exists outside `crates/domovoi-engine/src/operations/` and the store's repo file, scanned by a gate in `crates/domovoi-engine/tests/gates.rs`.

### 3.3 Excerpt to change: the legacy approve in `buttons.rs` (existing, `buttons.rs:152-186`)

```rust
let found = self.load(incident, action).await?;
if found.risk() == Risk::Destructive { return Err(WEB_ONLY.to_owned()); }
let binding = plan_prefix.and_then(|t| hex::decode(t).ok()).filter(|b| b.len() == 3)
    .map(PlanBinding::Prefix).ok_or_else(|| STALE_CARD.to_owned())?;
let (authorization, provenance) = chat_authorization(actor.admin.id, actor.platform, actor.platform_user, deps.clock.now());
let approved = deps.actions_service.approve(action, authorization, Some(token), binding, provenance).await
    .map_err(|e| approval_reason(&e))?;
```

## 4. Exact scope

| Crate | Change |
|---|---|
| `domovoi-core` | `ExecutionRecord` gains `risk: Option<Risk>` and `chat_token_hash: Option<[u8; 32]>` with accessors and mutators; `ExecutionParts` gains both; `ExecutionRecord::propose` takes the risk; `ExecutionRecord::authorize` enforces typed confirmation (decision 3); `ExecutionError::NeedsTypedConfirmation`; tests |
| `domovoi-store` | one migration; the executions repo reads and writes the two columns; the uniqueness check for a token hash; contract and upgrade tests |
| `domovoi-testkit` | the in-memory `OperationRepo` round-trips both fields; a fixture builder for a waiting execution **(new)** `waiting_execution(...)` if 26's harness lacks one |
| `domovoi-engine` | `chat_token.rs` **(new)** (moved helpers); `triage/proposals.rs` and `triage/mod.rs` lose them (re-export removed; every importer updated); `actions/approvals.rs` imports from `chat_token`; `cards.rs`: `WaitingStep`, `step_offers_chat`, `CardLinks.steps`, merge into `waiting_plans_and_buttons`; `operations/`: `rotate_chat_token`, the extra `authorize` parameter and token check, token clearing, stored risk at proposal, `OperationError::BadToken`, publishing `ExecutionChanged`, `OperationDeps.bus`; `operations/waiting.rs` **(new)** `waiting_steps(repo, incident)`; `operations/investigate/notify.rs` mints tokens and passes them to `attach_triage`; `events.rs` gets `ExecutionChanged`; `notify/plan.rs` plans a card update for it |
| `domovoi-hub` | `auth/interactions/buttons.rs`: step branch of `approve` and `deny`, `step_reason`; `runtime/delivery.rs`: `CardLinks.steps`; `runtime/orchestration.rs`: pass the bus into `OperationDeps`; `api/operations.rs`: the two call sites (extra argument `None`) and the new error arms; `api/incidents.rs`: `CardLinks.steps` for the web card (read only); `api/events.rs` (or 28's mapping): ignore the new event; `examples/dev_hub.rs` and `tests/hub/api_harness.rs` if they build `OperationDeps` |
| docs | section 15 |

Files expected to be added: `crates/domovoi-engine/src/chat_token.rs`, `crates/domovoi-engine/src/operations/waiting.rs`, `crates/domovoi-store/migrations/NNNN_execution_chat_tokens.sql` (take the next free number), tests of section 12 (`crates/domovoi-engine/tests/operations/chat_steps.rs`, `crates/domovoi-hub/tests/hub/step_interactions.rs`, both registered in their `main.rs`).

## 5. Explicit non-goals

| Not here | Owner / reason |
|---|---|
| Any change to `crates/domovoi-chat/` sources, the custom ID format, button labels or the embed | unchanged by design; the card shape is the same |
| A new chat platform, new chat commands, approval by typing a word | none exist; thread text never approves (INV-1) |
| Re-minting tokens on every card refresh | known limitation, matches the legacy behaviour (section 14) |
| Web screens, the approval card component | 30 |
| Removing legacy actions, their chat buttons, `ActionHandle::propose`, `chat_token_hash` on `actions`, the legacy approve/deny endpoints | 58 and the removal prompt (60); this prompt only **moves** the two helper functions and leaves the legacy path working |
| Policy permits, unattended execution | 39, 40, 45 |
| A wire or protocol change | forbidden (RW-42); none needed |
| `HumanApproval::new` anywhere new; a model tool; an approve path other than the two call sites of section 2 | INV-1 |
| Cancel, pause, resume, answer routes | 29 |

## 6. Required interfaces and contracts

Marks: **fixed** = names and semantics must match; **convention** = follow repository style. Every public item gets a doc comment saying what it is for. Never write a prompt number, "the build plan" or an RW/D id in code, comments, commits, the book or the PR text.

### 6.1 Core **fixed**

```rust
// crates/domovoi-core/src/operation/execution.rs (introduced by prompt 26; edit)
impl ExecutionRecord {
    /// The risk of the skill when the step was proposed; `None` when unknown (rows written
    /// before risk was stored). Unknown is treated like destructive wherever it matters.
    pub fn risk(&self) -> Option<Risk>;
    /// The hash of the single-use token a chat approval must carry, while one is outstanding.
    pub fn chat_token_hash(&self) -> Option<&[u8; 32]>;
    #[must_use] pub fn with_chat_token_hash(self, hash: [u8; 32]) -> Self;
    pub fn clear_chat_token_hash(&mut self);
    // propose(...) gains a `risk: Risk` argument (stored as Some(risk)); restore(parts) reads both new fields
}
pub enum ExecutionError { AlreadyAuthorized, Expired, PlanChanged, NotYetSupported,
    /// The stored risk is not read or write, so only a web approval with the typed skill name is allowed.
    NeedsTypedConfirmation }
```

`authorize` order (**fixed**, mirrors the legacy order): policy authorization → `NotYetSupported`; **risk gate**: when `risk()` is not `Some(Read | Write)` and the channel is not `ApprovalChannel::Web { typed_confirmation: true }` → `NeedsTypedConfirmation`; binding does not match → `PlanChanged`; earlier approval → `AlreadyAuthorized`; `now >= expires_at` → `Expired`. Nothing changes on any refusal. With a `Prefix` binding the recorded displayed hash is the stored hash (as `ActionRequest::approve` does).

### 6.2 Engine: token helpers **fixed**

```rust
// crates/domovoi-engine/src/chat_token.rs (new; code moved unchanged from triage/proposals.rs and actions/approvals.rs)
pub fn mint_token() -> String;                      // 244 random bits, hex (unchanged body)
pub fn token_hash(token: &str) -> [u8; 32];         // SHA-256 (unchanged body)
pub(crate) fn chat_token_ok(token: Option<&str>, stored: Option<&[u8; 32]>) -> bool;   // constant time; None on either side is false
```
`chat_token_ok` now takes the stored hash instead of an `ActionRequest`; the legacy caller passes `action.chat_token_hash()`. `pub mod chat_token;` in `lib.rs`, and `domovoi_engine::chat_token::{mint_token, token_hash}` are public (hub tests use them). The `triage` module no longer exports `mint_token` or `token_hash` (keep `PROPOSER` until 58); update every importer (`grep -rn "mint_token\|token_hash" crates --include=*.rs` and keep the list; the hub's agent-enrolment `token_hash` in `agent_server/tokens.rs` is a different function: do not touch it).

### 6.3 Engine: operation commands **fixed**

```rust
// crates/domovoi-engine/src/operations/ (introduced by prompt 26; edit)
impl OperationHandle {
    /// `authorize` with the chat token a chat approval must carry. The web passes `None`.
    pub async fn authorize(&self, op: OperationId, step: StepId, authorization: Authorization,
        binding: PlanBinding, provenance: ApprovalProvenance, chat_token: Option<&str>) -> Result<(), OperationError>;
    /// Replaces the chat token hash of a step that is still awaiting approval and unexpired.
    /// Refused with `Decide(StepNotOpen)` when the step is decided or not awaiting approval,
    /// `Execution(Expired)` when the window closed, `NotFound` when missing.
    pub async fn rotate_chat_token(&self, op: OperationId, step: StepId, hash: [u8; 32]) -> Result<(), OperationError>;
}
pub enum OperationError { /* existing variants */, /// A chat approval without the step's token, with a wrong one, or with one already used.
    #[error("the approval link is not valid")] BadToken }
```

Writer behaviour of `authorize`, in this order, all before any state change: load the snapshot and the execution; if `authorization.human_approval()` is `None` → `Execution(NotYetSupported)`; **if the approval's channel is `Chat` and `!chat_token_ok(chat_token, record.chat_token_hash())` → `BadToken`** (checked before the plan binding, so a bad token reveals nothing about the hash); then `ExecutionRecord::authorize` (risk gate, binding, window); on success `record.clear_chat_token_hash()` and the `ExecutionAuthorized` decision in **one commit** (token spent exactly when the approval is stored). A web authorization ignores `chat_token`.

The token is also cleared, in the commit that does it, by `ExecutionDenied`, `ExecutionExpired`, and any event that finishes the execution step without an approval (operation cancelled, step refused). Rotation writes one audit row `execution.token_rotated` (subject `execution:<id>`, detail `{operation, step}`, never a token or hash) and **no journal row** (nothing a viewer sees changes); if the unit of work requires a journal row with a state write, add an `operation.execution` note with `from == to` instead, and say so in the report.

`OperationDeps` gains `pub bus: broadcast::Sender<DomainEvent>` (convention: the type the action service's `ActionDeps.bus` has, `crates/domovoi-engine/src/actions/deps.rs:92`). After a commit that moved an execution out of awaiting approval, the writer sends `DomainEvent::new(EventKind::ExecutionChanged { execution }, &incident, true)` for the first linked incident (loaded through `OperationDeps.incidents`); a failed send or a missing incident logs at `debug` and never fails the commit.

```rust
// crates/domovoi-engine/src/events.rs: EventKind gains
/// An execution step left "awaiting approval": approved, denied, expired, cancelled or refused.
ExecutionChanged { execution: ExecutionId },
```
`notify/plan.rs`: `ExecutionChanged { .. } => card_changed(incident, &state)` (no thread reply). Every other exhaustive `match` on `EventKind` gets an arm that ignores it (let the compiler list them).

### 6.4 Engine: the card **fixed**

```rust
// crates/domovoi-engine/src/cards.rs
/// One Execution step waiting for approval, as the card needs it. Built from the stored record only.
pub struct WaitingStep { pub execution: ExecutionId, pub skill: String, pub host: HostName,
    pub risk: Option<Risk>, pub args: Vec<(String, String)>, pub plan_short: String,
    pub plan_prefix: String /* first 3 bytes of the v2 hash, 6 hex */, pub expires_at: Timestamp, pub created_at: Timestamp }
/// Whether a step may carry chat buttons: risk is Some(Read | Write) and its plan's `args_text()` fits.
pub fn step_offers_chat(step: &WaitingStep) -> bool;
// CardLinks gains:   pub steps: Vec<WaitingStep>,       // default empty; `tokens` is keyed by ExecutionId::as_action(step) for steps
```
`build_card` merges legacy `Proposed` actions and `links.steps` into one list ordered oldest first by `created_at` then id, takes at most `MAX_PLANS` (3) in total, and for each builds a `CardPlan` (for a step: `action = execution.as_action()`, `risk = step.risk.unwrap_or(Risk::Destructive)` so an unknown risk is displayed and treated as the most dangerous, `plan_short = step.plan_short`). Buttons: `Approve { action, token, plan_prefix: Some(step.plan_prefix) }` and `Deny { action, token }` when `step_offers_chat(step)` **and** `links.tokens` has the id; otherwise `OpenInWeb { action }`. The existing rule for legacy actions is unchanged. **Fixed:** a `WaitingStep` whose host differs from the incident's host is not passed to the builder (see 6.5).

```rust
// crates/domovoi-engine/src/operations/waiting.rs (new)
/// The incident's executions awaiting approval on `host`, oldest first, at most `limit`. Reads only.
pub async fn waiting_steps(repo: &dyn OperationRepo, incident: &Incident, host: &HostName, limit: usize)
    -> Result<Vec<WaitingStep>, StoreError>;
```
Implementation: `for_incident(incident.id())` → for each non-terminal operation `load` → each execution with `state(..) == ExecutionState::AwaitingApproval` and `host == incident.host()` and `now < expires_at` is not checked here (an expired step is settled by its wake effect; legacy `waiting()` does not check either). `skill` is the contract id without its `skill.` prefix (use the accessor `ContractId` offers; verify). `plan_prefix = hex(plan_hash.as_bytes()[..3])`, `plan_short = plan_hash.short()`.

### 6.5 Engine: `Notify` **fixed** (edit of prompt 27's handler)

After `report_view` and `cache.put`, **before** `attach_triage`:

```
let waiting = waiting_steps(repo, &incident, &host_name, MAX_PLANS)?;          // the turn's pending executions
let mut tokens = BTreeMap::new();   let mut web_only = 0;
for step in waiting {
    if !step_offers_chat(&step) { web_only += 1; continue; }
    let token = chat_token::mint_token();
    match handle.rotate_chat_token(op, step_id, chat_token::token_hash(&token)).await {
        Ok(()) => { tokens.insert(step.execution.as_action(), token); }
        Err(_) => { web_only += 1; }        // decided or expired meanwhile: no buttons for it
    }
}
```
then `manager.attach_triage(incident, outcome, tokens)`. The `APPROVAL_POINTER` constant keeps its text and is appended to `details` **only when `web_only > 0`** (one or more waiting steps without buttons). Steps beyond the third are `web_only`. A store failure in `waiting_steps` or a refused rotation never fails the effect: that step is `web_only` and the pointer says where to approve. Tokens are never logged, audited, journaled or stored in the outcome; they travel only in the `TriageCompleted` event to the delivery task, as for legacy actions.

### 6.6 Hub: the button handler **fixed**

In `buttons.rs`, `approve` and `deny` first try the step path:

```rust
async fn step_for(&self, incident: IncidentId, action: ActionId) -> Result<Option<StepTarget>, String>;
//  StepTarget { operation: OperationId, step: StepId, record: ExecutionRecord, skill: String }
```
`step_for`: `deps.operation_repo.execution(ExecutionId::from_action(action))`. `Ok(None)` → the caller continues with the **unchanged** legacy code. `Ok(Some(v))` → `load` its operation snapshot and require `incident` among `snapshot.incidents`, else `Err("That action no longer exists")`; `deps.operations == None` → `Err("Operations are not running; open the incident in the web UI")`; a store error → `Err("Something went wrong; try again")`.

Step approve (in this order, each failure returns before the next):

1. `record.risk()` is not `Some(Read | Write)` → `Err(WEB_ONLY)` (INV-6; the writer repeats the check).
2. `plan_prefix` → `PlanBinding::Prefix(3 bytes)` exactly as the legacy code (`None`, bad hex or length ≠ 3 → `STALE_CARD`); share the code in a small private helper used by both paths.
3. `(authorization, provenance) = chat_authorization(actor.admin.id, actor.platform, actor.platform_user, now)`.
4. `ops.authorize(operation, step, authorization, binding, provenance, Some(token)).await.map_err(|e| step_reason(&e))`.
5. `Accepted::new("approve", format!("execution:{step}"), format!("Approved {skill}")).with_detail(json!({"incident": .., "operation": ..}))`. The audit kind stays `chat.approve`; the detail never holds the token.

Step deny: `ops.deny(operation, step, chat_approval(actor.admin.id, actor.platform, actor.platform_user, now)).await.map_err(|e| step_reason(&e))`, `Accepted::new("deny", format!("execution:{step}"), format!("Denied {skill}"))`.

```rust
/// Why a chat press on a step was refused, in one line. Exhaustive over `OperationError`.
pub fn step_reason(err: &OperationError) -> String;
```

| `OperationError` | Text |
|---|---|
| `NotFound` | `That action no longer exists` |
| `BadToken` | `This button no longer works` |
| `Execution(PlanChanged)` | `This card is out of date; open the incident to review it` |
| `Execution(NotYetSupported)` | `Policy approvals are not supported yet; a person must approve` |
| `Execution(NeedsTypedConfirmation)` | `Approve this one in the web UI` |
| `Execution(AlreadyAuthorized)` | `Someone already decided this action` |
| `Execution(Expired)` | `The approval window has closed` |
| `Decide(_)` | `Someone already decided this action` |
| `Conflict`, `StaleEffect` | `This step was taken over; try again` |
| `HostUnavailable` | `The host's agent is not connected; try again later` |
| `Store(_)` | `Something went wrong; try again` |
| `Unavailable` | `The service that decides actions is restarting; try again` |

`approval_reason` (legacy) is unchanged. The web route's `operation_error` gains arms (never an exhaustive-match break): `BadToken` → `ApiError::Forbidden("the approval link is not valid")` (cannot occur for the web, which sends no token); `Execution(NeedsTypedConfirmation)` → `ApiError::Forbidden("type the skill's name to approve this action")`.

### 6.7 Hub: cards and events **fixed**

- `Delivery::card` (`delivery.rs:297`) fills `CardLinks.steps` with `waiting_steps(&*self.store, incident, &host.name, MAX_PLANS)`; on a store error it logs a warning and uses an empty list (the card still goes out, as it does for actions).
- `api/incidents.rs` (the web `IncidentDetail.card`, today `CardLinks::default()` at `:196`) also fills `steps`, with no tokens, so the markdown card and the web card list a waiting step and show `OpenInWeb`.
- `runtime/orchestration.rs` passes the event bus into `OperationDeps.bus`.

## 7. Ordered implementation steps

1. **Inspect and verify; report discrepancies before editing.** Open every file and symbol of section 3. Confirm prompts 26, 27 and 28 are merged (**if 26 or 27 is not merged, stop and report**) and write down each real name against the names above: `ExecutionRecord` and its constructor, `OperationHandle::{authorize, deny}`, the writer's authorize command, `OperationError`, `OperationRepo::execution`, `Notify` and `APPROVAL_POINTER`, the operation harness (`crates/domovoi-engine/tests/operations/harness.rs`), `OperationDeps`. Check: does `ExecutionRecord::authorize` accept `PlanBinding::Prefix`? Is a journal row required with an execution write? Is there an in-memory `OperationRepo` in testkit? How does the hub's `Store` expose `OperationRepo`? Run `ls crates/domovoi-store/migrations/` (take the next free number) and the engine, hub and store test suites once for a green baseline. Run `grep -rn "mint_token\|token_hash\|chat_token_ok" crates --include=*.rs`.
2. Create branch `chat-step-approvals` from the latest `overhaul-investigation`.
3. **Move the token helpers** (6.2): create `chat_token.rs`, delete them from `proposals.rs`, fix the re-export and every importer, change `chat_token_ok`'s parameter. No behaviour change: the whole engine and hub suites pass unchanged. Commit `refactor(engine): move the chat token helpers out of triage`.
4. **Core and store** (6.1, section 8): record fields, `authorize` order, `NeedsTypedConfirmation`, migration, repo mapping, testkit repo, contract tests and the N-1 upgrade test. Commit `feat(core): store each step's risk and chat token hash`.
5. **Operation writer** (6.3): stored risk at `propose_execution` (from the validated `SkillSummary`'s risk), `authorize` with the token, `rotate_chat_token`, clearing, `BadToken`, `OperationDeps.bus`, `ExecutionChanged`, the audit kind. Update the call sites of `authorize` (27's route and every test) with the extra `None`. Engine tests of 12.1. Commit `feat(engine): single-use chat tokens for operation steps`.
6. **Cards and notify** (6.4, 6.5): `WaitingStep`, `step_offers_chat`, `CardLinks.steps`, the merge in `build_card`, `waiting_steps`, the `Notify` change, the `notify/plan.rs` arm and the other `EventKind` matches. Tests of 12.2. Commit `feat(engine): chat buttons for steps waiting for approval`.
7. **Hub** (6.6, 6.7): `buttons.rs`, `step_reason`, `delivery.rs`, `incidents.rs`, `operation_error` arms, bus wiring, `dev_hub.rs` and `api_harness.rs` if they build `OperationDeps`. Tests of 12.3 and the gates of 12.5. Commit `feat(hub): approve and deny steps from chat`.
8. **Update the two 27 tests** named in 12.4 and extend the gates (12.5). Never weaken or delete a gate: change an expectation only where it is listed in 12.4.
9. **Docs** (section 15), then `cargo xtask docs-links`.
10. **Checks and the completion report.** `cargo fmt --all`; `cargo clippy -p domovoi-core -p domovoi-store -p domovoi-engine -p domovoi-hub -p domovoi-testkit --all-targets -- -D warnings`; `cargo nextest run -p domovoi-core -p domovoi-store -p domovoi-engine -p domovoi-hub -p domovoi-chat`; `cargo nextest run -E 'rdeps(domovoi-core)'` (a core public item changed); `cargo xtask arch-check`. No OpenAPI change is expected (no route, DTO or annotation changes): run `cargo xtask openapi` and confirm it leaves no diff.

## 8. Database and persistence

One additive migration, `NNNN_execution_chat_tokens.sql` (**fixed** contents; take the next free number by listing the directory on your branch; never edit a merged migration):

```sql
-- The single-use chat approval token of a step awaiting approval (hash only) and the skill's risk
-- when the step was proposed. Both are validated in Rust; a NULL risk means unknown.
ALTER TABLE executions ADD COLUMN chat_token_hash BLOB
    CHECK (chat_token_hash IS NULL OR length(chat_token_hash) = 32);
ALTER TABLE executions ADD COLUMN risk TEXT;
CREATE UNIQUE INDEX executions_chat_token ON executions (chat_token_hash)
    WHERE chat_token_hash IS NOT NULL;
```

SQLite refuses `ADD COLUMN ... UNIQUE`, hence the partial unique index. `risk` holds `read`, `write` or `destructive` (the names `Risk` already serialises to), with no `CHECK` (new enums are validated in Rust). Existing `executions` rows keep both columns `NULL`: unknown risk, no token. The migration touches no table with triggers or FTS rowids.

Transactions: the token spend is part of the authorize commit (one `Transaction`: the `executions` update with the version guard, the step progress, the audit row, the effects of 26's authorize). Rotation is one transaction (the `executions` update and one audit row). A unique-index violation on a rotated hash (a random collision) is a `CommitError::Constraint`: the writer returns `Store` and `Notify` treats the step as `web_only`. The store's executions repo maps `risk` through `Risk`'s name functions and refuses an unknown value as corruption (as the existing repos do).

**N-1 upgrade test (required):** use the existing upgrade-fixture mechanism (`grep -rn "fixture" crates/domovoi-store/tests crates/domovoi-hub/tests/hub/upgrade_flow.rs`): open the committed fixture database of the previous schema that already has an `executions` row (if the fixtures predate the operation tables, create the rows in a test through the previous schema's statements and then migrate), run all migrations, and assert the row is intact, `risk` and `chat_token_hash` are `NULL`, and a rotation on it works. Name: `upgrade_keeps_executions_and_adds_token_and_risk_columns`.

## 9. Protocol and compatibility

No wire change (`PROTOCOL_VERSION` stays 1.2), no agent change, no HTTP shape change (the two route edits pass `None` and add error arms). The Discord custom ID format is unchanged, so cards posted before the upgrade still parse; their step buttons do not exist (steps had no buttons before). One internal API change: `OperationHandle::authorize` gains a parameter (only prompt 27's route and tests call it).

## 10. Error handling and recovery

| Situation | Exact behaviour |
|---|---|
| Press Approve with the right token and prefix on an awaiting `Write` step | `authorize` commits: approval stored, token cleared, `dispatch` effect created, audit `execution.approved` with the user and `chat` channel; reply `Approved <skill>`; the card refreshes without buttons |
| Same press again | token is gone: `BadToken` → `This button no longer works` (or `Someone already decided` when the step already moved: the token check comes first, so `BadToken`) |
| Wrong token | `BadToken`; nothing changes; the right token still works |
| Right token, wrong prefix | `PlanChanged` → `This card is out of date; open the incident to review it`; the token is **not** consumed (the binding check runs before the commit) |
| Old button without a prefix | `STALE_CARD`; the service is not called |
| Destructive, unknown-risk (`NULL`) or arguments-too-long step | no chat buttons (web link); a crafted press is refused by the hub (`WEB_ONLY`) and, if it got past, by the record (`NeedsTypedConfirmation`) |
| Approval window closed before the press | `Expired` → `The approval window has closed`; the wake effect settles the step and clears the token |
| Step denied on the web, then a chat Approve | token cleared by the denial: `BadToken` |
| Operation cancelled or step refused while a card is open | token cleared in that commit; a press gets `BadToken` or `Someone already decided this action` |
| Press while `deps.operations` is `None` (orchestration switched off) | `Operations are not running; open the incident in the web UI` |
| Execution belongs to another incident than the card's | `That action no longer exists`; nothing changes |
| `Notify` redelivered | a new token replaces the old (the card goes out again with the new one); the old token no longer works |
| `rotate_chat_token` refused (decided or expired between draft and notify) | that step gets no token and no buttons; the pointer text is appended |
| Operation service closing | `Unavailable` → `The service that decides actions is restarting; try again` |
| Policy `Authorization` reaches `authorize` (cannot happen from chat) | `NotYetSupported`, nothing changes |
| Hub restarts between minting and pressing | the token hash is durable on the row, so the card's button still works; the card message itself lives in the chat platform |

## 11. Security requirements

- **INV-1.** Only a human approves a step: the button handler is reachable only after the allowlist and admin checks of `InteractionAuthorizer::deliver`; the authorization comes from `chat_authorization` (`HumanApproval::new` stays only in `crates/domovoi-hub/src/auth/`); no model tool, chat command or background task calls `authorize`. The gate of 12.5 lists the two permitted call sites. Thread text never reaches the approval path (`discord_message_text_can_never_approve` is extended to a step).
- **INV-6.** A chat approval of a destructive or unknown-risk step is impossible: not offered (builder), refused in the hub, refused in the core record. A web approval of such a step needs `typed_confirmation: true`.
- **Token.** Only the SHA-256 hash is stored; compared in constant time; single use; replaced on rotation; cleared on every terminal move of the step; never logged, audited, journaled, serialized into outcomes or returned by any endpoint; present only in the `TriageCompleted` event and the card's custom ID. Check order: policy case → token → risk gate → plan binding → state.
- **Plan binding.** The prefix names the **v2** plan hash (at least 3 bytes). It is a staleness check, not a boundary against a compromised hub; dispatch still recomputes v2 and refuses any difference. Say this in docs; never claim more.
- **No model text in the approval block.** The plan block comes from the stored record (skill, host, arguments, risk, hash, expiry); `args_text` already strips control characters and backticks (prompt 18). Unknown risk is shown as the most dangerous.
- **Fail closed.** `NULL` risk, an unreadable store, a missing operation service, an execution of another incident, a missing prefix: all refuse.
- **Audit.** Approve and deny are audited by the operation writer in the same commit; the chat layer adds its existing `chat.approve` / `chat.deny` row (subject `execution:<id>`). Rotation is audited without values.

## 12. Testing requirements

Names describe behaviour. Fakes that exist: `InMemoryStore`, `FakeClock`, `FakeAgentLink`, `ScriptedLlm`, `RecordingGateway` (`crates/domovoi-testkit/src/chat.rs`), the hub interaction harness (`api()`, `chat()`, `press()`, `interaction()`, `ALLOWED` in `crates/domovoi-hub/tests/hub/interactions.rs`) and prompt 26's operation harness. A helper that proposes a waiting execution for a test is **(new)** if 26's harness has none: add `waiting_execution(world, skill, risk)` to the operation harness and reuse it. Tests never sleep.

### 12.1 Engine: writer and token (`crates/domovoi-engine/tests/operations/chat_steps.rs`)

| Test | Setup, action, expected |
|---|---|
| `chat_authorize_with_the_right_token_and_prefix_approves_and_spends_the_token` | waiting `Write` step, rotate a token, `authorize` with a chat authorization, `Prefix(first 3 bytes)`, the token: approval stored with the chat channel, `chat_token_hash` is `None`, a `dispatch` effect exists, audit `execution.approved` |
| `chat_authorize_with_a_wrong_token_is_refused_and_changes_nothing` | `BadToken`; the step still awaiting; the right token still works afterwards |
| `chat_authorize_with_a_wrong_prefix_keeps_the_token` | `Execution(PlanChanged)`; a second call with the right prefix and the same token succeeds |
| `chat_authorize_checks_the_token_before_the_prefix` | wrong token and wrong prefix: `BadToken` |
| `chat_authorize_without_any_token_is_refused` | `chat_token: None` with a chat channel: `BadToken` |
| `web_authorize_ignores_the_chat_token` | web authorization with `Full` hash, `None`: approves; a token present on the row is cleared in the same commit |
| `chat_authorize_of_a_destructive_step_is_refused_by_the_record` | stored risk `Destructive`, valid token and prefix: `Execution(NeedsTypedConfirmation)`, nothing changes |
| `chat_authorize_of_an_unknown_risk_step_is_refused` | row with `risk = None`: same |
| `web_authorize_of_a_destructive_step_needs_typed_confirmation` | `typed_confirmation: false` refused, `true` approves |
| `authorize_with_a_policy_authorization_is_not_yet_supported` | still `NotYetSupported` with the new parameter |
| `rotate_replaces_the_token_and_the_old_one_stops_working` | rotate twice: only the newest works |
| `rotate_is_refused_for_a_decided_or_expired_step` | approved, denied, expired (advance `FakeClock` past `expires_at`): `Decide(StepNotOpen)` / `Execution(Expired)`; nothing stored |
| `token_is_cleared_when_the_step_is_denied_expired_or_cancelled` | each event, then a chat authorize: `BadToken` |
| `rotation_audits_without_the_token` | audit row `execution.token_rotated` exists; no audit, journal or effect payload contains the token text or its hash hex |
| `deciding_a_step_publishes_execution_changed` | approve, deny and expire each publish one `ExecutionChanged` for the linked incident on the bus; a failed publish does not fail the commit |
| `proposal_stores_the_skills_risk` | `propose_execution` for a `Write` skill stores `Some(Write)`; for a skill whose risk is destructive `Some(Destructive)` |
| `token_helpers_behave_as_before_the_move` | `mint_token` is 64 hex characters and fresh each time; `token_hash` is SHA-256 of the text; `chat_token_ok` is false for `None` on either side |

Core unit tests next to the code: `execution_authorize_risk_gate_order` (the order of 6.1 over a table), `execution_record_round_trips_risk_and_token_hash`.

### 12.2 Engine: cards and notify (`crates/domovoi-engine/tests/notify/cards.rs` and `tests/operations/chat_steps.rs`)

| Test | Expected |
|---|---|
| `card_lists_a_waiting_step_with_buttons_and_the_v2_plan_prefix` | one `CardPlan` with the step's skill, host, sorted args, `Plan <8 hex>` of the v2 hash and expiry; `Approve` with `plan_prefix` equal to the first 3 bytes of the v2 hash and `Deny`, both with `action == execution.as_action()` |
| `card_offers_a_web_link_for_a_destructive_unknown_or_long_step` | destructive, `risk None`, arguments over 500 characters, missing token: `OpenInWeb`, no `Approve`/`Deny`; the plan is still listed |
| `card_shows_at_most_three_plans_across_actions_and_steps` | 2 legacy `Proposed` actions and 2 steps: 3 plans, oldest first, the fourth absent |
| `card_ignores_a_step_on_another_host` | `waiting_steps` skips it |
| `unknown_risk_is_shown_as_destructive_on_the_card` | `plan.risk == Destructive` |
| `notify_mints_one_token_per_chat_capable_step_and_passes_them_to_the_card` | `TriageCompleted.tokens` has exactly the step ids that may have buttons, each a 64-hex token whose hash equals the row's `chat_token_hash`; replaces 27's `no_chat_token_is_minted_for_a_step` |
| `notify_appends_the_pointer_only_when_a_step_has_no_buttons` | all steps buttoned: no `APPROVAL_POINTER`; one destructive step: present; replaces the expectation of 27's `report_tells_the_reader_where_to_approve` |
| `notify_redelivery_rotates_the_token` | two deliveries: the first token no longer works, the second does |
| `notify_survives_a_refused_rotation` | step expired between draft and notify: no token for it, the pointer is appended, the effect completes |
| `execution_changed_updates_the_card` | `notify::plan` returns `UpdateCard` for `ExecutionChanged` and no thread reply |
| `markdown_card_names_a_waiting_step` | the `Waiting: ... · plan <short>` line for a step |

### 12.3 Hub (`crates/domovoi-hub/tests/hub/step_interactions.rs`, harness of `interactions.rs`)

Use a hub with orchestration enabled and a waiting `Write` step with a rotated token (helper `proposed_step(a, risk)` **(new)**, returning the step, its operation and the token; mirrors `proposed`).

| Test | Expected |
|---|---|
| `chat_approve_step_with_valid_token_authorizes_once` | reply `Approved restart_container`; the execution is `Authorized` with `approved_channel.type == "chat"` and the platform user; audit kinds include `chat.approve` for `chat:discord:<user>`; the audit detail holds no token |
| `chat_reused_step_token_is_rejected` | second press: `This button no longer works` |
| `chat_approve_step_with_wrong_prefix_is_refused_and_keeps_the_token` | `STALE`/out-of-date text; the right prefix then works |
| `chat_approve_step_with_old_button_without_prefix_is_refused` | `STALE_CARD`; nothing changes |
| `destructive_step_cannot_be_approved_from_discord` | `WEB_ONLY`; still awaiting approval |
| `unknown_risk_step_cannot_be_approved_from_discord` | risk `NULL`: `WEB_ONLY` |
| `chat_deny_step_refuses_the_execution` | reply `Denied ...`; approval step `refused`, execution `refused`, audit `execution.denied` |
| `chat_press_on_a_step_of_another_incident_is_refused` | `That action no longer exists`; unchanged |
| `chat_press_with_orchestration_off_points_to_the_web` | `deps.operations == None`: the text of 6.6 |
| `non_allowlisted_user_cannot_press_a_step_button` | `Not allowed`; unchanged |
| `discord_message_text_can_never_approve_a_step` | thread text with the word, the id and the token: no approval, token still stored (extends the legacy test at `gates.rs:808`) |
| `legacy_action_buttons_still_work_beside_steps` | the legacy tests listed in 3.1 still pass; a press on a legacy action id still reaches `actions_service` |
| `step_reason_covers_every_operation_error` | table of 6.6, one row per variant |
| `delivery_card_carries_step_buttons_after_triage` | a `TriageCompleted` with a step token produces a card whose Discord components include the step's Approve with the 100-character custom ID (use `RecordingGateway`; the id parses back to the same button) |
| `incident_detail_card_lists_the_waiting_step_without_buttons` | `GET /incidents/{id}`: `card.plans` has the step, buttons are `open_in_web` |

### 12.4 Existing tests that change (the only permitted edits)

| Test (prompt 27) | New expectation |
|---|---|
| `no_chat_token_is_minted_for_a_step` | renamed `notify_mints_one_token_per_chat_capable_step_and_passes_them_to_the_card` (12.2); the old assertion "empty tokens map" is replaced, not deleted: a destructive step still gets none |
| `report_tells_the_reader_where_to_approve` | the pointer is present only when a step has no buttons (12.2) |
| `step_approval_is_the_only_route_to_authorized` | the permitted call sites are `api/operations.rs` and `auth/interactions/buttons.rs` (12.5) |
| every test calling `OperationHandle::authorize` | passes `None` as the new last argument |

### 12.5 Gates (extend `crates/domovoi-engine/tests/gates.rs` and `crates/domovoi-hub/tests/gates.rs`; never weaken)

- `step_authorization_has_exactly_two_production_call_sites`: a source scan of `crates/domovoi-hub/src` finds `.authorize(` on an `OperationHandle` only in `api/operations.rs` and `auth/interactions/buttons.rs`; `crates/domovoi-engine/src` calls it nowhere outside `operations/` (definition); `HumanApproval::new` still only under `auth/`.
- `chat_cannot_authorize_a_step_without_the_record_gate`: a unit table in core asserts `ExecutionRecord::authorize` with a `Chat` channel refuses every risk except `Read` and `Write` (so removing the hub check alone cannot open the hole).
- `chat_token_never_reaches_audit_journal_or_effects`: the rotation and authorize tests of 12.1 scan every audit, journal and effect row for the token text (kept as one named gate-style test).
- `execution_rows_are_written_only_by_the_operation_writer`: 26's scan (no `INSERT INTO`/`UPDATE` of `executions` outside `operations/` and the store repo) still passes with the new columns.

## 13. Acceptance criteria

1. A Discord card for an incident with a waiting `Read` or `Write` Execution step shows Approve and Deny buttons labelled with the skill and host, a plan block, and a 100-character-or-shorter custom ID carrying the 6-hex prefix of the **v2** hash (`card_lists_a_waiting_step_with_buttons_and_the_v2_plan_prefix`, `delivery_card_carries_step_buttons_after_triage`).
2. Pressing Approve with the right token and prefix authorizes the step through `OperationHandle::authorize` with a human authorization and spends the token in the same commit (`chat_approve_step_with_valid_token_authorizes_once`, `chat_authorize_with_the_right_token_and_prefix_approves_and_spends_the_token`); a reused token, a wrong token and a wrong prefix are refused with the specified texts, and a wrong prefix keeps the token.
3. Deny refuses the step with no token (`chat_deny_step_refuses_the_execution`).
4. A destructive, unknown-risk or long-argument step has no chat buttons, and a forced press is refused by the hub and by the core record (`destructive_step_cannot_be_approved_from_discord`, `chat_authorize_of_a_destructive_step_is_refused_by_the_record`, `card_offers_a_web_link_for_a_destructive_unknown_or_long_step`).
5. `APPROVAL_POINTER` is appended only when a waiting step has no buttons; `Notify` never fails because a token could not be minted.
6. The card refreshes when a step is approved, denied, expired or cancelled (`deciding_a_step_publishes_execution_changed`, `execution_changed_updates_the_card`).
7. The token is stored only as a hash on the `executions` row, cleared on every terminal move, and appears in no audit, journal, effect or API output (`chat_token_never_reaches_audit_journal_or_effects`).
8. The legacy action path and its chat buttons behave exactly as before (the legacy tests of 3.1 pass unchanged); `mint_token` and `token_hash` live in `chat_token.rs` and the legacy path uses them from there.
9. `OperationHandle::authorize` has exactly two production call sites (`step_authorization_has_exactly_two_production_call_sites`); `HumanApproval::new` call sites are unchanged and `cargo xtask arch-check` passes.
10. One additive migration, an N-1 upgrade test, no wire change, no HTTP shape change; `cargo xtask openapi` leaves no diff.
11. `cargo clippy` for the five crates, the nextest runs of step 10 and the new tests pass; no gate was weakened and nothing is `#[ignore]`d.

## 14. Migration and compatibility

- **Existing installs.** The migration adds two nullable columns; steps proposed before it have unknown risk and no token, so they are approved on the web (with the typed skill name) until they expire or finish. New steps get buttons.
- **Legacy actions.** Untouched. Their buttons, tokens, endpoints and executor work as before; 58 and the removal prompt retire them later.
- **Known limitation (documented, not fixed here).** Tokens exist only on the card that `Notify` publishes. A refresh caused by another event (ack, mute, a later decision) rebuilds the card without tokens, so waiting steps then show the web link. The legacy path behaves the same today. A later change can mint on refresh.
- **Rollback.** The previous binary ignores the new columns; tokens simply stop working.
- **Agents, wire, config.** No change; no new configuration key.

## 15. Documentation requirements

- `docs/book/src/concepts/cards.md`: Approve and Deny buttons on a card now cover an investigation's proposed actions; what the button names; destructive or unknown-risk actions and very long arguments show a web link; the plan prefix is a staleness check.
- `docs/book/src/concepts/operations.md` (created by the operation prompt, extended by 27): replace "chat cannot approve steps yet" with how chat approval of steps works and its limits.
- `docs/book/src/concepts/actions.md`: one paragraph that the same buttons approve steps and legacy actions.
- `ARCHITECTURE.md` §15 (approval invariants): the two call sites of `OperationHandle::authorize`, the per-step single-use token, the core risk gate; §7.1 only if it mentions step approval. Do **not** touch `xtask/src/arch/rules.rs` and add no ADR (no rule changes); if `cargo xtask arch-check` reports one is needed, stop and report.
- No configuration reference change and no OpenAPI regeneration expected. Run `cargo xtask docs-links`.
- Do not edit `docs/rework/`. Do not mention prompts, the build plan or coding agents in any committed file.

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
