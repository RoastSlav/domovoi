# 29 — Hub use-case layer, module-edge rule and operation controls

| | |
|---|---|
| Branch | `use-case-layer` |
| Milestone | M3 |
| Depends on (must be merged) | 26, 28, 27 and 59, **in that order** (26, then 28, then 27, then 59 `chat-step-approvals`, then this prompt). 59 edits the same hub files (`auth/interactions/buttons.rs`, `api/operations.rs`) and calls `OperationHandle` directly from them; this prompt moves those calls into the services, so it must run after 59. 22 and 23 are merged. 24, 25, 26, 27, 28 and 59 may be written but not merged when this brief is read: **every name taken from them is "introduced by prompt NN — verify in code before use"**. If 59 is not merged when you start, stop and report (do not implement its scope). If prompt 58 (remove legacy triage path) is merged first, `retry_triage` has only the operation path: move what is there |
| Can run in parallel with | none (one branch at a time). 30 (operations UI) depends on this prompt; 58 is order-flexible with it (see above) |
| Produces (shared contracts) | The hub use-case layer `crate::services` (`Services`, `ServicesDeps`, `Caller`, `Surface`, `IncidentService`, `ApprovalService`, `OperationCommands`, `HostService`, `BriefingService`); the module-edge rules `MOD-1`, `MOD-2`, `MOD-3` and the `CALLER-1` cards in `cargo xtask arch-check`; HTTP `POST /api/v1/operations/{id}/cancel`, `POST /api/v1/operations/{id}/pause`, `POST /api/v1/operations/{id}/resume`, `POST /api/v1/operations/{id}/steps/{step}/answer`; DTOs `CancelOperationRequest`, `ResumeOperationRequest`, `BudgetExtensionDto`, `AnswerStepRequest`, `OperationControlDto`; `PauseReason::Manual`; audit kind `operation.answered`; an ADR |
| Consumes | 59: the chat step path in `auth/interactions/buttons.rs` and its call-site gate; 26: `OperationHandle::{cancel, pause, resume, answer, authorize, deny}`, `OperationRepo`, `OperationError`, `DecideError`, `Budgets`/`BudgetKind`, audit constants; 27: `api/operations.rs`, `api/dto_operations.rs`, `ApiDeps.operations`/`operation_repo`, `OperationSummaryDto`, `ExecutionDto`, `operation_error`, `OperationHandle::investigate_and_wait`; 28: `snapshot_seq(&ApiState)`, `ApiDeps.journal`; 19: `crates/domovoi-hub/src/settings/step_up.rs`; the existing `xtask/src/arch/` checker |
| Launch gate | none beyond the dependencies |
| Suggested model | medium: the refactor is mechanical but wide (hub, xtask, tests), and the control routes are small but security-relevant |

## 1. Objective

Do three things in `crates/domovoi-hub` (plus `xtask`):

1. **Introduce the use-case layer.** Create `crate::services`: plain Rust services that hold the hub's
   commands (acknowledge, mute, ask a follow-up, approve and deny an action, approve and deny a step,
   cancel, pause, resume and answer an operation, build a host's status, build the digest). Move the logic
   that today lives inside HTTP handlers and Discord interaction code into those services **without any
   change of behaviour**. After this change the HTTP transport (`api`) and the chat transport
   (`auth::interactions`) call services, and neither imports the other.
2. **Enforce it.** Add a module-edge rule to `cargo xtask arch-check` (rules `MOD-1`, `MOD-2`, `MOD-3`,
   plus the `CALLER-1` cards): transports may not import each other, services may not import a transport
   or the runtime, and a new top-level hub module must declare what it is.
3. **Own the operation control routes.** Add the four human-only routes that cancel, pause, resume and
   answer a question of an operation, built as services first and HTTP handlers second, with human
   authentication, audit, bounded and redacted input, and an OpenAPI regeneration. Prompt 30 (the UI banners)
   consumes exactly these routes and DTOs.

The refactor is the larger part by line count and must be provably invisible: every existing response, status
code, error text and audit row stays byte-identical, and the generated OpenAPI document changes **only by
addition** (the four new routes and their schemas).

With `[orchestration] enabled = false` (the default) the four new routes answer 503 "orchestration is not
enabled", exactly like prompt 27's step routes, and nothing else changes.

## 2. Architectural context

Everything the executor needs is here; the cited documents are background only.

**RW-19 (use-case layer in the hub).** Today `auth/interactions/*` imports `crate::api::*`: the Discord
authorizer calls HTTP handler helpers (`host_dto`, `summary_of_host`, `system::build`, `latest_triage`) and
holds an `ApiState`. The decision: HTTP handlers, Discord interactions and the future Operator call
**application services** (`approve_step`, `cancel_operation`, `edit_fact`, `publish_policy`,
`request_install`, ...), and an arch-check rule inside the hub forbids transports importing each other. A
transport does three things only: authenticate and parse (turn a cookie or a button press into a typed
request and a `Caller`), call one service method, and render the answer in its own format (a JSON status
code, a chat sentence). Everything between ("is this allowed, what exactly is written, which audit row")
belongs to the service. This prompt builds the layer for what exists now and the rule that keeps it honest;
later prompts (knowledge edits, policy publication, package install) add their services in the same place.

**RW-04, RW-05, RW-35, RW-36 (operations).** An operation is written only by the one operation writer
(prompt 26). `OperationHandle` is the only way in. Cancelling an operation cancels its pending effects and
steps but **cannot recall a dispatched execution**: a `Running` Execution step stays `Running` and its
result or reconciliation is still recorded (04 §3). Pausing stops new steps from being added; resuming
re-checks budgets and may raise a spent limit (`Resume { extend }`). Answering a question creates a `Note`
step with an `answers` edge and finishes the `Question` step.

**INV-1 and ADR 0007/0021 (approvals).** `HumanApproval::new` and `Authorization::human` are called only in
`crates/domovoi-hub/src/auth/` (enforced by `HUMAN_APPROVAL_ALLOWED_PREFIXES` and
`AUTHORIZATION_HUMAN_ALLOWED_PREFIXES` in `xtask/src/arch/rules.rs`). The services never mint an approval:
they call new helpers in `auth/approval.rs` that wrap the existing `web_authorization` and
`chat_authorization`. No chat command, model tool or background task approves, cancels, pauses, resumes or
answers anything for a person. The service layer therefore needs a value that says "an authenticated human
asked": the **`Caller`**. Only a transport builds one, only after its own authentication (a valid session
for the web, an allowlisted platform user for chat), and only two call sites in the whole hub may do so
(`CALLER-1`).

**INV-6 (typed confirmation).** A destructive skill, and a skill whose risk is unknown when the person
approves, needs the skill's name typed exactly; chat can never approve it. The rule stays enforced where it
is (the writer refuses without the typed channel) and the service adds the same check before the write
(defence in depth, same texts).

**INV-9 (model output is data).** The text a person types to answer a question reaches the model later as
data. It is bounded, redacted before it is stored, and never executed or interpreted as an instruction by
the hub.

**RW-24 and prompt 19 (step-up).** `crates/domovoi-hub/src/settings/step_up.rs` keeps two lists:
`STEP_UP_SETTINGS` (enforced) and `STEP_UP_ACTIONS` (declared, not enforced yet): `policy.publish`,
`capability.enable`, `installation.start`, `autonomy.resume`. **Decision (do not revisit):** cancel, pause,
resume and answer of an *operation* are **not** on that list and need no password re-entry: an owner session
is enough, exactly like approving an offered step (04 §10 threat table: "approve offered steps" is allowed
for a stolen session; step-up covers policy publish, locked rules, enable, install, retention and LLM CLI).
`autonomy.resume` is the future **global** pause switch of M8 and is a different thing from
`POST /operations/{id}/resume`. A test asserts that no `STEP_UP_ACTIONS` entry names an operation control, so a
later change of that decision is a visible edit to the list and to this layer together.

**RW-42 (no wire change).** Nothing here touches the agent protocol (stays 1.2) or the database.

## 3. Repository context

Verify each item before relying on it. Marks: **(existing, path:line)** or **(new)**; names from prompts
26, 27, 28 are **introduced by prompt NN — verify in code before use**.

### 3.1 The hub's modules and the edges that violate RW-19 today

`crates/domovoi-hub/src/lib.rs` declares `admin`, `agent_server`, `api`, `auth`, `csp`, `digest` (private),
`ingest`, `middleware`, `profile`, `router`, `runtime`, `selftest`, `settings`, `spa` **(existing,
`lib.rs:8-22`)**. Transports are `api` (HTTP and SSE), `auth::interactions` (Discord buttons, commands and
thread messages) and `ingest` (webhooks). The edges that cross between them, and the two that put a
transport's helper in a place a service cannot import:

| Edge | Where | Why it must go |
|---|---|---|
| chat → api | `auth/interactions/mod.rs:23` `use crate::api::ApiState`; `InteractionAuthorizer { api: ApiState, chat }` (`mod.rs:89-92`) | chat reaches every HTTP dependency |
| chat → api | `auth/interactions/buttons.rs:12` `use crate::api::incidents::latest_triage` | a pure read helper lives in an HTTP module |
| chat → api | `auth/interactions/commands.rs:11` `use crate::api::{hosts, profile, system}`, used at `:105` (`hosts::host_dto`), `:121` (`profile::summary_of_host`), `:207` (`system::build`) | chat renders HTTP read models |
| profile → api | `profile/mod.rs:34` `use crate::api::AgentAdmin` | the `AgentAdmin` trait is defined in `api/mod.rs:67-92` with its impl for `AgentServer` at `:94-128`, so a service could not use it without importing `api` |
| host status → api | `api/hosts.rs:18` `use super::version::{HUB_VERSION, is_older}` (`api/version.rs`) | the "outdated" rule lives under `api` |

`api/mod.rs` `ApiDeps` **(existing, `api/mod.rs:135-190`)** holds the raw repositories and the write handles
(`manager`, `actions_service: ActionHandle`, `triage: Option<TriageHandle>`); prompt 27 adds
`operations: Option<OperationHandle>` and `operation_repo: Arc<dyn OperationRepo>`, prompt 28 replaced `bus`
by `journal`. `ApiDeps` is built in three places: `runtime/mod.rs:306`, `examples/dev_hub.rs:501`,
`tests/hub/api_harness.rs:413`. `ApiState { deps, auth, shutdown }` **(`api/mod.rs:194-199`)**, `Authed(Current)`
extractor **(`:248-264`)**, `Current { user: User, session_hash: [u8; 32], csrf_token }` **(`auth/mod.rs:103-110`)**.

### 3.2 The handlers whose logic moves (verbatim behaviour; line numbers before prompts 27/28)

| Handler | Today | Moves to |
|---|---|---|
| `incidents::ack` **(`api/incidents.rs:225-237`)** | `manager.ack(parse_id, current.user.id)`, `CommandError` mapped by `command` (`:41-50`) | `IncidentService::ack` |
| `incidents::mute` **(`:251-273`)** | validates minutes `1..=MAX_MUTE_MINUTES`, `until = now + minutes`, `manager.mute` | validation stays in the handler; `IncidentService::mute(caller, id, until)` |
| `incidents::unmute` **(`:286-298`)** | `manager.unmute` | `IncidentService::unmute` |
| `incidents::ask` **(`:423-455`)** | validates the question, `triage` present else `TriageOff`, `tokio::spawn(triage.follow_up(..))`, `TriageError` mapped | `IncidentService::follow_up`; validation, the `tokio::spawn` and the error texts stay in the handler |
| `incidents::retry_triage` **(`:470-491`, as changed by prompt 27 §6.6)** | `triage` present else `TriageOff`, loads the incident, runs `investigate_and_wait` (operations on) or `triage/retriage` (off) in a spawned task | `IncidentService::retriage(&Incident, fresh)` |
| `actions::approve` / `deny` **(`api/actions.rs:118-155`, `:169-188`)** | `load` (404 first), parse `plan_hash` (400), `web_authorization`, `actions_service.approve/deny`, `approval_error` (`:19-46`) | `ApprovalService::{find, approve, deny}` |
| Chat buttons **(`auth/interactions/buttons.rs`)**: Ack `:83-89`, Mute1h `:90-99`, Approve `:152-187` (destructive → `WEB_ONLY`, prefix binding or `STALE_CARD`, `chat_authorization`), Deny `:189-213`, Details `:114-138` | direct `deps.manager`, `deps.actions_service` calls | the same service methods as the web, with `Caller::chat` |
| Chat commands **(`commands.rs`)**: `/status` `:73-134`, `/mute` `:163-198`, `/digest` `:200-220` | `hosts::host_dto`, `profile::summary_of_host`, `manager.mute`, `system::build` + `Notifier::send` | `HostService::status`, `BriefingService::profile_summary`, `IncidentService::mute`, `BriefingService::digest` |
| Chat thread message **(`mod.rs:137-154`)** | `triage.follow_up(..)` | `IncidentService::follow_up` |
| Chat step buttons **(introduced by prompt 59, `auth/interactions/buttons.rs`: `step_for`, the step branches of `approve` and `deny`, `step_reason`)** | `deps.operation_repo.execution(..)` lookup, then `OperationHandle::authorize(.., Some(token))` or `deny` with `chat_authorization`/`chat_approval` | `OperationCommands::{find_by_action, approve_step, deny_step}` with `Caller::chat` (the chat-only decisions and texts stay in `buttons.rs`, 6.2) |
| `hosts::host_dto` **(`api/hosts.rs:43-100`)** | builds `HostDto` from `hosts`, `incidents`, `agents.presence`, `hosts.live_agent`, `hosts.agent_report`, `protocol_name`, `is_older` | `HostService::status` returns a plain `HostStatus`; `host_dto` becomes a field-by-field mapper |
| `profile::summary_of_host` **(`api/profile.rs:39-52`)** | `profiles(state)` unavailable → `""` | `BriefingService::profile_summary` |
| `system::build` **(`api/system.rs:44-87`)** | `DigestService::next_due`, `crate::digest::build` | `BriefingService::digest` returns a plain `Briefing`; `build` maps it to `DigestPreview` |
| `incidents::latest_triage` **(`:113-123`)** | pure | `services::incidents::latest_triage` |
| Prompt 27's step handlers `approve_step`, `deny_step` **(introduced by prompt 27, `api/operations.rs`)** | `web_authorization`/`web_approval` then `OperationHandle::authorize/deny`; typed-confirmation check before the write | `OperationCommands::{approve_step, deny_step}` |

HTTP-only handlers **stay where they are** (they have no second transport and belong to the features that
will own them): `incidents::{list, detail, dismiss}`, `rules::*`, `notes*`, `settings*`, `hosts::{enrollment,
revoke, skills, logs}`, `profile::{get, refresh}`, `followups::*`, `chat::chat` (the Operator, replaced by a
later prompt), `overview`, `logs`, `tool_usage`, `updates`, `auth`, `readiness`, and all prompt 27/28 **read**
endpoints. The rule of the layer, written into the ADR: *commands go through a service; plain repository
reads may stay in the transport that renders them, but a read shared by two transports lives in a service.*

### 3.3 Approvals and the human-only helpers

`auth/approval.rs` **(existing, `auth/approval.rs:13-76`)**: `web_approval(user, confirm_text, skill, now)`,
`chat_approval(user, platform, platform_user, now)`, `web_authorization(user, confirm_text, skill, session_hash,
now) -> (Authorization, ApprovalProvenance)`, `chat_authorization(user, platform, platform_user, now)`. These
are the only constructors of `HumanApproval`/`Authorization::human` in the hub.

`ApprovalError` **(existing, `crates/domovoi-engine/src/actions/mod.rs:51`)**; the HTTP texts are in
`approval_error` (`api/actions.rs:19-46`) and the chat texts in `approval_reason`
(`auth/interactions/buttons.rs:48-71`, re-exported at `interactions/mod.rs:26`, tested at
`tests/hub/interactions.rs:386-405`). Both mappings stay in their transport.

### 3.4 What prompts 26, 27, 28 provide (introduced by — verify every name in code)

- 26: `OperationHandle` with `cancel(op, Actor, reason)`, `pause(op, Actor)`, `resume(op, Actor, extend)` where
  `extend: Vec<(BudgetKind, Option<u64>)>`, `answer(op, question, text, Actor)`, `authorize(op, step,
  Authorization, PlanBinding, ApprovalProvenance)`, `deny(op, step, HumanApproval)`;
  `OperationError { NotFound, Decide(DecideError), Execution(ExecutionError), HostUnavailable, Conflict,
  StaleEffect, Store, Unavailable }`; `OperationRepo::{get, load, list, for_incident, ..}`;
  `OperationSnapshot`; `BudgetKind { ModelTurns, EvidenceRequests, Executions, Cycles, WallTime, Tokens, Cost }`
  (snake_case serde); `PauseReason { Budget, OutcomeUnknown, GlobalPause, Error }`; `OperationStatus` and its
  `name()`; the decide rules for `Cancel`, `Pause`, `Resume`, `Answer` (26 §6.2); the audit constants in
  `crates/domovoi-engine/src/operations/audit.rs` (`operation.cancelled`, `operation.paused`,
  `operation.resumed`, ...; **no audit kind for an answer was planned**); `Actor::System(SYSTEM_OPERATIONS)`.
  26 plans **no `PauseReason` for a pause a person asks for**: section 6.7 adds `Manual`.
- 27: `api/operations.rs` (handlers `list`, `detail`, `trace`, `step`, `approve_step`, `deny_step`,
  `operation_error`), `api/dto_operations.rs` (`OperationSummaryDto`, `ExecutionDto`, `ApproveStepRequest`,
  `PauseReasonDto`, ...), `ApiDeps.operations`, `ApiDeps.operation_repo`, `ActionDto.legacy`, the error texts
  of 27 §6.6, and tests in `tests/hub/operations_api.rs`.
- 59: `OperationHandle::authorize(op, step, Authorization, PlanBinding, ApprovalProvenance, chat_token: Option<&str>)`,
  `OperationError::BadToken`, `ExecutionError::NeedsTypedConfirmation`, `ExecutionRecord::{risk, chat_token_hash}`, the chat step
  path in `buttons.rs` and its tests (`chat_approve_step_with_valid_token_authorizes_once` and siblings), and the gate
  `step_authorization_has_exactly_two_production_call_sites`. The web routes pass `None` for the token.
- 28: `crates/domovoi-hub/src/api/snapshot.rs` `pub(crate) async fn snapshot_seq(state: &ApiState) -> Result<u64,
  ApiError>` (read it **before** the data it describes), `ApiDeps.journal: Arc<dyn JournalFeed>`.

### 3.5 The arch checker

- `xtask/src/arch/mod.rs` `run` calls `graph::check`, `manifest::check`, `scan::check_tree`, optionally `git::check`.
  `scan::check_tree` builds the list of production files (`production: Vec<(String, String)>`, workspace-relative
  path and text) and ends with `out.extend(super::ast::exec_violations(&production))` **(existing,
  `xtask/src/arch/scan.rs:73`)**.
- `ast.rs` parses each file with `syn` and visits paths, struct expressions and macro token streams;
  `alias.rs` `aliases(&syn::File)` resolves `use` renames **(existing, `ast.rs:1-30`, `alias.rs:11-16`)**;
  `wiring.rs:280` `is_cfg_test`; `ast::cfg_test_closure(&files)` returns the test-only files **(`ast.rs:125`)**.
- Rule data lives in `xtask/src/arch/rules.rs` (`BOUNDARY_PAIRS` `:216`, `BoundaryCard` `:261`, `BOUNDARY_CARDS`
  `:278`, `EXEC_OPERATION_CRATES` `:396`). **Changing `rules.rs` requires a new ADR in `docs/adr/`** (`RULES_FILE`
  `:455`; enforced by `cargo xtask arch-check --base origin/main`). Existing ADRs end at `0023`; take the next
  free number on your branch.
- The hub gate tests call the same checker: `crates/domovoi-hub/tests/gates.rs:18-26`
  `transports_do_not_name_the_agent_link`, with `hub_sources()` (`:34-45`) returning the production files.
- `xtask/src/arch/ast.rs` is already at the 500-line limit: put new checker code in a **new file**.

### 3.6 Tests that exist and must keep passing unchanged

`tests/hub/interactions.rs` (chat: `non_allowlisted_user_is_rejected`, `ack_and_mute_reach_incident_manager`,
`approve_with_valid_token_approves_once`, `reused_token_is_rejected`, the two prefix tests,
`refused_chat_approval_explains_a_decision_already_made`, `deny_and_details`,
`thread_message_becomes_follow_up_and_reply`, `commands_status_incidents_and_mute_by_short_id`,
`status_of_one_host_when_the_hub_collects_no_profiles`), `tests/hub/api_routes.rs`
(`incident_detail_ack_and_mute` `:84`, `approve_and_deny_flow_through_approval_service` `:136`, the three
`approve_api_*` tests, `advisories_digest_and_metrics`, `digest_send_tells_a_refused_send_from_a_missing_notifier`,
`digest_preview_says_when_the_next_send_is_due`, `openapi_snapshot` `:529` with
`tests/hub/snapshots/hub__api_routes__openapi.snap`), `tests/hub/api_chat.rs`, `tests/hub/api_admin.rs`,
`tests/hub/operations_api.rs` (introduced by prompt 27). The chat harness builds
`InteractionAuthorizer::new(&a.state, ChatDeps { users, notify, audit, gateway, allowed })`
**(existing, `tests/hub/interactions.rs:62-71`)**.

## 4. Exact scope

### 4.1 `domovoi-hub` (production)

| File | Change |
|---|---|
| `src/lib.rs` | `pub mod services;` and `pub(crate) mod version;` |
| `src/version.rs` **(new, moved)** | content of `api/version.rs` (`HUB_VERSION`, `is_older` and its tests); `api/version.rs` deleted; `api/hosts.rs` and `api/updates.rs` import `crate::version` |
| `src/agent_server/admin.rs` **(new, moved)** | the `AgentAdmin` trait (`api/mod.rs:67-92`) and `impl AgentAdmin for AgentServer` (`:94-128`); `agent_server/mod.rs` declares and re-exports it; `api/mod.rs` keeps `pub use crate::agent_server::AgentAdmin;` so `domovoi_hub::api::AgentAdmin` (used by `examples/dev_hub.rs:36`) still resolves; `profile/mod.rs:34` imports it from `crate::agent_server` |
| `src/services/mod.rs` **(new)** | module doc (what a service is, the three-line rule of 2), `Services`, `ServicesDeps`, re-exports |
| `src/services/caller.rs` **(new)** | `Caller`, `Surface` |
| `src/services/incidents.rs` **(new)** | `IncidentService`, `FollowUpError`, `RetriageError`, `latest_triage` |
| `src/services/approvals.rs` **(new)** | `ApprovalService`, `ApproveInput`, `ApproveError` |
| `src/services/operations.rs` **(new)** | `OperationCommands`, `OperationCommandError`, input limits; split into `operations/{mod,steps,controls,limits}.rs` if the file would pass 500 lines |
| `src/services/hosts.rs` **(new)** | `HostService`, `HostStatus`, `OPEN_COUNT_CAP` (moved from `api/hosts.rs`) |
| `src/services/briefing.rs` **(new)** | `BriefingService`, `Briefing`, `BriefingHost`, `BriefingError` |
| `src/auth/approval.rs` | **add** `authorization_for(&Caller, confirm_text, skill, now) -> (Authorization, ApprovalProvenance)` and `approval_for(&Caller, confirm_text, skill, now) -> HumanApproval` (each dispatches on `Surface` to the existing helpers; existing functions untouched) |
| `src/api/mod.rs` | `ApiDeps` gains `services: Services`, loses `actions_service` and (prompt 27's) `operations`; `Authed::caller(&self) -> Caller`; `ApiState::services(&self) -> &Services`; four routes; `AgentAdmin` re-export; tag text for `operations` (27 adds the tag) |
| `src/api/actions.rs`, `api/incidents.rs` (ack, mute, unmute, ask, retry_triage), `api/hosts.rs` (`host_dto`), `api/profile.rs`, `api/system.rs` (`build`), `api/tool_usage.rs` (test import of `latest_triage`) | handlers call services; mapping and validation stay |
| `src/api/operations.rs` (prompt 27) | `approve_step`/`deny_step` call `OperationCommands`; **new handlers** `cancel`, `pause`, `resume`, `answer` |
| `src/api/operation_errors.rs` **(new)** | `operation_error(OperationError) -> ApiError` (moved from 27's `api/operations.rs`, texts unchanged) and `command_error(OperationCommandError) -> ApiError` |
| `src/api/dto_operations.rs` (prompt 27) | add `CancelOperationRequest`, `ResumeOperationRequest`, `BudgetExtensionDto`, `AnswerStepRequest`, `OperationControlDto` (6.4) |
| `src/auth/interactions/mod.rs`, `buttons.rs`, `commands.rs` | no `crate::api` import; `InteractionAuthorizer { services: Services, chat: ChatDeps }`; `ChatDeps` gains read ports (6.3); `Actor::caller()` |
| `src/runtime/mod.rs`, `src/runtime/adapters.rs` | build `Services` once from the values the runtime already holds; `gateway_sinks` takes `&Services` |
| `examples/dev_hub.rs`, `tests/hub/api_harness.rs` | construct `Services` and the new `ApiDeps` field |
| `src/settings/step_up.rs` | **doc comment only**: names the operation controls as deliberately not step-up actions (no list change) |

### 4.2 `domovoi-core`, `domovoi-engine` (the two permitted edits of prompt 26's code)

| File | Change |
|---|---|
| `crates/domovoi-core/src/operation/status.rs` | `PauseReason::Manual` (unit variant, serde `{"type":"manual"}`), unit test of its JSON; every `match` over `PauseReason` the compiler flags |
| `crates/domovoi-engine/src/operations/` (`decide`, `writer`, `audit.rs`, `journal.rs`) | `OperationHandle::pause` uses `PauseReason::Manual`; `Resume` of a `Manual` pause needs no budget check; audit kind `operation.answered` (actor the human, detail `{operation, step}`, **never the text**) created in the same transaction as the answer. **Skip an item if the merged 26 already has it** and say so in the report (step 1) |

### 4.3 `xtask`

| File | Change |
|---|---|
| `xtask/src/arch/modules.rs` **(new)** | the module-edge checker (6.6) |
| `xtask/src/arch/modules_tests.rs` **(new)** | its unit tests |
| `xtask/src/arch/rules.rs` | `HUB_SRC`, `ZONES`, `UNZONED_MODULES`, `CALLER_*` data; `BOUNDARY_PAIRS` gains `"Caller::web"`, `"Caller::chat"`; two `BOUNDARY_CARDS` entries |
| `xtask/src/arch/mod.rs`, `scan.rs` | `mod modules;`, one call after `exec_violations` |
| `xtask/src/arch/ast_tests.rs` | tests for the two `CALLER-1` cards |

### 4.4 Web, docs, ADR

`web/src/api/openapi.json` and `web/src/api/schema.d.ts` **regenerated, never hand-edited**. Section 15 lists the
ADR, `ARCHITECTURE.md`, the book and the changelog items.

### 4.5 Tests

`crates/domovoi-hub/tests/hub/service_parity.rs` **(new)** + snapshots, `tests/hub/operation_controls.rs`
**(new)**, `tests/hub/main.rs` (register both), `tests/gates.rs` (extend), unit tests inside `src/services/`.
Existing tests change only where a constructor changed (`InteractionAuthorizer::new`, `ApiDeps`).

## 5. Explicit non-goals

| Not here | Owner / reason |
|---|---|
| Knowledge edits, policy publication, capability enable, package install services and their routes | 34/48, 40-45, 42/49: each adds its own service in `crate::services` |
| New chat behaviour: chat cancel, pause, resume or answer; any change to 59's chat step buttons beyond routing their calls through `OperationCommands` | no planned owner; this prompt only moves 59's calls into the services, with the same texts and order, and makes the services refuse what chat must not do |
| Moving HTTP-only handlers (3.2 list), `dismiss`, rules, notes, settings, enrollment | stay; the rule of the layer covers them when a second transport needs them |
| Moving prompt 27's and 28's read endpoints, `snapshot_seq`, SSE | stay in `api`; reads from repositories are not commands |
| Starting an operation from a route (`start_operation`); the Operator | 36 (durable Operator); `IncidentService::retriage` is the only start path today |
| A global pause switch, `PauseReason::GlobalPause`, `autonomy.resume`, step-up enforcement for operation controls | M8 / decided in section 2 |
| A new table, migration, config key or wire message | none; `PROTOCOL_VERSION` stays 1.2 (RW-42) |
| Changing any existing response, status code, error text, audit kind or card text | forbidden: the refactor is invisible |
| Policy evaluation, permits, unattended execution | 39, 40, 45 |
| Any UI file | 30 |
| `HumanApproval::new`/`Authorization::human` outside `crates/domovoi-hub/src/auth/` | INV-1 |

## 6. Required interfaces and contracts

Marks: **fixed** = names and semantics must match; **convention** = follow repository style. Every public item
gets a doc comment that says what it is for. Never copy a prompt number, "the build plan" or an RW/D id into
code, doc comments, commit messages, the book or the PR description. Where a type name below comes from a
prompt that is not merged, use the merged name and list the difference in the report.

### 6.1 The caller and the service bundle **fixed** (names), **convention** (layout)

```rust
// crates/domovoi-hub/src/services/caller.rs
/// Who asked: an authenticated human and the surface they used. Only a transport builds one,
/// after its own authentication; a service never builds one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller { user: UserId, surface: Surface }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Surface {
    Web { session_hash: [u8; 32] },                          // the session's stored one-way hash, never the cookie
    Chat { platform: String, platform_user: String },
}

impl Caller {
    pub(crate) fn web(user: UserId, session_hash: [u8; 32]) -> Self;                   // CALLER-1: one call site
    pub(crate) fn chat(user: UserId, platform: &str, platform_user: &str) -> Self;     // CALLER-1: one call site
    pub fn user(&self) -> UserId;
    pub fn surface(&self) -> &Surface;
    pub fn is_chat(&self) -> bool;
    pub fn actor(&self) -> Actor;                            // Actor::User(self.user)
}
```

The two call sites (**fixed**; the cards of 6.6 count them): `Authed::caller(&self) -> Caller` in
`crates/domovoi-hub/src/api/mod.rs` calls `Caller::web(self.0.user.id, self.0.session_hash)`; `Actor::caller(&self)
-> Caller` in `crates/domovoi-hub/src/auth/interactions/mod.rs` (the existing private `Actor<'a>` struct, `mod.rs:82-86`)
calls `Caller::chat(self.admin.id, self.platform, self.platform_user)`. A moved handler writes `auth: Authed`
instead of destructuring it, calls `auth.caller()` for the service and reads anything else it needs from `auth.0`
(the `Current`). No other code may build a `Caller`.

```rust
// crates/domovoi-hub/src/services/mod.rs
#[derive(Clone)]
pub struct Services {
    pub incidents: IncidentService,
    pub approvals: ApprovalService,
    pub operations: OperationCommands,
    pub hosts: HostService,
    pub briefing: BriefingService,
}
pub struct ServicesDeps {        // every field is a value the runtime already holds
    pub manager: ManagerHandle,
    pub triage: Option<TriageHandle>,
    pub actions: Arc<dyn ActionRepo>,
    pub actions_service: ActionHandle,
    pub operations: Option<OperationHandle>,        // None unless [orchestration] enabled
    pub operation_repo: Arc<dyn OperationRepo>,
    pub incidents: Arc<dyn IncidentRepo>,
    pub hosts: Arc<dyn HostRepo>,
    pub facts: Arc<dyn FactRepo>,
    pub advisories: Arc<dyn AdvisoryRepo>,
    pub agents: Arc<dyn AgentAdmin>,
    pub profiles: Option<ProfileDeps>,
    pub config: Arc<HubConfig>,
    pub clock: Arc<dyn Clock>,
    pub redactor: Arc<dyn Redactor>,
}
impl Services { pub fn new(deps: ServicesDeps) -> Self; }
```
Rules for every service (**fixed**): it is `Clone + Send + Sync + 'static` (a transport may move a clone into a
`tokio::spawn`); **it never spawns a task** (the transport that wants a detached task wraps the call); it never
imports `crate::api`, `crate::auth::interactions`, `crate::ingest`, `crate::runtime` or `crate::router`; it
returns domain or engine types and its own small error enums, never `ApiError`, never a DTO; every method that
acts for a person takes `&Caller` first; it never builds a `HumanApproval`, `Authorization` or `Caller`.

### 6.2 Service signatures **fixed** (names, parameters, return types); bodies are the old handler code moved

```rust
// services/incidents.rs
impl IncidentService {
    pub async fn ack(&self, caller: &Caller, incident: IncidentId) -> Result<(), CommandError>;       // manager.ack(incident, caller.user())
    pub async fn mute(&self, caller: &Caller, incident: IncidentId, until: Timestamp) -> Result<(), CommandError>;
    pub async fn unmute(&self, caller: &Caller, incident: IncidentId) -> Result<(), CommandError>;    // manager.unmute; logs the user at debug
    pub fn triage_enabled(&self) -> bool;
    pub async fn follow_up(&self, caller: &Caller, incident: IncidentId, question: &str) -> Result<String, FollowUpError>;
    pub async fn retriage(&self, caller: &Caller, incident: &Incident, fresh: bool) -> Result<Retriaged, RetriageError>;
}
pub enum FollowUpError { TriageOff, Triage(TriageError) }                 // TriageError, CommandError: existing engine types
pub enum Retriaged { Outcome(TriageOutcome), StillRunning }               // StillRunning: prompt 27's TimedOut
pub enum RetriageError { TriageOff, AlreadyRunning, Failed(String) }      // Failed: text for ApiError::internal
pub fn latest_triage(timeline: &[IncidentTimelineEntry]) -> Option<TriageOutcome>;   // moved unchanged
```
`CommandError`, `TriageError` and `ApprovalError` are engine types **(existing)**, already transport-neutral:
the services return them as they are and each transport keeps its own mapping (`command()` and the `ask` mapping in
`api/incidents.rs`, `approval_error` in `api/actions.rs`, `approval_reason` in `buttons.rs`).
`retriage` is the old `retry_triage` body from "run triage" on, **exactly as merged after prompt 27**: with
`operations` present it calls `investigate_and_wait(incident, fresh)` and maps `AlreadyRunning` and `TimedOut`;
without it, it calls `triage.retriage(incident)` when `fresh` and `triage.triage(incident)` otherwise (if prompt
58 removed that branch, there is no such branch). The handler order stays: `triage_enabled()` (else
`ApiError::TriageOff`) → parse and load the incident (404) → `tokio::spawn` around `services.incidents.retriage(..)`.

```rust
// services/approvals.rs   (legacy actions, both surfaces)
pub struct ApproveInput<'a> { pub binding: PlanBinding, pub chat_token: Option<&'a str>, pub confirm_text: Option<&'a str> }
pub enum ApproveError { WebOnly, Approval(ApprovalError) }               // WebOnly: chat tried a destructive action
impl ApprovalService {
    pub async fn find(&self, id: ActionId) -> Result<ActionRequest, ApprovalError>;          // NotFound(id) when absent
    pub async fn approve(&self, caller: &Caller, action: &ActionRequest, input: ApproveInput<'_>)
        -> Result<ActionRequest, ApproveError>;
    pub async fn deny(&self, caller: &Caller, action: &ActionRequest) -> Result<ActionRequest, ApprovalError>;
}
```
`approve`: (1) `caller.is_chat() && action.risk() == Risk::Destructive` → `WebOnly` (the chat button handler also
checks first and answers `WEB_ONLY`, so its text and order do not change; this is defence in depth); (2)
`authorization_for(caller, input.confirm_text, action.skill(), clock.now())`; (3) `ActionHandle::approve(action.id(),
authorization, input.chat_token, input.binding, provenance)`. `deny`: `approval_for(..)` then `ActionHandle::deny`.
The web handler keeps its order: `find` (404) → parse `plan_hash` (400) → `approve`. The chat handler keeps
`found.incident() == incident`, the destructive check, the prefix binding (`STALE_CARD`) in its own code.

```rust
// auth/approval.rs  (additions; the only place that turns a Caller into an approval)
pub fn authorization_for(caller: &Caller, confirm_text: Option<&str>, skill: &SkillName, now: Timestamp)
    -> (Authorization, ApprovalProvenance);     // Web -> web_authorization(user, confirm_text, skill, session_hash, now); Chat -> chat_authorization(user, platform, platform_user, now)
pub fn approval_for(caller: &Caller, confirm_text: Option<&str>, skill: &SkillName, now: Timestamp) -> HumanApproval;   // web_approval / chat_approval
```

```rust
// services/hosts.rs and services/briefing.rs
pub struct HostStatus {                      // the fields of today's HostDto, as plain values
    pub id: HostId, pub name: HostName, pub kind: HostKind, pub connected: bool, pub agent_enrolled: bool,
    pub version: Option<String>, pub connected_since: Option<Timestamp>, pub last_seen: Option<Timestamp>,
    pub outdated: bool, pub protocol_state: &'static str, pub reported_protocol: Option<String>,
    pub reported_release: Option<String>, pub incompatible_since: Option<Timestamp>, pub hub_protocol: String,
    pub open_incidents: u32,
}
impl HostService { pub async fn status(&self, host: &Host) -> Result<HostStatus, StoreError>; }   // body = host_dto's
pub const OPEN_COUNT_CAP: u16 = 200;                                                              // moved from api/hosts.rs

pub struct Briefing { pub date: String, pub markdown: String, pub next_send_at: Option<Timestamp>, pub hosts: Vec<BriefingHost> }
pub struct BriefingHost { pub host: HostName, pub quiet: bool, pub lines: Vec<String> }
pub enum BriefingError { Digest(DigestError) }                                                    // crate::digest::DigestError
impl BriefingService {
    pub async fn digest(&self) -> Result<Briefing, BriefingError>;                                // body = system::build's
    pub async fn profile_summary(&self, host: &Host) -> Result<String, ProfileError>;             // "" when no profile task or no profile
}
```
`api::hosts::host_dto(state, host)` becomes `state.services().hosts.status(host)` mapped field by field into
`HostDto` (timestamps with `to_string()` exactly as today; `kind` text via the existing `kind()` helper which stays
in `api/hosts.rs`). `api::system::build(state)` maps `Briefing` into `DigestPreview` (`next_send_at` via
`to_string()`, `DigestHostDto { host: host.to_string(), .. }`) and stays `pub(crate)` for the digest routes.
`api::profile::summary_of_host` is deleted (its only caller was chat). The chat `/status` and `/digest` code calls the
services and keeps its own formatting.

```rust
// services/operations.rs
pub struct BudgetExtension { pub kind: String, pub limit: u64 }            // kind = BudgetKind snake_case name
pub enum OperationCommandError {
    NotEnabled,                                  // no operation service in this hub
    Invalid(String),                             // bad input; the text is shown to the person
    TypedConfirmationRequired,                   // approve_step: destructive or unknown-risk skill without its exact name
    WebOnly,                                     // approve_step from chat for a destructive or unknown-risk skill
    Conflict(String),                            // the operation is in a state that cannot take the command
    NotFound,                                    // unknown operation or step
    Operation(OperationError),                   // whatever the writer answered
}
impl OperationCommands {
    pub async fn approve_step(&self, caller: &Caller, op: OperationId, step: StepId, binding: PlanBinding,
                              confirm_text: Option<&str>, chat_token: Option<&str>) -> Result<(), OperationCommandError>;
    pub async fn deny_step(&self, caller: &Caller, op: OperationId, step: StepId) -> Result<(), OperationCommandError>;
    pub async fn cancel(&self, caller: &Caller, op: OperationId, reason: Option<&str>) -> Result<(), OperationCommandError>;
    pub async fn pause(&self, caller: &Caller, op: OperationId) -> Result<(), OperationCommandError>;
    pub async fn resume(&self, caller: &Caller, op: OperationId, extend: &[BudgetExtension]) -> Result<(), OperationCommandError>;
    pub async fn answer(&self, caller: &Caller, op: OperationId, question: StepId, text: &str) -> Result<(), OperationCommandError>;
}
pub const CANCEL_REASON_MAX: usize = 500;  pub const ANSWER_MAX: usize = 2000;  pub const EXTEND_MAX_ENTRIES: usize = 7;
pub const EXTEND_MAX_COUNT: u64 = 10_000;                 // counted kinds: model_turns, evidence_requests, executions, cycles
pub const EXTEND_MAX_MEASURED: u64 = 10_000_000_000;      // measured kinds: wall_time (seconds), tokens, cost (micro-units)
pub const DEFAULT_CANCEL_REASON: &str = "cancelled by a person";
```
`approve_step` and `deny_step` are prompt 27's web handler logic **and prompt 59's chat button logic**, moved: load the snapshot
(`NotFound`), find the Execution step and its record (`NotFound` when the step is not an Execution step of that operation), take the
skill's risk from the same source the merged code uses today (the stored `ExecutionRecord::risk()` that 59 adds, or, if the merged
code still derives it, the live skill list through `agents.skills(host)` with the helper that builds `ExecutionDto.risk` moved here as
`risk_of_skill(&[SkillSummary], &SkillName) -> Option<Risk>` and 27's DTO builder importing it from `services`; add **no second
source** of risk), `confirm_required = !matches!(risk, Some(Risk::Read | Risk::Write))`; when required: a chat caller gets `WebOnly`,
a web caller whose `confirm_text != Some(skill name)` gets `TypedConfirmationRequired` (**before any write**); then
`authorization_for(..)` and `OperationHandle::authorize(op, step, authorization, binding, provenance, chat_token)` (the last argument is
59's single-use chat token: `None` for the web); `deny_step`: `approval_for(..)` then `OperationHandle::deny`. `binding` is whatever
the caller passes: the web handler passes `PlanBinding::Full(hash)`, a chat button passes the prefix binding it builds today. The
signatures above therefore carry one more parameter, `chat_token: Option<&str>`, on `approve_step`, and one more member:

```rust
/// What a chat button needs to know about the step behind an action id (59's `step_for` lookup; verify the merged shape).
pub struct StepRef { pub operation: OperationId, pub step: StepId, pub incidents: Vec<IncidentId>, pub skill: String, pub risk: Option<Risk> }
impl OperationCommands {
    /// The step whose execution id equals `action` (`ExecutionId::from_action`), or `None` for a legacy action.
    pub async fn find_by_action(&self, action: ActionId) -> Result<Option<StepRef>, OperationCommandError>;
}
```
The chat handler keeps, in its own code and in this order, everything 59 put there that is a chat decision: `incident` must be among
`StepRef::incidents` (else `"That action no longer exists"`), the destructive or unknown-risk refusal `WEB_ONLY`, the prefix binding
(`STALE_CARD`), and the texts of `step_reason(&OperationError)`. It maps `OperationCommandError` as: `Operation(e)` → `step_reason(&e)`;
`WebOnly` and `TypedConfirmationRequired` → `WEB_ONLY`; `NotFound` → "That action no longer exists"; `NotEnabled` → "Operations are not
running; open the incident in the web UI"; `Conflict(_)` and `Invalid(_)` → "Something went wrong; try again". **59's gate**
`step_authorization_has_exactly_two_production_call_sites` (`.authorize(` on an `OperationHandle` only in `api/operations.rs` and
`auth/interactions/buttons.rs`) is **tightened, not weakened**, by this prompt: it becomes `step_authorization_has_exactly_one_production_call_site`,
with the only allowed site `crates/domovoi-hub/src/services/operations.rs`.

### 6.3 Transport rewiring **fixed** (what is removed), **convention** (the rest)

- `ApiDeps`: add `pub services: Services`; **remove** `actions_service` and (prompt 27's) `operations`. Keep
  `manager`, `triage`, `incidents`, `actions`, `hosts`, `operation_repo` and the other read ports: the HTTP-only
  handlers and every read endpoint still use them. `ApiState::services(&self) -> &Services`. After this change no file under
  `api/` or `auth/interactions/` names `ActionHandle` or `OperationHandle` (rule `MOD-4`, 6.6, and a gate test).
- `ChatDeps` gains the read ports chat needs once it has no `ApiState`: `incidents: Arc<dyn IncidentRepo>` (timeline
  for Details, the active list for `/incidents` and `/mute`), `hosts: Arc<dyn HostRepo>` (`/status`),
  `notifier: Option<Arc<dyn Notifier>>` (`/digest`), `clock: Arc<dyn Clock>`, `public_url: Url` (the type of
  `WebIdentity::public_url`, `auth/mod.rs:77`; `incident_url`). `InteractionAuthorizer { services: Services, chat: ChatDeps }`
  and `InteractionAuthorizer::new(services: &Services, chat: ChatDeps)`; `InteractionAuthorizer::api` and every use of
  `self.api` disappear. Behaviour, texts and audit rows of chat stay as they are.
- `runtime/mod.rs` builds `Services::new(ServicesDeps { .. })` immediately before `ApiState::new` (all inputs exist
  there: `manager`, `triage.0`, `actions_service`, the operation handle if prompt 26/27 built one, `store`, `server`,
  `profile_deps`, `config`, `clock`, `redactor`), passes it into `ApiDeps.services`, and passes `&services` plus the chat
  read ports (`store`, `clock`, `FanOut::over(&notifiers)`, `public_url`) to `adapters::gateway_sinks`
  **(existing, `runtime/adapters.rs:210-245`)**. `examples/dev_hub.rs` and `tests/hub/api_harness.rs` do the same.
- Handlers: each moved handler keeps its `#[utoipa::path]` annotation, its parsing and validation, and its error
  mapping function; only the call into the handle or the inline logic is replaced by the service call. `cargo xtask
  openapi` must produce the same document except for the four new routes.

### 6.4 The operation control routes **fixed**

All four sit behind `Authed`, pass the existing CSRF guard (an unsafe method without the header is 403), live in
`api/operations.rs`, carry `tag = "operations"`, and answer through `OperationControlDto`.

```
POST /api/v1/operations/{id}/cancel                  operation_id cancel_operation           body CancelOperationRequest
POST /api/v1/operations/{id}/pause                   operation_id pause_operation            no body
POST /api/v1/operations/{id}/resume                  operation_id resume_operation           body ResumeOperationRequest
POST /api/v1/operations/{id}/steps/{step}/answer     operation_id answer_operation_question  body AnswerStepRequest   ({step} = the Question step id)
```

```rust
// api/dto_operations.rs  (utoipa::ToSchema; follow 27's conventions for derives and `#[schema]` attributes)
pub struct CancelOperationRequest { #[serde(default)] pub reason: Option<String> }                // send {}; at most 500 characters
pub struct ResumeOperationRequest { #[serde(default)] pub extend: Vec<BudgetExtensionDto> }       // send {}; at most 7 entries
pub struct BudgetExtensionDto {
    pub kind: String,        // "model_turns" | "evidence_requests" | "executions" | "cycles" | "wall_time" | "tokens" | "cost"
    pub limit: u64,          // the new limit: seconds for wall_time, micro-units for cost, a count otherwise; 1..=cap (6.2)
}
pub struct AnswerStepRequest { pub text: String }                                                // 1 to 2000 characters
pub struct OperationControlDto {
    pub as_of_seq: u64,                  // journal head read AFTER the command and BEFORE the summary (28's rule)
    pub operation: OperationSummaryDto,  // 27's DTO: the refreshed operation
}
```

Wire examples (the first three answer 200 with the same shape; `waiting_for` is `[]` for a terminal or paused operation):

```json
POST /api/v1/operations/0190f2a8-.../cancel   {"reason": "wrong host"}
POST /api/v1/operations/0190f2a8-.../pause
POST /api/v1/operations/0190f2a8-.../resume   {"extend": [{"kind": "executions", "limit": 6}, {"kind": "wall_time", "limit": 7200}]}
POST /api/v1/operations/0190f2a8-.../steps/0190f2a9-.../answer   {"text": "That disk is the backup disk; ignore it."}
-> 200 {"as_of_seq": 1842,
        "operation": {"id": "0190f2a8-...", "kind": "investigate", "goal": "Investigate: disk full on nas",
                      "status": "paused", "pause_reason": {"type": "manual"}, "waiting_for": [], "cycle": 0,
                      "incidents": ["0190f2a7-..."], "hosts": ["0190f2a6-..."],
                      "created_at": "2026-11-18T09:00:00Z", "started_at": "2026-11-18T09:00:01Z",
                      "ended_at": null, "updated_at": "2026-11-18T09:05:12Z"}}
```

Handler shape (**convention**; every one of the four is this): parse `{id}` (and `{step}`) with 27's id parser (unparsable
or unknown → 404); call the one service method with `&auth.caller()`; map `OperationCommandError` with
`command_error`; then `let as_of_seq = snapshot_seq(&state).await?;` **and only then** load the operation and build
`OperationSummaryDto` exactly as 27's `get_operation` does; return `OperationControlDto`. With the service
absent every one of them answers 503 "orchestration is not enabled" (the same text as 27's step routes).

`command_error(OperationCommandError) -> ApiError` (`api/operation_errors.rs`, **fixed** texts):

| Variant | Status and text |
|---|---|
| `NotEnabled` | 503 `Unavailable("orchestration is not enabled")` |
| `Invalid(text)` | 400 `BadRequest(text)` |
| `TypedConfirmationRequired` | 403 `Forbidden("type the skill's name to approve a destructive action")` |
| `WebOnly` | 403 `Forbidden("Approve this one in the web UI")` (the chat button text; the web handlers never produce it) |
| `Conflict(text)` | 409 `Conflict(text)` |
| `NotFound` | 404 |
| `Operation(e)` | 27's `operation_error(e)` unchanged (`NotFound` 404; `Execution(PlanChanged)` 409 "the plan changed since you opened it; reload the page and review it again"; `Execution(Expired)` 409; `Execution(AlreadyAuthorized)` 409; `Execution(NotYetSupported)` 409; `Decide(e)` 409 with `e`'s plain text; `Conflict`/`StaleEffect` 409 "another change landed first; reload and try again"; `HostUnavailable` 409; `Store` 500 via `ApiError::internal`; `Unavailable` 503 "the operation service is restarting; try again") |

### 6.5 What each control does **fixed** (state checks and texts)

Common first step: `handle = self.handle.as_ref().ok_or(NotEnabled)?`. A precheck reads `OperationRepo::get` (status) or
`OperationRepo::load` (steps); the writer re-checks everything, so a lost race still ends in a 409 from the writer.
Prechecks exist to give accurate texts and idempotent repeats, not to decide.

```text
cancel(caller, op, reason):
  reason = reason.map(trim); None or empty -> DEFAULT_CANCEL_REASON
  chars(reason) > CANCEL_REASON_MAX      -> Invalid("the reason must have at most 500 characters")
  reason = redactor.redact(reason)
  status = repo.get(op)                  -> None: NotFound
  status == cancelled                    -> Ok(())            // a repeat changes nothing, writes nothing
  status terminal (succeeded|failed|escalated) -> Conflict("the operation has already finished")
  handle.cancel(op, caller.actor(), reason)    // writer: pending steps cancelled, dispatched execution stays running

pause(caller, op):
  status terminal -> Conflict("the operation has already finished")
  status queued   -> Conflict("the operation has not started yet; cancel it instead")
  status paused   -> Ok(())            // repeat
  handle.pause(op, caller.actor())     // writer records PauseReason::Manual

resume(caller, op, extend):
  extend.len() > EXTEND_MAX_ENTRIES -> Invalid("at most 7 limits can be raised at once")
  each entry: kind must parse as BudgetKind -> else Invalid("unknown limit `<kind>`"); kinds distinct -> else Invalid("each limit can be raised once");
              limit >= 1 and <= EXTEND_MAX_COUNT (counted kinds) or EXTEND_MAX_MEASURED (measured) -> else Invalid("the limit for `<kind>` must be between 1 and <cap>")
  status != paused -> Conflict("the operation is not paused")
  handle.resume(op, caller.actor(), extend.map(|e| (kind, Some(e.limit))))   // never None: the API cannot make a budget unlimited
  // a spent budget that is still spent surfaces as Operation(Decide(BudgetStillSpent(_))) -> 409 with the engine's text

answer(caller, op, question, text):
  text = trim(text); chars(text) not in 1..=ANSWER_MAX -> Invalid("the answer must have 1 to 2000 characters")
  snapshot = repo.load(op) -> None: NotFound;  step missing -> NotFound
  step.kind != Question      -> Conflict("that step is not a question")
  step.outcome != pending    -> Conflict("this question is not waiting for an answer")
  status terminal            -> Conflict("the operation has already finished")
  status paused              -> Conflict("the operation is paused; resume it before answering")
  text = cut(redactor.redact(text), ANSWER_MAX chars)       // idempotent redaction, cut at a char boundary
  handle.answer(op, question, text, caller.actor())
```
The Manual pause is a pause **with no budget condition**: resuming it needs no extension. The answer text is
data for the next model turn (26/27 decide how it reaches the model); it is never logged by this layer and is
never part of an audit row.

### 6.6 The module-edge rule **fixed** (rule ids, data, semantics)

New data in `xtask/src/arch/rules.rs` (**fixed** names):

```rust
/// The hub's source root, workspace-relative.
pub const HUB_SRC: &str = "crates/domovoi-hub/src/";
pub enum ZoneKind { Transport, Service }
pub struct Zone { pub name: &'static str, pub module: &'static str, pub kind: ZoneKind }   // module: path after `crate::`, `::` separated
pub const ZONES: &[Zone] = &[
    Zone { name: "http",     module: "api",                kind: ZoneKind::Transport },
    Zone { name: "chat",     module: "auth::interactions", kind: ZoneKind::Transport },
    Zone { name: "webhook",  module: "ingest",             kind: ZoneKind::Transport },
    Zone { name: "services", module: "services",           kind: ZoneKind::Service },
];
/// Modules services may not import besides the transports: the composition root.
pub const SERVICE_FORBIDDEN_MODULES: &[&str] = &["runtime", "router"];
/// Command handles transports may not name: commands go through a service.
pub const TRANSPORT_FORBIDDEN_NAMES: &[&str] = &["ActionHandle", "OperationHandle"];
/// Top-level hub modules that are not a zone. A module not listed here and not a zone is MOD-3.
pub const UNZONED_MODULES: &[&str] = &[
    "admin", "agent_server", "auth", "csp", "digest", "middleware", "profile", "router", "runtime", "selftest",
    "settings", "spa", "version",
];
```

| Rule | Violation | Message (one line, ends with how to fix) |
|---|---|---|
| `MOD-1` | a file in transport zone A references a path in transport zone B (A ≠ B) | "transports do not import each other; both call a service in `crate::services`" |
| `MOD-2` | a file in the `services` zone references a transport zone or a module in `SERVICE_FORBIDDEN_MODULES` | "a service does not import a transport or the runtime; move the shared type out of the transport" |
| `MOD-3` | a hub source file whose top-level module is neither a zone's first segment nor in `UNZONED_MODULES` (`lib.rs` excepted) | "a new top-level hub module must be added to `ZONES` (a transport or the services) or `UNZONED_MODULES`, with a reason in the ADR" |
| `MOD-4` | a file in a transport zone (`api`, `auth::interactions`) names a `TRANSPORT_FORBIDDEN_NAMES` identifier (type or path segment, in code or a macro body) | "a transport does not hold a command handle; call the service that owns the command" |

`runtime`, `router`, `spa`, `middleware`, `selftest`, `admin`, `profile`, `settings`, `agent_server`, `auth` (outside `interactions`) and the
crate root are unconstrained: they compose or support. The rule covers production files only; `#[cfg(test)]` modules and
test-only files are skipped (the checker passes `production`, and the visitor skips `#[cfg(test)]` items with
`wiring::is_cfg_test`).

Algorithm (**convention** for the code; the observable behaviour is **fixed**), in the new file
`xtask/src/arch/modules.rs`, `pub fn module_edge_violations(files: &[(String, String)]) -> Vec<Violation>`,
exported with `pub mod modules;` in `arch/mod.rs` and called from `scan::check_tree` right after `exec_violations`:

1. Keep only files whose workspace-relative path starts with `HUB_SRC`. A file's module path is its path below
   `HUB_SRC` without `.rs`, with a trailing `mod` segment removed (`api/actions.rs` → `[api, actions]`,
   `auth/interactions/mod.rs` → `[auth, interactions]`, `lib.rs` → `[]`).
2. The zone of a module path is the `ZONES` entry with the longest `module` that is a prefix of it (`None` when none).
3. Parse with `syn::parse_file`; a file that does not parse is a `MOD-1` violation naming it (never a pass).
4. Collect every path the file mentions: each leaf of every `use` tree (full segments, groups and renames expanded),
   every `syn::Path` in expressions, types, patterns and attributes, and, inside macro bodies, every
   `crate :: seg :: seg` token run. Resolve each with `alias::aliases` (a local name introduced by a `use` expands to its
   full path), then: first segment `crate` → the rest; `self` → file module + rest; each leading `super` pops one
   segment of the file's module path; anything else (an external crate, a local item) is ignored.
5. Look up the zone of the resolved path and apply the table. Report `"<file>:<line>"` from the path's span (as
   `ast::at` does), one violation per offending path, sorted for stable output.
6. `MOD-4` also reports an identifier equal to a forbidden name wherever it appears in a transport-zone file.

Hub gate (extend `crates/domovoi-hub/tests/gates.rs`, never weaken): `hub_modules_respect_the_module_edge_rule` calls
`xtask::arch::modules::module_edge_violations(&hub_sources())` and asserts it is empty.

`CALLER-1` (in `rules.rs`; each with a unit test in `xtask/src/arch/ast_tests.rs`): `BOUNDARY_PAIRS` gains `"Caller::web"`
and `"Caller::chat"`; `BOUNDARY_CARDS` gains two cards, id `"CALLER-1"`, `BoundaryOp::Pair("Caller::web")` with the one site
`crates/domovoi-hub/src/api/mod.rs` count 1, and `BoundaryOp::Pair("Caller::chat")` with the one site
`crates/domovoi-hub/src/auth/interactions/mod.rs` count 1; message: "a Caller is built only by a transport after it
authenticated the person: the session extractor for the web, the allowlist check for chat".

An ADR `docs/adr/NNNN-use-case-layer.md` is **required** (`rules.rs` changes, `cargo xtask arch-check --base origin/main` fails
without it). It records: the layer and its three-line rule, the zones, the four rules and the two cards, what stays unconstrained
and why, the "commands through services, reads may stay" rule, the list of HTTP-only handlers left in place, the step-up decision
of section 2, and that chat approval of steps and the Operator plug into the same services.

### 6.7 The two permitted edits of prompt 26's code **fixed**

- `PauseReason::Manual` (core). Serde: `{"type": "manual"}`. Display text for a status line, if the type has one: "paused by a person".
  `decide`: `Pause` stores `Paused { reason: Manual }`; `Resume` from `Manual` goes to `Running` without a budget check.
  `OperationHandle::pause(op, actor)` sends `Pause { reason: Manual }`. The five frozen trace fixtures stay byte-identical
  (none contains a manual pause); add a **new** unit test, not a new fixture. `PauseReasonDto.type` is an open string, so the
  generated schema does not change shape.
- Audit kind `operation.answered` (constant in `operations/audit.rs`), written in the same transaction as the `Answer` event, actor
  `Actor::User(..)` of the command, subject `operation:<id>`, detail `{ "operation": <id>, "step": <question step id> }` and nothing
  else.
- If the merged prompt 26 already has either item (under any name), use it, delete the matching edit from your plan and say so in
  the report. Do not add a third edit to 26's code.

## 7. Ordered implementation steps

1. **Inspect and verify the repository context; report discrepancies before editing.**
   - Open every file and symbol of sections 3 and 4. Record moved lines, renamed types, changed signatures.
   - Confirm 26, 28 and 27 are merged (`OperationHandle`, `OperationRepo`, `snapshot_seq`, `api/operations.rs`, `ApiDeps.operations`,
     `ApiDeps.journal`). **If any is missing, stop and report.** Use the merged names; list every difference.
   - From the merged 26: how `OperationHandle::pause` picks a `PauseReason` and whether a person-initiated variant exists;
     whether an audit kind for an answer exists; the exact return types of `cancel`, `pause`, `resume`, `answer`; the signatures of
     `OperationRepo::get` and `load`; `BudgetKind`'s serde names and whether it has `FromStr`/`parse`.
   - From the merged 27: where `operation_error`, the `ExecutionDto` builder and the risk lookup live; the harness function that starts a
     hub with orchestration on (`tests/hub/operations_api.rs`, `api_harness.rs`); whether `retry_triage` still has the legacy branch (58).
   - `grep -rn "ActionHandle\|OperationHandle" crates/domovoi-hub/src` and `grep -rn "ApiDeps {\|InteractionAuthorizer::new\|ChatDeps {"
     crates` and keep both lists for steps 5 to 7. `grep -rn "OperationHandle" crates/domovoi-hub/src/auth`: the hits are 59's chat step
     buttons (`step_for`, `step_reason`, the step branches of `approve` and `deny` in `buttons.rs`); step 6 moves their handle calls onto
     `OperationCommands::{approve_step, deny_step, find_by_action}` with `Caller::chat`, keeping the prefix `PlanBinding` and every chat
     text. **No hit means 59 is not merged: stop and report.** Also read 59's tests and its call-site gate (`tests/hub/`).
   - Run `cargo xtask arch-check` and `cargo nextest run -p domovoi-hub` once so you know the baseline is green. `ls docs/adr` for the next
     free ADR number. Confirm this prompt adds **no** migration.
2. Create branch `use-case-layer` from the latest `overhaul-investigation`.
3. **Pin today's behaviour first (tests only).** Write `crates/domovoi-hub/tests/hub/service_parity.rs` (12.1) against the **unrefactored** tree, register it
   in `tests/hub/main.rs`, generate the `insta` snapshots, **read every snapshot**, commit. Commit `test(hub): pin the responses of the commands that move into services`.
   From here to step 6 these snapshots and `hub__api_routes__openapi.snap` must not change by a single byte.
4. **Neutral homes (no behaviour).** Move `api/version.rs` to `src/version.rs`; move `AgentAdmin` and its impl to `src/agent_server/admin.rs` and re-export it from `api`;
   point `profile/mod.rs` at it; move `OPEN_COUNT_CAP` later with `HostService`. `cargo nextest run -p domovoi-hub`. Commit `refactor(hub): move shared helpers out of the http module`.
5. **Read services and the chat decoupling.** Create `services/{mod,caller,hosts,briefing,incidents}.rs` with `HostService`, `BriefingService` and `latest_triage` only, plus
   `Services::new`. Add `ApiDeps.services` (keep every other field for now), `ApiState::services()`, map `host_dto` and `system::build` through the services, delete `summary_of_host`. Replace
   `InteractionAuthorizer { api }` by `{ services, chat }` with the new `ChatDeps` fields, switch `commands.rs` and `buttons.rs` (Details) to the services, update `runtime/adapters.rs`,
   `runtime/mod.rs`, `examples/dev_hub.rs`, `tests/hub/api_harness.rs`, `tests/hub/interactions.rs` (constructor only). **After this step `auth/interactions` has no `crate::api` import.** Run the whole
   hub suite; the step-3 snapshots must be unchanged. Commit `refactor(hub): chat reads through services instead of the http module`.
6. **Command services.** Add `Caller` constructors and the two call sites, `authorization_for`/`approval_for` in `auth/approval.rs`, `IncidentService` commands, `ApprovalService`. Move
   the handler logic of 3.2 (ack, mute, unmute, ask, retry_triage, approve, deny and the chat Ack, Mute1h, Approve, Deny, thread follow-up, `/mute`). Also move 27's `approve_step` and `deny_step` web handlers and 59's chat step path into
   `OperationCommands::{approve_step, deny_step, find_by_action}` (the control methods come in step 8), move `operation_error` to `api/operation_errors.rs`, and tighten
   59's call-site gate to the single site `services/operations.rs`. Remove `actions_service` and `operations` from `ApiDeps`. Each moved handler keeps its annotation, validation, spawn
   and error mapping. Hub suite and step-3 snapshots unchanged. Commit `refactor(hub): commands go through services for both transports`.
7. **The rule.** `xtask/src/arch/modules.rs` and `modules_tests.rs`, the data in `rules.rs`, the two `CALLER-1` cards and their tests, the call from `scan::check_tree`, the hub gate, the ADR, the `AGENTS.md` hard rule (section 15). Fix any
   violation the new rule reports by moving code, never by editing the rule data. `cargo xtask arch-check` and `cargo xtask arch-check --base origin/main`. Commit `feat(xtask): module-edge rules for the hub`.
8. **Operation controls.** The two edits to prompt 26's code (6.7, skipping what already exists), `OperationCommands::{cancel, pause, resume, answer}`, DTOs, `command_error`, the four handlers and routes, `tests/hub/operation_controls.rs` (12.2).
   Commit `feat(core): a pause a person asks for` (the `PauseReason::Manual` edit and its test, engine writer edit, audit kind) then `feat(hub): cancel, pause, resume and answer routes for operations`.
9. `cargo xtask openapi && cd web && npm run gen:api`. Review the diff of `web/src/api/openapi.json` and `schema.d.ts`: **only additions** (four paths, five schemas). `cargo insta review` for `hub__api_routes__openapi.snap`: accept only that additive change. `cd web && npm run lint && npm run typecheck`.
10. **Docs** (section 15), `cargo xtask docs-links`.
11. **Checks and the completion report.** `cargo fmt --all`; `cargo clippy -p domovoi-hub -p domovoi-core -p domovoi-engine -p xtask --all-targets -- -D warnings`; `cargo nextest run -p domovoi-hub -p domovoi-core -p domovoi-engine -p xtask`;
    `cargo nextest run -E 'rdeps(domovoi-core)'` (a public enum gained a variant); `cargo xtask arch-check`; `cargo xtask arch-check --base origin/main`; `git diff --stat <pin commit> -- crates/domovoi-hub/tests/hub/snapshots` shows only the OpenAPI snapshot.

Commit in small green steps with one-line Conventional Commits and exactly one scope each.

## 8. Database and persistence

**No migration and no schema change** (`ls crates/domovoi-store/migrations/` is identical before and after). Every write goes through prompt 26's operation writer (one `UnitOfWork` commit per command: state, journal, audit and effect changes together) or the existing action and
incident writers; the services add no write path and no SQL. A pause reason is stored in the existing `operations.pause_reason` JSON column; `{"type":"manual"}` needs no DDL. **N-1 upgrade test: not applicable** (no schema change); the existing upgrade tests must pass.

## 9. Protocol and compatibility

No wire change (`PROTOCOL_VERSION` stays 1.2). HTTP: four new routes and five new schemas, purely additive; no existing field, path, status code or text changes, so the generated `schema.d.ts` changes only by addition. Rust API of the hub library (public, but consumed only by the
`domovoi` binary and the tests): `ApiDeps` loses `actions_service` and `operations` and gains `services`; `InteractionAuthorizer::new` and `ChatDeps` change as in 6.3. `domovoi_hub::api::AgentAdmin` keeps resolving through a re-export.

## 10. Error handling and recovery

| Situation | Exact behaviour |
|---|---|
| Orchestration is off (no `OperationHandle`) | all four control routes: 503 "orchestration is not enabled"; nothing written |
| `{id}` or `{step}` is not a UUID, or the operation or step does not exist | 404 |
| `cancel` on `cancelled` | 200 with the current summary; no write, no audit row (a repeat is safe) |
| `cancel` on `succeeded`, `failed` or `escalated` | 409 "the operation has already finished" |
| `cancel` while an Execution is `Running` | 200; pending steps and effects are cancelled, the run is **not** recalled; its result is still recorded later (26) |
| `pause` on `paused` | 200, no write |
| `pause` on `queued` | 409 "the operation has not started yet; cancel it instead" |
| `resume` on a status other than `paused` | 409 "the operation is not paused" |
| `resume` of a budget pause with no or too small an extension | 409 with the engine's text (`BudgetStillSpent`); the operation stays paused |
| `resume` of an `outcome_unknown` pause | allowed; no budget condition; the person has decided to continue (26 decides what the writer then does) |
| `resume` with an unknown kind, a duplicate kind, more than 7 entries, a limit of 0 or above the cap | 400 with the texts of 6.5; nothing written |
| `answer` to a question that is not `pending` (answered, timed out, cancelled) | 409 "this question is not waiting for an answer" |
| `answer` to a step that is not a question | 409 "that step is not a question" |
| `answer` while the operation is paused or finished | 409 "the operation is paused; resume it before answering" / "the operation has already finished" |
| Text or reason empty or too long | 400 with the texts of 6.5 |
| The writer conflicts twice | 409 "another change landed first; reload and try again" (27's mapping of `Conflict`) |
| The operation service is shutting down | 503 "the operation service is restarting; try again" |
| Store error during a precheck | 500 via `ApiError::internal`; nothing written |
| A precheck passes and the state changes before the writer runs | the writer's `DecideError` is mapped to 409 with its plain text; the state is whatever the writer left |
| A chat caller asks `approve_step` for a destructive or unknown-risk skill | `WebOnly`, nothing written (no chat route reaches this today) |
| Any moved handler (ack, mute, ask, approve, deny, retry, chat buttons and commands) | the same status, text, audit row and card text as before the move; proven by the step-3 snapshots |

## 11. Security requirements

- **Human-only (INV-1, ADR 0007, 0021).** Every control route needs `Authed` and, being a `POST`, the CSRF header. `HumanApproval::new` and `Authorization::human` stay inside `crates/domovoi-hub/src/auth/` (the existing
  cards pass unchanged); the services reach them only through `authorization_for`/`approval_for`. No model tool, chat command, scheduler or effect handler calls a control or a service method for a person. The `ReadTool` catalogue is untouched.
- **`Caller` is the proof of authentication.** `Caller::web` is called once, in `Authed::caller`, after the session lookup; `Caller::chat` once, in the interaction authorizer after the allowlist and admin lookup. `CALLER-1` fails arch-check on a second call site.
  A service never builds or widens a `Caller`.
- **No new authority for chat.** Chat gets no new command. The services refuse a chat caller where the web would need typed confirmation (INV-6), with the same texts.
- **Audit.** `operation.cancelled`, `operation.paused`, `operation.resumed` and `operation.answered` rows are written by the writer in the same transaction as the change; the actor is `user:<id>` of the person; no row holds the answer text, the cancel
  reason is not copied into an audit detail, a secret or a plan hash in full. This layer writes no audit row of its own.
- **Input is bounded and redacted.** Cancel reason at most 500 characters, answer 1 to 2000, extension at most 7 entries with the caps of 6.2; both texts pass the `Redactor` before storage; the API can never make a budget unlimited (`None`).
- **Fail closed.** A missing or unparsable id is 404; a missing handle is 503; an unreadable state is a 500 or a 409, never a guessed success. A repeat of `cancel` or `pause` is the only silent success, and only when the stored status already is the requested one.
- **Model output stays data (INV-9).** The answer is stored as a step and later shown to the model as data; nothing in this layer interprets it.
- **Secrets.** No secret is read or stored. The session's stored hash goes only into the provenance of an approval, as today, never into a log line.
- **No step-up** for the four controls (section 2); a test pins the decision.
- **Layer rules are the guard.** `MOD-1` to `MOD-4` stop a second transport from reaching the first one's helpers and stop a transport from holding a command handle.

## 12. Testing requirements

Names describe behaviour. Fakes that exist: `InMemoryStore`, `FakeClock`, `FakeAgentLink`, `ListRedactor`, `RecordingGateway` (hub tests), `ScriptedLlm` (`crates/domovoi-testkit/src/`) and the hub harness (`tests/hub/api_harness.rs`, `harness.rs`, `interactions.rs`);
prompt 26's and 27's harnesses for operations. Tests never sleep: advance `FakeClock`.

### 12.1 Pinned behaviour (`crates/domovoi-hub/tests/hub/service_parity.rs`, written **before** the refactor, snapshots under `tests/hub/snapshots/`)

A helper `normalise(&Value) -> Value` replaces each distinct UUID string by `<id-1>`, `<id-2>`, ... in order of first appearance so snapshots are stable; time comes from the fake clock (`t0()`).

| Test | Setup, action, snapshot |
|---|---|
| `incident_commands_answer_as_before` | `api()`: ack, ack of an unknown id, mute 60, mute 0, mute 10081, unmute when not muted, unmute after a mute, ask empty, ask of 1001 characters, ask with triage off: one snapshot of `(status, body)` per call, in order |
| `retry_triage_answers_as_before` | `api_with_triage()` and `api()`: `POST /incidents/{id}/triage` with and without `fresh`: status and body (`TriageStateDto`, or 503) |
| `action_approval_answers_as_before` | a write action and a destructive action from `a.propose(..)`: approve unknown id, approve without hash, with a wrong hash, with the right hash, deny, deny again, destructive without and with `confirm_text`; status and body, and the audit kinds and actors that exist afterwards |
| `host_and_digest_reads_answer_as_before` | `GET /hosts`, `GET /hosts/{name}`, `GET /digest/preview` with a connected scripted agent: normalised JSON |
| `chat_answers_as_before` | the chat harness of `interactions.rs`: buttons Ack, Mute1h, Approve (valid token and prefix), Deny, Details; commands `/status`, `/status nas`, `/incidents`, `/mute`, `/digest` (with a recording notifier); a thread message; a non-allowlisted user. Snapshot: every `InteractionReply` and the list of `(audit kind, subject, actor)` |
| `chat_caller_cannot_approve_a_destructive_action_through_the_service` | (written with step 6) a chat `Caller` and a destructive action: `ApprovalService::approve` returns `ApproveError::WebOnly`, the action stays `Proposed`, no audit row |

The first five must pass **unchanged** against the final code. `openapi_snapshot` (`api_routes.rs:529`) stays byte-identical until step 8.

### 12.2 Operation controls (`tests/hub/operation_controls.rs`; the operation harness of 27 with orchestration on, `FakeClock`, `ListRedactor`)

Setup helper: create an Investigate-style operation with `OperationHandle::create(OperationSeed { .. })` with one pinned subject and a `Question` first step (`expires_at` far ahead), admitted `Running`; log in with `a.admin()`.

| Test | Action, expected |
|---|---|
| `cancel_route_cancels_and_records_the_person` | `POST .../cancel {"reason":"wrong host"}` → 200, `operation.status == "cancelled"`, the question step `cancelled`, pending effects `cancelled`, audit `operation.cancelled` with actor `user:<admin id>`, `as_of_seq` at least the journal head after the command |
| `cancel_twice_writes_once` | second call 200, exactly one `operation.cancelled` audit row |
| `cancel_of_a_finished_operation_is_a_conflict` | a `Succeeded` operation → 409 "the operation has already finished", nothing written |
| `cancel_reason_is_bounded_and_redacted` | 501 characters → 400; a reason holding a known secret is stored redacted (check the journal payload and step reason) |
| `pause_route_pauses_with_a_manual_reason` | 200, `status == "paused"`, `pause_reason.type == "manual"`, audit `operation.paused` |
| `pause_twice_is_idempotent_and_pause_of_queued_or_finished_is_refused` | repeat 200 no write; `queued` (second operation with `max_running = 1`) 409 "has not started yet"; finished 409 |
| `resume_route_resumes_a_manual_pause_without_an_extension` | 200 `running`, audit `operation.resumed` |
| `resume_of_a_spent_budget_needs_an_extension` | executions limit 1 spent, operation `paused{budget}`: `{}` → 409 with the engine text; `{"extend":[{"kind":"executions","limit":3}]}` → 200 `running` |
| `resume_validates_the_extension` | table: unknown kind, duplicate kind, 8 entries, limit 0, counted limit 10001, measured limit 10000000001 → 400 with the exact texts of 6.5, nothing written |
| `resume_of_a_running_operation_is_a_conflict` | 409 "the operation is not paused" |
| `answer_route_finishes_the_question_with_a_note` | 200; the question `succeeded`; a `note` step with an `answers` edge in `GET .../trace`; audit `operation.answered` whose detail has `operation` and `step` and **no text**; the operation is no longer `waiting_user` |
| `answer_text_is_trimmed_bounded_and_redacted` | `""`, whitespace only and 2001 characters → 400; a text with a known secret is stored redacted; surrounding whitespace is trimmed |
| `answer_refusals` | unknown step 404; a non-question step 409 "that step is not a question"; an answered question 409 "this question is not waiting for an answer"; paused 409; finished 409 |
| `control_routes_need_a_session_and_the_csrf_header` | the four routes: 401 without a cookie, 403 with a cookie and no `X-CSRF-Token` |
| `control_routes_answer_503_when_orchestration_is_off` | `api()` without operations: the four routes → 503 "orchestration is not enabled" |
| `control_responses_read_the_journal_head_after_the_command_and_before_the_summary` | `as_of_seq` is at least the `seq` of the last journal row the command wrote and the summary in the same body shows the new status; a journal row appended by a store hook between the two reads never makes `as_of_seq` exceed the head the summary saw |
| `openapi_lists_the_four_control_routes` | the generated document has the four paths with operation ids `cancel_operation`, `pause_operation`, `resume_operation`, `answer_operation_question`, tag `operations`, the request bodies, and `OperationControlDto` with responses 200, 400, 401, 404, 409, 503 |
| `step_approval_through_the_service_matches_the_old_behaviour` | the existing tests of `operations_api.rs` for approve and deny (hash, typed confirmation, 409 texts) and the chat step tests of 59 in `interactions.rs` (valid token authorizes once, reused token refused, destructive refused, prefix refused, deny) pass **unmodified** |
| `step_authorization_has_exactly_one_production_call_site` | the tightened gate (6.2): a source scan of `crates/domovoi-hub/src` finds `.authorize(` on an `OperationHandle` only in `services/operations.rs`, and `.deny(` on one only there too |
| `find_by_action_returns_none_for_a_legacy_action_and_the_step_for_an_execution` | a legacy `ActionId` → `None`; the `ActionId` of an execution step → `StepRef` with its operation, incidents, skill and risk |

Unit tests next to the code (`src/services/`): `caller_actor_is_the_user`; `operation_controls_are_not_step_up_actions` (no id in `STEP_UP_ACTIONS` starts with `operation.`);
`risk_of_skill_treats_a_missing_skill_as_unknown`; `extension_limits_are_checked_per_kind`; `approve_step_asks_for_the_typed_name_before_any_write` (a recording handle is not needed: assert the error variant);
`a_chat_caller_cannot_approve_a_step_that_needs_the_typed_name` (variant `WebOnly`).
Core: `pause_reason_manual_round_trips_as_json`. Engine (next to 26's writer tests): `manual_pause_resumes_without_a_budget_check`, `answer_writes_an_audit_row_without_the_text` (skip the matching one if 26 already has it).

### 12.3 The rule (`xtask/src/arch/modules_tests.rs`, `ast_tests.rs`, `crates/domovoi-hub/tests/gates.rs`)

Fixtures are `(path, text)` pairs under `crates/domovoi-hub/src/`.

| Test | Expected |
|---|---|
| `chat_importing_http_is_refused` | `auth/interactions/mod.rs` with `use crate::api::ApiState;` → `MOD-1` naming file and line |
| `http_importing_chat_is_refused` | `api/x.rs` with `use crate::auth::interactions::Actor;` → `MOD-1` |
| `webhook_importing_http_is_refused` | `ingest/x.rs` with `crate::api::error::ErrorBody` → `MOD-1` |
| `a_super_path_is_resolved` | `auth/interactions/buttons.rs` with `use super::super::super::api::X;` → `MOD-1`; `use super::Accepted;` → clean |
| `a_renamed_import_is_resolved` | `use crate::api as web;` then `web::hosts::host_dto(..)` → `MOD-1` |
| `a_fully_qualified_expression_path_is_counted` | `crate::api::incidents::latest_triage(&t)` with no `use` → `MOD-1` |
| `a_macro_body_cannot_hide_an_edge` | `format!("{}", crate::api::version::HUB_VERSION)` → `MOD-1` |
| `services_may_not_import_a_transport_or_the_runtime` | `services/x.rs` with `crate::api::dto::HostDto`, `crate::auth::interactions::Accepted`, `crate::ingest::IngestState`, `crate::runtime::support` → `MOD-2` each |
| `services_may_import_auth_approval_agent_server_profile_and_digest` | clean |
| `transports_may_import_services` | `api/x.rs` and `auth/interactions/x.rs` with `use crate::services::Services;` → clean |
| `runtime_router_and_unzoned_modules_may_import_every_transport` | clean |
| `a_new_top_level_module_must_be_declared` | `crates/domovoi-hub/src/operator/mod.rs` → `MOD-3` |
| `a_transport_naming_a_command_handle_is_refused` | `api/x.rs` with `ActionHandle` in a type, a path and a macro body, and `auth/interactions/x.rs` with `OperationHandle` → `MOD-4`; the same names in `services/` and `runtime/` → clean |
| `a_cfg_test_module_is_not_checked` | an `#[cfg(test)] mod` with a forbidden import → clean |
| `files_outside_the_hub_are_ignored` | `crates/domovoi-engine/src/x.rs` with `crate::api` → clean |
| `an_unparsable_file_is_a_violation` | not a pass |
| `a_second_caller_web_call_is_refused` and `a_caller_chat_call_in_another_file_is_refused` | `CALLER-1` in `ast_tests.rs`; the exact counts of the card sites pass |
| `hub_modules_respect_the_module_edge_rule` | gate: `module_edge_violations(&hub_sources())` is empty on the real tree |

## 13. Acceptance criteria

1. `crate::services` exists with `Services`, `ServicesDeps`, `Caller`, `Surface`, `IncidentService`, `ApprovalService`, `OperationCommands`, `HostService`, `BriefingService` as in section 6; no file under it imports `crate::api`, `crate::auth::interactions`, `crate::ingest`, `crate::runtime` or `crate::router`.
2. No file under `crates/domovoi-hub/src/auth/interactions/` contains `crate::api`, and `InteractionAuthorizer::new` takes `&Services`.
3. No file under `api/` or `auth/interactions/` names `ActionHandle` or `OperationHandle`; `ApiDeps` has no `actions_service` and no `operations` field; `.authorize(` on an `OperationHandle` has exactly one production call site, in `services/operations.rs`, and 59's chat step tests pass unmodified.
4. `cargo xtask arch-check` passes with `MOD-1` to `MOD-4` and `CALLER-1` in place; every rule has a failing fixture test; `cargo xtask arch-check --base origin/main` passes with the ADR present.
5. `Caller::web` and `Caller::chat` each have exactly one call site (`api/mod.rs`, `auth/interactions/mod.rs`); `HumanApproval::new` and `Authorization::human` still appear only under `src/auth/`.
6. The snapshots of 12.1 (first five tests) were committed before any refactor commit and `git diff <pin commit> -- crates/domovoi-hub/tests/hub/snapshots` shows no change to them at the end.
7. Every pre-existing test passes with edits limited to constructors (`ApiDeps`, `InteractionAuthorizer::new`, `ChatDeps`); no test or gate was deleted or weakened.
8. The regenerated OpenAPI document differs from the previous one only by four paths and the schemas `CancelOperationRequest`, `ResumeOperationRequest`, `BudgetExtensionDto`, `AnswerStepRequest`, `OperationControlDto`; `web/src/api/openapi.json` and `schema.d.ts` are regenerated, not hand-edited; `npm run typecheck` passes.
9. `POST .../cancel`, `.../pause`, `.../resume` and `.../steps/{step}/answer` behave exactly as the tables of 6.4, 6.5 and 10 say, including the idempotent repeats of cancel and pause and every 400, 404, 409 and 503 text.
10. Each control writes its audit row through the operation writer in the same transaction, with the person as actor; the answer's audit detail has no text; this layer writes no audit row of its own.
11. `PauseReason::Manual` exists (or the merged equivalent is used and reported), a manual pause resumes without an extension, and 26's five trace fixtures are byte-identical.
12. A `resume` can never set a budget to unlimited; bounds and caps of 6.2 hold; cancel reason and answer text are redacted before storage.
13. `as_of_seq` in every control response was read after the command and before the summary (test of 12.2).
14. Chat and web approval behaviour is unchanged, and `ApprovalService::approve` refuses a destructive action for a chat caller.
15. With orchestration off the four routes answer 503 "orchestration is not enabled" and no other behaviour changes.
16. `operation_controls_are_not_step_up_actions` passes and `step_up.rs` documents the decision.
17. The ADR, the `ARCHITECTURE.md` and `AGENTS.md` updates and the book pages of section 15 exist; `cargo xtask docs-links` passes.
18. No non-test source file exceeds 500 lines; no new dependency; no migration; no wire change.

## 14. Migration and compatibility

- **Existing installs, data and agents:** no change. No migration, no config key, no wire message. Orchestration stays off by default; the new routes answer 503 until it is on.
- **Rollback note:** a `{"type":"manual"}` pause reason in `operations.pause_reason` is unreadable by a hub built before this change (26 reads an unknown value as a store error). Operations exist only behind the off-by-default flag in V2 builds, so this affects no released install; the report says so.
- **Rust API:** the hub library's public `ApiDeps`, `ChatDeps` and `InteractionAuthorizer::new` change; their only consumers are `crates/domovoi` (it uses `domovoi_hub::api::MetricsRender` only), `examples/dev_hub.rs` and the tests, all updated here. `domovoi_hub::api::AgentAdmin` keeps resolving.
- **Web:** prompt 30 reads the four routes and `OperationControlDto` from the regenerated types; this prompt changes no web source file.
- **Earlier prompt:** 59's chat step buttons, tests and texts are unchanged for users; only their calls move into `OperationCommands`, and its call-site gate is tightened to one site (6.2).
- **Later prompts:** 34/48, 40-45 and 42/49 add their services under `crate::services` and their routes in `api`; 30 consumes the routes; 58 and 60 remove the legacy paths this layer wraps (`ApprovalService`, the `retriage` legacy branch).

## 15. Documentation requirements

- **ADR** `docs/adr/NNNN-use-case-layer.md` (next free number): contents listed in 6.6. Required because `xtask/src/arch/rules.rs` changes.
- **`ARCHITECTURE.md`**: a short paragraph in §7.1 (the hub's tasks) naming the use-case layer, the `Caller`, and that the web and chat transports call it; a bullet after R14 in §4 naming `MOD-1` to `MOD-4` and `CALLER-1` with one sentence each; no other section.
- **`AGENTS.md`** (committed; it may name agents): add a hard rule "Hub layering": HTTP and chat handlers call `crate::services`, never import each other and never hold `ActionHandle` or `OperationHandle`; a new command is a service method taking a `Caller`; enforced by `MOD-1` to `MOD-4`.
- **Book:** `docs/book/src/reference/api.md`: a section "Operation controls" describing the four routes, their bodies, limits, idempotent repeats and error texts (written for users: no prompt numbers, no RW ids); `docs/book/src/concepts/operations.md` (created by the operation aggregate; verify it exists): one paragraph on cancelling (a dispatched run is not recalled), pausing, resuming with a higher limit and answering a question. `cargo xtask docs-links`.
- **OpenAPI:** `cargo xtask openapi && cd web && npm run gen:api` (step 9).
- **Config reference:** not applicable (`cargo xtask docs-config` reports no change; no key was added). **CHANGELOG:** not edited (release-plz generates it).
- `web/DESIGN.md`, `TESTING.md`: not applicable.

---

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
