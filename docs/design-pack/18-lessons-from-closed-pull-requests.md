# Lessons from Domovoi's Closed Pull Requests

**Repository reviewed:** `RoastSlav/domovoi`  
**Review date:** 2026-11-12  
**Scope:** all 113 closed pull requests in this repository (PRs #1–#113; every number is a closed PR). The first pass (2026-11-12) covered #1–#111 and counted 110; #112 and #113 were merged afterwards and #108 was missed (it is the merged `chore: release v0.0.10`). Titles and descriptions were reviewed across the full set. Conversation comments were checked where present; the substantive maintainer follow-up comments on #107 and #110 were included. Code changes were inspected selectively where the description exposed concurrency, security, persistence, protocol, UI-system or operational-recovery risks. Routine dependency-only and release-only changes were classified but not treated as product-design evidence.

**Verification pass.** The lessons below were re-checked against the code at baseline `64117f0` (investigation G, with E and F). Each lesson carries a status:

- **VERIFIED**: the PR text and the current code or the PR itself agree (code or PR evidence cited).
- **STATIC**: established by reading code only. Nothing was executed and no failing test reproduces it.
- **UNVERIFIED**: taken from PR text; not checked against the code.

Several claims in the first version overstated what the repository enforces. They are corrected in place and listed in "Corrections from verification" below. Target rules ("Target rule:") describe what the rework should do; they are not claims about current behaviour.

This document is a lessons-learned record, not a requirement to preserve the current implementation. The rework may replace mechanisms, but should not repeat known failure modes or discard proven product requirements without a reason. Decisions that carry each lesson into the target design are in `docs/rework/03-architecture-decisions.md` (`RW-NN`); the "Rework invariants" section at the end maps invariants to them.

## Corrections from verification

| Earlier claim | Corrected statement | Status |
|---|---|---|
| "110 closed PRs", "#108 no closed PR" | 113 closed PRs. #108 is the merged v0.0.10 release. #112 and #113 are added below. | VERIFIED (`gh pr view`) |
| The mutation-feedback lint enforces feedback on every mutation | The ESLint rule is file-level: a file passes if it imports `useActionFeedback`, `InlineMessage` or `InlineError` anywhere, so one reported mutation excuses an unreported one in the same file (`web/eslint/domovoi-rules.js:10,128`). Four files carry a `// feedback-ok:` exemption. | STATIC |
| Unsaved-change warnings exist for settings and forms | The leave guard (`useLeaveGuard`) is used by the notes editor only (`web/src/features/notes/useLeaveGuard.tsx:16`). No other form blocks navigation while dirty. | STATIC |
| #97 "checksum-verified" upgrade | `domovoi upgrade` compares the archive with a `.sha256` file fetched from the same release origin (`crates/domovoi/src/cmd/upgrade/package.rs:13-30`). That detects corruption, not a hostile or compromised release. The SLSA attestation produced by the release workflow is not checked by the upgrade command (no attestation or signature code under `cmd/upgrade/`). | STATIC |
| `HumanApproval` is sealed by the type system | It is not. `HumanApproval::new` and `restore` are `pub` in `domovoi-core` (`core/src/action/approval.rs:54`). The restriction is a line-based text scan in `xtask arch-check` for the strings `HumanApproval::new(` and `HumanApproval::restore(` outside allowed path prefixes (`xtask/src/arch/scan.rs`, allow-lists at `xtask/src/arch/rules.rs:169-175`). A renamed import or a macro evades it. The type has no `Default` and no `Deserialize`, and gate tests exist. RW-11 in `docs/rework/03-architecture-decisions.md` records this correction. | VERIFIED (code) |
| Approval binds to "the exact plan" | The plan hash covers the serialised manifest, not the entry script's bytes (`core/src/skill/manifest.rs:145-151`; `runner/src/catalog/checks.rs:100-111` re-checks mode and path, not contents). The approve API (`hub/src/api/actions.rs:104-123`) takes only `confirm_text`; it carries no plan hash, so the server cannot tell what the operator saw. | STATIC |
| Single writer "guarantees" serialised decisions | True for action approve, deny, expiry, dispatch and completion. Not true for proposal token rotation (called from the triage task, `triage/proposals.rs:115`, `followup.rs:252`). The incident store has an unguarded `IncidentRepo::update`; its single-writer rule is a convention (ADR 0011). | STATIC |
| Restart recovery "resumes approved actions" | The mechanism exists and is tested with a fake link, but a real hub probably rejects them (D36 in `docs/rework/01-repository-reality.md`). See lesson 2. | STATIC (not reproduced at runtime) |
| Fallback "records model identity" | Only failure text carries provider and model. A successful `TriageReport` carries neither, and usage rows store only the provider id. | STATIC |
| #36 findings "to evaluate against current main" | They are still present on main (lesson 6). | STATIC |

## Executive summary

The repository's history contains several important lessons for the target design:

1. **Concurrency and durable state need explicit ownership.** Action decisions and action transitions were vulnerable to races until a single-writer model and guarded updates were introduced (#107, #109). Cached proposals also needed to be revalidated and converted back into live actions (#110).
2. **Restart recovery is part of correctness, not an operational afterthought.** Do not blindly replay actions whose execution outcome is unknown. Reconstruct rate limits and pending state from durable history; distinguish approved-but-not-started from dispatched-but-uncertain work (#109). The action loop guard is rebuilt from audit; the triage hourly cap (#112) is not, and catch-up of approved actions probably runs before agents reconnect (STATIC, lesson 2).
3. **An AI answer, cached report, notification or UI card is not the source of truth.** Revalidate against current capabilities, policy and target state at the point of action. UI affordances must reflect the current action token/state (#93, #107, #110).
4. **The UI needs explicit state design and quality gates.** Loading, empty, no-match, offline, partial failure, retry, unsaved changes, pending mutation and mutation failure are distinct states, not variations of an empty page (#100–#105, #111).
5. **A design system must be enforceable.** Semantic tokens, centralized status vocabulary, accessible components, contrast tests, axe checks, mutation feedback and bundle budgets prevent visual and interaction drift (#98, #100, #105). Enforcement today is partial: the mutation-feedback lint is file-level, not per call site, and only the notes editor blocks navigation with unsaved changes.
6. **Long-running and unreliable dependencies are normal.** Local models can be slow; providers can fail or return unusable answers; agents can disconnect; notification delivery can fail. Timeouts, streaming, fallback chains, durable queues and honest user-visible status must be designed in (#36, #50, #52, #54, #71, #87–#90).
7. **The agent must remain an enforcement boundary.** Hub requests are untrusted proposals; the agent revalidates local catalogue/policy and the runner constrains process execution (#6, #10, #13).
8. **Tests should prove invariants and contracts, not just happy paths.** Pure state machines, repository contract suites, protocol snapshots, adversarial gate tests, fake clocks and explicit regression tests make later redesign safer (#5–#13, #15–#16, #105, #107, #109–#110).
9. **The new product needs stronger abstractions than the old UI, but must keep proven user-facing behaviors.** Per-host capability enablement, host profiles, readable known-condition rules, rich logs, visible tools used, follow-up conversation history, manual steps and update guidance are useful foundations to carry forward conceptually (#63, #65–#69, #79–#84, #87–#95, #96–#97).
10. **A feature is not complete when its backend endpoint exists.** It also needs clear feedback, recovery paths, docs, tests, generated API artifacts and behavior on mobile/offline/error states.

## Lessons that directly affect the target architecture

### 1. Single-writer ownership for mutable workflows

**Evidence:** #16 established a single-writer incident manager; #107 moved action approve/deny/expiry decisions behind one writer; #109 extended that ownership to every action transition and startup recovery.

**Failure mode:** when multiple request handlers, expiry sweeps or executors independently read and write the same action, stale copies can overwrite a newer decision. The audit log can then describe a transition that the database did not actually retain. Approve and deny can race; expiry can overwrite a human decision; shutdown can close the database while queued decisions are still pending.

**Target rule:** every important state machine has one authoritative transition owner. Callers submit commands; the owner loads current state, validates the transition, performs a guarded write, records the corresponding audit event and publishes the resulting state. Database updates must be conditional on the expected prior state/version. A stale decision returns a conflict, not a silent overwrite.

**Status: VERIFIED for actions; STATIC for the caveats.** `actions/writer.rs:95,132`, guarded SQL `store/repos/actions.rs:173-178` (`WHERE id = ? AND json_extract(status,'$.type') = ?`), `update_from` is the only action write in the port. Caveats: the guard compares the status name, not a version, so ordering between two writes from the same status depends on the single writer; proposal token rotation calls `update_from` from the triage task (`triage/proposals.rs:115`) and bypasses the writer; the incident store has an unguarded `IncidentRepo::update` (`manager/actor.rs:259`), so incident single-writer is a convention (ADR 0011). Rework decisions: RW-03, RW-04.

**Apply to the rework:** incidents, operations, approvals, autonomy decisions, capability installation, agent enrollment, and long-running orchestration should each have explicit transition ownership. Do not automatically put all domains in one global writer; use the smallest ownership boundary that guarantees serializable decisions.

### 2. Crash recovery must distinguish safe resume from unknown execution

**Evidence:** #109 recovers approved actions once at startup, fails actions still marked dispatched with an explicit “check the host before running it again” reason, and rebuilds the hourly dispatch guard from audit history. #36 (closed without merge) proposed durable spool/notification hardening and corruption handling.

**Failure mode:** a process may crash after an external command started but before its result was persisted. Replaying it can duplicate a destructive or non-idempotent action. Resetting an in-memory rate limiter at restart can bypass safety limits. Conversely, failing every pending item loses approvals that were durable but not yet dispatched.

**Target rule:** persist operation checkpoints and action lifecycle transitions. On restart, classify work as safe-to-resume, safe-to-retry, needs-reconciliation, or outcome-unknown. Never replay outcome-unknown external actions automatically unless the operation is explicitly idempotent and its idempotency contract is proven. Reconstruct safety counters from durable events. Surface reconciliation needs in the UI.

**Status: VERIFIED for the mechanism (`actions/recover.rs:27,139,204`; `executor.rs:88`; tests `tests/services/recovery.rs`). STATIC for the gaps, none reproduced at runtime:**

- Catch-up runs before any agent reconnects (`runtime/mod.rs:230`). `AgentLink::skills` is empty for a host not yet connected (`agent_server/registry.rs:263-269`), so `revalidate` rejects valid approvals with "the skill is no longer on the host". The recovery tests use a fake link that hides this. Code reading only; a failing test is needed to confirm.
- "Outcome unknown" is a string inside `Failed { reason }`, not a status. A link error or deadline during `run_skill` is recorded as plain `Failed` although the command may have run.
- An `Approved` action has no age limit and no check that the incident is still open.
- The loop-guard slot is claimed before `wait_for_agent`, so an offline agent consumes a slot (`executor.rs:244`).
- The rate limiter rebuilt from audit covers the action loop guard only. The triage hourly window (#112) is in memory.

Rework decisions: RW-09, RW-30, RW-08.

**Design consequence:** operation execution requires durable step identity, idempotency classification, start/result records, deadlines, cancellation semantics and recovery behavior. A command's exit code alone is not proof of the external system's final state.

### 3. Revalidate at every trust boundary

**Evidence:** #6 validates skill manifests and typed arguments (its plan hash covers the manifest, not the script); #10 rechecks executable paths and permissions immediately before running; #13 makes the agent revalidate incoming requests locally; #22 revalidates before dispatch; #93 constrains model-proposed skills and arguments; #110 revalidates cached proposals against current skills.

**Failure mode:** a capability may have been removed or changed since a model generated a proposal. A cached report may be stale. A script path may have been replaced after catalogue loading. A plan may change between approval and execution.

**Target rule:** treat LLM output, cache entries, chat buttons, hub requests and UI state as untrusted or potentially stale. Revalidate capability identity/version, target, typed arguments, current policy, plan hash and local host constraints immediately before execution. Approval must bind to the exact plan that was reviewed. Any material change invalidates approval and requires a new decision.

**Status: VERIFIED for revalidation at the agent and for cached proposals; the "exact plan" binding is overstated.** The agent re-does catalogue lookup, risk, args and plan hash locally (`agent/src/requests/skills.rs:61-125`); #110 re-checks cached proposals against current enabled skills (`triage/run.rs:88-93`, `proposals.rs:48-60`). But the plan hash covers the serialised manifest, not the entry script's bytes, so editing `run` after approval executes under the approved hash (STATIC). The approve API carries no plan hash (STATIC). Weaker points in #110 (STATIC): the duplicate lookup is `Page::first(50)` and a listing error becomes an empty list, so a store error can store duplicates (`proposals.rs:67-91`). Human-only approval (#22) is enforced by a text scan plus a type with no `Deserialize`, not by compile-time sealing (see "Corrections from verification"). Rework decisions: RW-08, RW-11, RW-07.

**Design consequence:** a plan is a versioned immutable object or snapshot; the execution request references that exact plan. The UI should clearly show when a plan changed and why previous approval no longer applies.

### 4. Keep policy, knowledge, and experience separate

**Evidence:** #22 and #107 show the need for explicit approval and guarded state transitions; #81/#82 add host profile context; #66/#63 add known-condition rules; #91 stores conversation history; #110 shows cached triage output must not itself authorize a new action.

**Lesson:** preferences, profiles, notes, learned patterns, historical approvals and model conclusions are useful context, but none should silently grant authority. A prior successful action is not permission to repeat it. A known condition is not the same as an autonomy policy.

**Status: UNVERIFIED** as a description of current behaviour (#63, #66, #81, #82, #91 not re-read against code). The negative claim holds in the code reviewed: nothing the model returns reaches `HumanApproval`, and a report becomes `Proposed` actions only through `proposals::store` (E section 3). Rework decisions: RW-14, RW-10.

**Target rule:** maintain distinct stores and APIs for environment knowledge, authorization policy and operational experience. Policy evaluation is deterministic and separately auditable. Knowledge can explain why an action is useful, but cannot make it allowed.

### 5. Model failure is a normal control-flow path

**Evidence:** #54 increased API timeouts for slow local models; #71 added streaming and configurable timeout; #89 introduced provider fallback; #90 added ordered model fallback chains; #94 handles providers without tool calling by gathering read output before asking the model.

**Target rule:** model calls need explicit deadlines, cancellation, streaming, provider/model attribution, usage accounting, retry classification and fallback policy. Distinguish transport failure, timeout, rate limit, authentication failure, missing model, invalid structured output and tool-call incompatibility. Do not retry every error identically.

**Status: failure attribution VERIFIED; the rest of the target rule is not met today (STATIC).** `triage/run.rs` joins failure reasons with provider and model, and #113 redacts and clips them. Success attribution is absent: `TriageReport` has no provider or model field and usage rows store only `provider.id()`. A failed call records no usage. Every error falls through to the next provider identically (no classification). The only whole-call limit is `max_seconds`, checked between turns; the client has only an idle read timeout, so there is no hard per-call deadline. Rework decision: RW-06.

**Design consequence:** an operation can remain in a visible “waiting for model” or “provider unavailable” state. The UI must not imply that reasoning is progressing when the provider is stalled. Fallback model identity must be recorded in experience history.

### 6. Durable delivery and offline behavior

**Evidence:** #13 implements a signal spool; #36 proposes making check-state commits contingent on durable spool writes, validating/repairing corrupted spool state, retaining notification state until delivery succeeds and preserving scheduler errors. #50/#52 address Discord delivery and quiet-hour behavior. #87/#88 add bounded log retrieval over the agent link.

**Target rule:** distinguish “created”, “queued”, “sent”, “acknowledged”, “failed” and “expired” for messages and events. Persist work before acknowledging it. Use bounded retries with backoff and dead-letter/error visibility. Do not discard state merely because delivery failed. Offline agent data must be bounded and have explicit retention.

**Status: ack-after-persist VERIFIED; #36 findings STATIC and still present on main.** The hub acks signals only after the store accepts them (`agent_server/session.rs:90-105`) and the spool is bounded (`agent/src/spool.rs:14-22`). Still open: the agent scheduler saves check state even when the spool push fails (`agent/src/scheduler.rs:333-344`); the hub saves notification state before delivery succeeds and treats a state read error as default state (`runtime/delivery.rs:55-67,111-125,242-248`); a corrupt `spool.json` becomes default meta (`spool.rs:66-95`). The earlier text "not implemented" for #36 is corrected to "confirmed present". Rework decisions: RW-03, RW-15, RW-30.

**Design consequence:** the hub/agent link, operation event stream, notifications and emergency local autonomy need durable delivery semantics and reconciliation after reconnect.

### 7. Protocol and deployment compatibility are product behavior

**Evidence:** #9 defines a typed, versioned protocol with frame bounds and explicit unknown-message handling; #86 adds minimum supported release checks with a human-readable reason; #72 detects Docker/Podman; #34 documents/handles unavailable host checks in container mode.

**Target rule:** protocol evolution must be additive where possible, bounded, explicitly versioned and testable against compatibility matrices. Unsupported operations should return a typed explanation. Do not let incompatibility turn into an endless reconnect loop. Detect runtime capabilities rather than assuming the installation mode.

**Status: VERIFIED.** `proto/src/version.rs:17-26`, `hub/src/agent_server/handshake.rs:140-148,194-215`, frame cap `proto/src/envelope.rs:11`. Notes: `at_least` lets an unparseable agent version pass the floor (`version.rs:104-108`); a new `ReadTool` variant or a new `Risk` variant in `hello` still breaks old peers (D13 in `docs/rework/01-repository-reality.md`). Rework decision: RW-17.

**Design consequence:** UI should show hub/agent versions, compatibility state, runtime type, missing host permissions and unsupported capabilities with actionable remediation.

## Lessons that directly affect the UI plan

### 8. Page and component states must be designed, not improvised

**Evidence:** #100 established a component kit; #103 explicitly added loading, empty, error and offline states; #104 distinguishes no notes from no search matches, adds retry, unsaved-change warnings and mutation outcomes; #105 adds disabled reasons and mutation feedback (the unsaved-change guard is in the notes editor only); #111 refines overview, incident, host and settings hierarchy.

**Target rule:** every page, tab, table, graph, form and async action must specify at least: initial/loading, loaded, empty, no-results, partial-data, stale-data, offline, permission-denied, error/retry, submitting, success and failure states where relevant. “Empty” must never be used to conceal a failed request.

**Status: VERIFIED in part.** `QueryBoundary` and the state kit exist (#100, #103, #104). A failed refetch is dropped when data exists and there is no stale or refreshing indicator (STATIC, `QueryBoundary.tsx:143-189`). Only 24 of 94 data-using files use `QueryBoundary` (D34), so "every load" is a convention, not a check. Rework decision: RW-21.

**UI specification addition:** page specs must include state matrices and transitions, not only static content and happy-path interactions.

### 9. Mutations need clear feedback, reversibility and protection against accidental loss

**Evidence:** #104 adds a single save bar, unsaved counts, discard, field-level errors, connection-test feedback, typed host-name revocation, delete confirmation and Undo with exact expiry restoration. #103 reports mutation results and requires typed confirmation for agent revocation. #105 adds a lint that flags mutations without feedback unless the file carries a documented exemption (file-level, see lesson 14).

**Target rule:** every mutation must tell the user what changed, whether it succeeded, and what remains pending. Destructive or hard-to-reverse actions require clear consequence previews and confirmation appropriate to risk. Prefer Undo for reversible changes. Preserve user input on failure. Warn before navigating away from unsaved work.

**Status: PARTIALLY VERIFIED.** Undo with exact expiry restoration exists (`features/rules/DeleteRuleDialog.tsx`, test `rules.test.tsx:369`). Corrected claims: the guard that warns before navigating away from unsaved work is used by the notes editor only (`useLeaveGuard`, imported by `NoteEditor.tsx` only), not by settings or other forms (STATIC); and the lint that bans mutations without feedback is file-level, not per call site (lesson 14). `ChatPage.tsx:56` uses `mutateAsync` and reports through the thread instead (test `chat.test.tsx:108`). Typed host-name revocation (#103/#104) was not re-checked (UNVERIFIED).

**UI specification addition:** each action definition includes preconditions, confirmation level, pending state, success state, failure state, retry behavior and undo/rollback availability.

### 10. Context should follow the user, but global scope must remain explicit

**Evidence:** #64/#101 introduce global host selection and a selector that follows page navigation; #94 gives host-scoped chat read tools only when a host is selected and no tools for an all-hosts conversation; #103 makes host tabs deep-linkable through query parameters.

**Target rule:** selected host/environment context should persist across related pages and be visible. Global/all-hosts scope must be explicit and must not silently inherit a previous host's tools or permissions. Deep links should preserve meaningful tabs/filters and be shareable/bookmarkable where safe.

**Status: VERIFIED.** `hub/src/api/chat.rs:118-133` (`tool_host` is `None` for no host or an offline host), `engine/src/triage/chat.rs:149-162` (no skills without a host; only read-risk skills offered), tests in `tests/hub/api_chat.rs`. Web: the chat conversation is keyed by host (`ChatPage.tsx`); the selector is a view filter and mutations take explicit ids. Strengthening from the verification: the selected host persists in `localStorage` across sessions (`HostContext.tsx:6-27`), so a new session starts host-scoped; a deleted host name stays stored and gives a 404 from chat; the general chat has no server conversation (history is client `sessionStorage` per host). See INV-H1 and INV-H2. Rework decision: RW-20.

**Design consequence:** global Operator context, task scope, incident scope and capability target must be separately visible. Never infer a destructive target only from a hidden global selector.

### 11. Show what the AI actually used and preserve the conversation

**Evidence:** #69 exposes skills/tools used on incident details; #79 adds a manual-steps checklist; #81/#82 provide host profile context; #91 stores follow-up conversation with generation/race protection and chronological ordering; #92 shows the conversation, pending state and clear confirmation; #93 allows structured action proposals; #94/#95 create general host chat.

**Target rule:** the user should be able to inspect evidence, tools/capabilities invoked, manual steps, model answers and action proposals as separate structured elements. Preserve conversation context across turns, but make clearing/reset behavior explicit. A request still in flight must not be silently erased or resurrected by a late response.

**Status: VERIFIED for incident follow-up chat; the general chat has no server conversation.** Generation check `manager/annotate.rs:132-150`, `triage/history.rs` (`count_clear`, `asked_at` ordering), clear disabled while sending (`web/src/components/ChatThread.tsx:234`), the answer shown from the response when the turn is not yet remembered, history-load failure distinct from empty (`IncidentChat.tsx:35-45,127-129`), tests `incidentChat.test.tsx:291,306`. Weak points (STATIC): when the generation check drops an answer, `answer()` still returns `Ok(answer)`, so a client can show a reply to a conversation another tab cleared; a failed timeline read captures generation 0, which wrongly drops the answer if any earlier clear exists (`followup.rs:182-188`); the client matches a landed turn by question text, so a repeated question can match an older turn (`IncidentChat.tsx:38`); tool use from a failed follow-up is not recorded (`followup.rs:235`). Rework decision: RW-20 (request-id keyed pending turn, stale responses discarded, composer draft kept until the send commits). See INV-Q1 to INV-Q3.

**Design consequence:** Operator workspace should be built around a durable task/conversation identity, with linked orchestration, evidence and action history. Chat is not just a transient message list.

### 12. Live views need bounded refresh, useful controls and performance budgets

**Evidence:** #87/#88 bound in-memory logs, redact before storage and again at the API/agent boundary, and fetch remote logs in chunks. #103 adds live toggle, search/highlighting, jump-to-latest, wrapping, copy and show-older; polls host list only while waiting for enrollment. #105 memoizes large tables and sets bundle/CSS budgets.

**Target rule:** real-time views need explicit live/pause state, bounded windows, pagination/virtualization, search, copy/export and clear freshness indicators. Poll only while a user-visible condition requires it; prefer event streams where appropriate. Keep the interface responsive under large histories and frequent updates.

**Status: VERIFIED for logs (#87/#88); UNVERIFIED for the #103 live-view controls.** Ring capacity 2000, redacted at capture and again in the API (`domovoi/src/log_ring.rs:15-75`, `hub/src/api/logs.rs:54`), agent cap 2000 lines (`agent/src/requests/mod.rs:43`). SSE has no event ids and a slow client silently loses events (D14). Rework decision: RW-15.

**Design consequence:** orchestration graph and event timeline need incremental updates, bounded rendering and stable selection/scroll behavior. New events must not unexpectedly move the user's current inspection target.

### 13. Technical language should be explained, not erased

**Evidence:** #65 adds explanations for pages and jargon; #111 keeps detailed evidence and operational labels while improving hierarchy.

**Status: UNVERIFIED** (#65, #111 wording not checked against the UI).

**Target rule:** retain precise terminology (orchestration, capability, policy, verification) and pair it with concise explanations. Do not dumb down the detailed view. Use configurable information density and progressive disclosure, with the detailed experience as the reference design.

### 14. A design system needs automated enforcement

**Evidence:** #98 centralizes design tokens, themes and status tones; #100 creates shared feedback/hint/state/form components; #105 adds WCAG AA contrast checks, axe checks, voice/tone lint, mutation-feedback rules, disabled-reason requirements, bundle-size budgets and memoization checks.

**Target rule:** define semantic tokens and shared component contracts, then enforce them in CI. Avoid raw colors and one-off status interpretations. Components should expose accessible names, focus behavior, disabled reasons and consistent feedback. Accessibility and performance are acceptance criteria, not final polish.

**Status: PARTIALLY VERIFIED; "enforced in CI" is overstated.** Token, status-vocabulary, icon, inline-style, raw-button and file-length rules are lint or test checks (`web/eslint/domovoi-rules.js`, `src/guardrails.test.ts`, `styles/contrast.test.ts`). Weaker than described (STATIC): the mutation-feedback rule passes a file if it imports `useActionFeedback`, `InlineMessage` or `InlineError` anywhere (`domovoi-rules.js:10,128`), so one reported mutation excuses an unreported one in the same file; four files use a `// feedback-ok:` header exemption that does not prove the wrapping; the disabled-reason check is likewise structural; axe runs in jsdom with colour contrast excluded (contrast is covered by the token-pair test instead); `api/health.ts` uses raw `fetch`. Rework decision: RW-21.

**Important nuance:** shared components should support consistent behavior without forcing every page into the same layout. Reuse the design language and interaction contracts; don't let the component kit dictate the information architecture.

## Capability, onboarding and operations lessons

### 15. Capability lifecycle needs more than a list of scripts

**Evidence:** #83 exposes skill README, limits and recent runs; #84 allows enable/disable per host; #103 shows risk, timeout, applies-to and last-used; #44 installs starter skills by default and suggests restart after upgrade.

**Status: #84 VERIFIED; the rest UNVERIFIED.** `PolicyLink` (`hub/src/agent_server/skill_policy.rs:157-221`) filters skills and refuses `run_tool` read skills and `run_skill` for disabled ones, with a mutex serialising read-modify-write (`:32-45,93-107`); every consumer is handed `policy_link` (`runtime/mod.rs:216,220,235,269`, `profile/collect.rs:51,193`). Weak points (STATIC): enforcement is a wrapper the consumer must be handed; `server.link()` stays public and `AgentAdmin::skills` is deliberately unfiltered, with no arch-check rule against bypass; `run_check` is not policy-filtered (`skill_policy.rs:186-195`); the policy is hub-side only and keyed by host name. Rework decisions: RW-07, RW-26.

**Target rule:** future capability management should distinguish discover, inspect, compatibility, install, enable, configure, authorize, update, disable, uninstall and audit. These are different operations. Installing a capability must not grant it permission. Per-agent/host compatibility and enablement must be explicit.

### 16. Host profiles and environment discovery are valuable, but provenance matters

**Evidence:** #81 collects and stores host profile; #82 injects profile into triage/chat/notes; #67 adds expected SSH logins and a login time window; #72 detects runtime type.

**Status: #81/#82 snapshot-diff behaviour VERIFIED; the rest UNVERIFIED.** `hub/src/profile/diff.rs:26-31,75` compares snapshots only when both sides answered (tests `tests/hub/profile.rs:541,566`). Not re-checked: #67 default login window, #72 runtime detection, #81 fence escaping. Rework decision: RW-14.

**Target rule:** use discovered host facts to tailor questions and diagnostics, but show source, freshness and uncertainty. Auto-discovered information must not silently become user-confirmed constraints or permissions. Profiles should be editable and linked to the evidence that produced them.

### 17. Known conditions should be understandable and scoped

**Evidence:** #63 provides UI management with plain-language condition rules; #66 supports dismissing an incident as a known condition; #104 shows expiry, creator, readable phrases, operator-specific inputs and Undo.

**Status: #66 VERIFIED (rule stored before dismiss, `hub/src/api/incidents.rs:343-398`; the rule audit runs after dismiss, so its failure returns an error for an already-done dismiss); #63 and #104 UNVERIFIED.**

**Target rule:** known conditions are explicit, scoped, time-bounded when appropriate and reviewable. The UI should explain exactly what will be suppressed and until when. A known condition suppresses expected noise; it must not conceal a security event or authorize a remediation action.

### 18. Installation and upgrade flows need operational safety

**Evidence:** #33 deployment methods; #32 guided first run and UI-managed settings; #43 LAN/integration setup; #44 starter capabilities and restart guidance; #96 safe release discovery; #97 in-place upgrade that compares the archive with a `.sha256` file from the same release origin (integrity, not authenticity) and does not silently restart services.

**Status: #96 VERIFIED; #97 overstated.** The release-tag allowlist `is_release_tag` (`proto/src/version.rs:97-103`) is used in `cmd/upgrade/fetch.rs:48`. The #97 upgrade is integrity-checked, not authenticated: the archive is compared with a `.sha256` file from the same release origin (`cmd/upgrade/package.rs:13-30`), which detects corruption but not a compromised release, and the attestation the release workflow produces is not verified by the command (STATIC). `install.sh` also verifies SHA-256 (F). Rework decisions: RW-26, RW-31.

**Target rule:** setup and upgrade flows must explain prerequisites, what will change, how to recover and whether a restart is required. Pin versions, verify downloaded artifacts and never execute untrusted release metadata as shell input. Separate downloading/replacing a binary from restarting services.

## Unmerged closed PRs: useful signals, not current behavior

Seven closed PRs were not merged: #1–#4 and #14 are dependency updates; #36 is a substantive hardening/audit PR; #38 is an attempted `v0.1.0` release PR. Do not assume their changes are in `main`.

- **#36 — signal and notification delivery hardening (not merged):** proposed durable-spool ordering, corrupted-spool repair/rejection, notification retention until delivery succeeds, repair of missing discussion threads, digest paging, scheduler error visibility, generated API artifact checks and documentation/link audits. These are valuable review findings. Verification against `main` (STATIC) found the spool-before-state, notification-state-before-delivery and state-read-error-as-default findings still present (lesson 6); do not treat them as implemented.
- **#38 — release `v0.1.0` (not merged):** release-plz generated a release PR that was closed. It is historical release automation, not evidence that a `v0.1.0` release shipped.
- **#1–#4 and #14:** dependency bumps closed without merge. No product behavior should be inferred from them.

## PR groups reviewed

The following index records the complete closed-PR set and the primary topic inferred from each title/description. Routine dependency and release PRs are included so the audit scope is explicit.

| PRs | Primary topic / lesson |
|---|---|
| #113 | Redact a provider failure reason and clip it to 500 characters before it leaves the engine (provider error text is a leak channel). |
| #112 | One admission gate caps triage, questions and chat: shared hourly and monthly limits, runs reserved for incident triage, free refusal, 429 with `Retry-After`. |
| #111 | Current UI overhaul: operator-oriented hierarchy, dense evidence, host views, responsive shell. |
| #110 | Revalidate cached proposals; deduplicate actions; rotate stale chat approval tokens. |
| #109 | Single writer for all action transitions; crash recovery; rebuild rate limits from audit. |
| #108 | Release PR `chore: release v0.0.10`, merged. Release-only; no product lesson. |
| #107 | Serialize approve/deny/expiry; guarded writes; drain queued decisions on shutdown. |
| #106 | Playwright patch dependency update. |
| #105 | UI guardrails: contrast, axe, voice, mutation feedback, bundle budgets and rendering performance. |
| #104 | Settings/notes/known-condition forms; unsaved changes, retry, undo, validation and explicit read-only sources. |
| #103 | Host/skill/log pages; offline/error states, host-scoped navigation, live log controls and mutation feedback. |
| #102 | Incident/approval/chat pages; needs-attention grouping, mobile behavior and action-focused presentation. |
| #101 | App shell, grouped navigation, host selector, lazy-loaded pages. |
| #100 | Shared UI kit for feedback, hints, states and forms. |
| #99, #85, #70, #55, #51, #48, #45, #42, #40 | Release PRs; generated version/changelog history. |
| #98 | Design tokens, light/dark/system themes, semantic tone/status system and icons. |
| #97 | In-place binary upgrade checked against a same-origin `.sha256` file (integrity, not authenticity); no implicit restart. |
| #96 | Release update discovery and safe, pinned update instructions. |
| #95 | General Chat UI, host scope and per-host conversation persistence. |
| #94 | General host chat with scoped read tools and bounded/redacted context. |
| #93 | Structured chat action proposals, deduplication and no direct execution. |
| #92 | Incident follow-up chat UI, pending state, chronological conversation and clear confirmation. |
| #91 | Persisted follow-up conversations, generation/race handling and ordered replay. |
| #90 | Ordered model fallback chain within and across providers. |
| #89 | Secondary provider fallback and provider-specific connection testing. |
| #88 | Retrieve agent logs over hub-agent protocol in bounded chunks. |
| #87 | Bounded hub log ring, redaction and Logs page. |
| #86 | Minimum supported hub/agent release and explicit incompatibility reason. |
| #84 | Per-host skill enablement. |
| #83 | Skill detail view: README, limits, parameters and recent runs. |
| #82 | Host profile supplied to triage, chat and notes. |
| #81 | Host profile collection, persistence and UI. |
| #80, #73 | Additional starter skills. |
| #79 | Manual-step checklist in triage report and incident UI. |
| #78, #77, #76, #75, #74 | Dependency updates (metrics exporter, Rust, Node types, web dependencies, BusyBox). |
| #72 | Docker/Podman runtime detection and hub selection. |
| #71 | LLM response streaming and configurable timeout. |
| #69 | Show skills/tools used on incident details. |
| #68, #49, #37 | Release pipeline/attestation/changelog fixes. |
| #67 | Expected SSH logins and login time window. |
| #66 | Dismiss an incident as a known condition. |
| #65 | Explain page purpose and jargon in the UI. |
| #64 | Sidebar shell and global host selector. |
| #63 | Manage known conditions through the UI. |
| #62 | Group incidents by state. |
| #61 | Rerun triage from scratch without duplicate proposals. |
| #60 | Bound retries across mixed timelines and freed concurrency slots. |
| #59 | Retry skipped/failed triage under hourly limits. |
| #58 | Correct resolved-incident status presentation. |
| #57 | Document local testing and CI checks. |
| #56 | Distinguish hub-only host from offline agent hosts. |
| #54 | Longer API request timeout for slow local models. |
| #53 | Discord test uses a placeholder approver when none is configured. |
| #52 | Deliver open-incident cards when quiet hours end. |
| #50 | Discord digest and one-way message delivery. |
| #47, #46 | Retry triage from incident page; setup/enrollment fixes. |
| #44 | Install starter skills by default and suggest restart on upgrade. |
| #43 | Improve LAN and integration configuration. |
| #41, #39 | Consolidated CI and checks scoped to changed areas. |
| #38 | Closed, unmerged `v0.1.0` release attempt. |
| #36 | Closed, unmerged signal/notification durability and audit hardening proposal. |
| #35 | Starter skills, triage drills and documentation. |
| #34 | Handle checks unavailable in container mode. |
| #33 | Installer, systemd, container images and install guides. |
| #32 | UI-managed hub settings and guided first run. |
| #31 | CLI roles, single-box mode, metrics and self-test. |
| #30 | Hub runtime lifecycle assembly. |
| #29 | Discord threads/buttons/follow-ups and webhook notifier. |
| #28, #24 | ADR/documentation proposals; not treated as binding decisions in this design pack. |
| #27 | Initial host/enrollment/skills/notes/settings/digest UI pages. |
| #26 | Redact triage reports and inbound signal text. |
| #25 | Initial app shell, authentication, live incidents and approvals. |
| #23 | JSON API, sessions, CSRF, SSE and generated OpenAPI. |
| #22 | Human-only approvals, revalidated dispatch, verification and audit. |
| #21 | Triage orchestrator with read-only tool loop, caching and provider fallbacks. |
| #20 | LLM provider adapters. |
| #19 | Signed webhook ingestion and agent relay. |
| #18 | Agent server, enrollment, pinned TLS, presence and host-down detection. |
| #17 | Notification policy, cards, digest and advisories. |
| #16 | Single-writer incident manager, correlation and suppression. |
| #15 | SQLite repositories, FTS5, append-only audit and backups. |
| #14 | SHA-2 dependency update closed without merge. |
| #13 | Agent enrollment, pinned TLS, scheduler, spool and local request handling. |
| #12 | Container/unit/SMART/security/network checks. |
| #11 | Check framework, bounded runs and system checks. |
| #10 | Sole process runner, sandbox constraints and skill catalogue revalidation. |
| #9 | Typed/versioned hub-agent protocol, frame bounds and compatibility tests. |
| #8 | Typed configuration, validation and secret indirection. |
| #7 | Ports, fakes and repository contract suites. |
| #6 | Typed skills/actions, plan hashes, approval model and closed read-tool catalogue. |
| #5 | Pure incident state machine and stable fingerprints. |
| #1–#4 | Dependency update PRs closed without merge. |

## Concrete requirements to carry into the rework

- [ ] Every mutable domain has one authoritative transition owner and guarded state updates.
- [ ] Restart recovery classifies pending work; dispatched-but-uncertain operations are not blindly replayed.
- [ ] Action approval binds to an immutable/versioned plan; any material plan change invalidates approval.
- [ ] Cached model output is never authority; revalidate it against current capability and policy state.
- [ ] Capability installation, enablement, configuration and authorization are separate lifecycle steps.
- [ ] Agent-side enforcement remains mandatory even when the hub has a richer policy engine.
- [ ] Model provider failure, timeout, fallback and streaming are explicit operation states.
- [ ] Agent signals and notifications have durable delivery/acknowledgement semantics.
- [ ] UI page specs include complete async/error/offline/empty/partial states and mutation feedback.
- [ ] Destructive operations have risk-appropriate confirmation, typed target confirmation where justified, and undo/rollback when possible.
- [ ] Host/environment context is visible and scoped; global chat does not inherit host-only tools implicitly.
- [ ] Conversations, operation events, evidence and action history remain linked and race-safe.
- [ ] Live views are bounded, incremental, searchable and do not jump the user's selection/scroll unexpectedly.
- [ ] Semantic tokens, accessible controls, contrast/axe tests, performance budgets and mutation-feedback checks run in CI.
- [ ] Documentation, generated API artifacts, examples and acceptance tests ship with the feature, not as cleanup later.
- [ ] Every model call passes one admission gate whose limits survive restart and declare fail-open or fail-closed; a cache hit costs no model slot.
- [ ] Every error string that reaches a user, a chat reply or an external log passes the redactor and a length cap.
- [ ] Approval binds to implementation bytes (not only a manifest), carries the plan hash the user saw, and expires.
- [ ] Event kinds that drive control flow are typed, not parsed from strings.

## What this review does not decide

- Whether to retain Rust, SQLite, the current agent protocol or any current UI component. The PR history supplies evidence, not a veto on the target architecture.
- The exact future operation/incident schema, workflow engine, graph store, capability package format or policy language. Those remain design tasks in the relevant pack documents.
- Whether every #36 hardening proposal should be merged into the future design. Its durability findings should be checked against the current code and tests before implementation.

## Additional lessons found in PR review comments

The PR descriptions often summarize the intended change; the maintainer's follow-up comments record edge cases discovered during review. These are particularly useful because they show where an apparently complete feature was still unsafe or misleading.

- **Never treat malformed or quoted model text as structured intent.** **Status: VERIFIED.** The follow-up parser takes one complete JSON array after the last `PROPOSALS:` line, `ActionProposal` has `deny_unknown_fields`, and count and per-line caps apply (`triage/followup.rs:16-24,116-142,242-249`; `core/src/triage/report.rs:37`). Weak point (STATIC): malformed output goes through `unwrap_or_default()`, so a broken payload silently yields no proposals, and the marker is out-of-band text rather than provider-native structure. RW-06 replaces it with schema-validated meta-tool calls. Cached proposals (#110) are revalidated through the same `check` (lesson 3). #93 found that extracting the first `[` through the last `]` could turn quoted prose into an action proposal. Parse a complete, explicitly delimited structured payload; reject surrounding prose and unknown fields. Prefer provider-native structured output when available. Also enforce proposal count/size limits, deduplicate within a batch, and address cross-request uniqueness at the store/serialization boundary.
- **Bound every model input, not only the visible prompt.** **Status: VERIFIED with a difference.** General chat bounds and redacts every replayed turn (`triage/chat.rs:20-35,60-67`). The incident follow-up replays stored turns without re-redacting, relying on redaction at write time (`followup.rs:84-107`, STATIC), and there is no total prompt cap across context, tool results and history (D11 in `docs/rework/01-repository-reality.md`). #94/#95 found that historical turns and skill catalogues could exceed budgets and that historical user/assistant messages could carry secrets. Redact every historical turn before sending it to a provider; cap each turn, total history, question, catalogue and host overview; drop oldest history deliberately and preserve complete question/answer pairs where possible. A global chat without a selected host should not receive host tools; offline hosts should receive no tools.
- **A chat must never lose a successful answer or user input.** **Status: VERIFIED for the incident chat** (lesson 11 lists the remaining weak points; the general chat keeps history client-side only). #92 found a race where the response could arrive before history refreshed, leaving “Thinking…” stuck or making the answer disappear. Pending state should follow the request mutation; render the answer from the response if persistence lags; clear the composer only on success; retain input on failure; distinguish history-load failure from an empty conversation; block clear while a request is active and prevent a stale answer from reappearing after clear. Enforce the server's character limit in the client too.
- **Conversation clear needs generation semantics.** **Status: VERIFIED for incident follow-ups; STATIC weak points in lesson 11.** The general chat has no server conversation, so no generation exists to protect. #91 prevents a slow in-flight answer from restoring a conversation after the user clears it. Requests capture a conversation generation and asking time; persistence rejects stale generations and history replays in asking order even if responses finish out of order. Apply the same pattern to any resettable long-running UI or task context.
- **Provider fallback must preserve diagnostic information.** **Status: VERIFIED for failures, STATIC for successes.** Failure reasons carry provider and model and are now redacted and clipped (#113); a successful output records neither (lesson 5). #89 found that fallback could hide the primary provider's failure and that invalid model output could omit provider identity. Aggregate provider-specific failure reasons and include provider/model attribution in diagnostics and operation history.
- **Never trust release metadata just because it parses as a version.** **Status: VERIFIED** (`is_release_tag`, `proto/src/version.rs:97-103`, used in `cmd/upgrade/fetch.rs:48`). #96 found tag-injection risk: a parser that stops at a prerelease/build separator can accept a version prefix while ignoring unsafe suffix characters. Validate the entire release tag against a strict allowlist before using it in URLs, UI or copyable shell commands. Pin the exact image version in generated Docker upgrade commands. Bound HTTP response bodies at the shared HTTP layer, not just one call site.
- **Protocol additions must be understood by old peers.** **Status: VERIFIED** (`handshake.rs:140-148,194-215`). #86 found that sending a newly introduced rejection reason to an old agent could leave it in a reconnect loop. Only send a reason the peer version can understand; otherwise use a compatible terminal rejection. Authenticate the peer before recording a version refusal, so unauthenticated traffic cannot create misleading host events.
- **A disabled capability must be disabled on every path.** **Status: VERIFIED for current consumers; STATIC for the structure.** Every consumer is handed the policy wrapper, but the raw link stays reachable and `run_check` is unfiltered (lesson 15). #84 found host-profile collection bypassing the skill policy wrapper even though triage and action execution used it. All consumers (profile collection, chat, incident investigation, scheduled jobs, etc.) must use the same policy-enforcing capability access layer. Concurrent per-host enable/disable updates need serialized read-modify-write behavior so one update cannot silently erase another.
- **Missing data must not be interpreted as deletion or recovery.** **Status: VERIFIED for host profiles** (`profile/diff.rs:26-31,75`). **STATIC: the same lesson is broken elsewhere.** An offline or not-yet-connected agent looks like "no skills" (lesson 2); a notification-state read error becomes default state (lesson 6); a failed duplicate lookup becomes an empty list (lesson 3); a budget read error becomes "not spent" (lesson 5 and #112). #82 found that a failed container or listening-port query could make the next profile claim all previous entities had disappeared. Compare snapshots only when both sides contain valid data; represent collection failure/unknown separately from an empty successful result. Do not infer removals from missing observations.
- **Generated host profiles need explicit limits and honest semantics.** **Status: UNVERIFIED** (PR text only; fence escaping, refresh concurrency and note editability were not re-checked). #81 found read skills with required parameters being called with empty arguments, generated profile notes overwriting prior content, tool output breaking Markdown fences, profile refresh being synchronous/expensive, and editable generated notes being ambiguous. Skip skills that cannot be called safely without parameters; escape/fence arbitrary tool output; label generated data as a latest snapshot rather than a history if it is overwritten; decide whether generated records are editable; bound refresh work and add backoff for failed sections.
- **Tool-use history must distinguish not-run from failed.** **Status: VERIFIED in part.** Names are validated against `ReadTool::ALL` and records carry `failed` and `refused` flags (`triage/agent.rs:20-33`); there is no skipped or cancelled outcome, and tool use from a failed follow-up is not recorded (STATIC, `followup.rs:235`). #69 found invented model-supplied tool names, budget-refused calls counted as failures, missing prefetched calls for providers without tool support, and identical names across tool/skill categories being merged. Validate names against registries, record category and run identity, and use distinct outcomes such as succeeded, failed, refused/not-run, skipped and cancelled. Record partial/in-flight runs if the UI is expected to show them.
- **Network-security defaults must preserve access.** **Status: UNVERIFIED** (#67 defaults not re-checked). #67 found that a login-allowance window could accidentally suppress all SSH logins and that one bare IP address was rejected. Default an “expected login” window to LAN-only, require explicit opt-in for internet-origin logins, confirm before creating broad rules, support /32 and /128 single-address semantics, and derive active-window UI state from persisted rules so reloads do not lie.
- **Create suppression rules before closing the incident they suppress.** **Status: VERIFIED** (`hub/src/api/incidents.rs:343-398`; see lesson 17 for the audit-after-dismiss caveat). #66 found a race where a new signal could arrive between dismissing an incident and storing its known-condition rule. Validate and persist the rule first, then close/dismiss; if the later step fails, leave the rule visible and explain it. The UI must state that a rule can cover all severities for that check/target and must not offer dismissal while triage is still pending.
- **Retry budgets need typed reasons.** **Status: VERIFIED** (reason enum `triage/mod.rs:180`, per-reason limit `triage/queue.rs:206-222`). STATIC: retry state is rebuilt on startup from timeline kinds that are plain strings, so the typing stops at the memory boundary. #60 found that cap-deferred retries and failure retries were being conflated, so an old failure chain could block a later retry that was only deferred by a concurrency cap. Track why a run was deferred and apply each limit only to the retry class it governs. Test mixed timelines and slot release behavior.
- **Do not let optional integration setup block a connection test for the wrong reason.** **Status: UNVERIFIED** (#53 not re-checked). #53 found that a Discord test could fail validation because no approver list had yet been saved, even though the test itself should be able to reach Discord. Use narrowly scoped test-only placeholders where appropriate, explain their scope and ensure they are not persisted as real authorization.
- **Hardware diagnostics must not create side effects or leak identifiers.** **Status: VERIFIED in part.** `smartctl -n standby` (`examples/skills/hardware_health/run:53`) and the "could not list" versus "no UPS detected" distinction (`power_status/run`) were confirmed; serial-number filtering before model exposure was not re-checked (UNVERIFIED). #73 found that SMART health checks should not wake sleeping disks, serial numbers should be filtered before model/timeline exposure, and UPS command failures must not be rendered as “no UPS detected.” Use non-waking commands, bounded execution time and precise “unavailable/failed/not detected” distinctions.
- **Web fixtures must not depend on the real clock (UNVERIFIED as a current defect; recorded in the #107 comments).** A web test failed because fixtures were relative to the real date. Tests of time-dependent behaviour, web as well as Rust, use a fixed or fake clock.
- **Review findings should be classified, not silently ignored.** **Status: process lesson, not tested against code.** It applies to this document too: every item above carries a status so "reviewed" is not read as "resolved". Several comments explicitly left non-blocking items as separate design decisions (for example, profile refresh concurrency, generated-note editability, per-section size caps, API body bounds and cross-request proposal uniqueness). Preserve these as open design questions when the affected feature is selected; do not assume “reviewed” means “resolved.”

## Lessons added by #112, #113 and the verification pass

- **#112, one admission gate caps triage, questions and chat (VERIFIED, PR text and `triage/admission.rs`).** Every model call (triage, follow-up question on web and Discord, general chat) goes through one `admit(shared, Purpose)` gate with one hourly count and one monthly budget. Questions may not use the last `reserved_for_incidents` runs, so a few questions cannot starve triage of a new incident. A busy refusal takes no run of the hour; a refused question answers 429 with `Retry-After`. Lessons: put one gate in front of every model path, reserve capacity for the safety-critical path, and make a refusal free. Gaps (STATIC): the hourly window is in memory and resets on every restart and every settings save; a budget read error fails open (`admission.rs:52-59`); a start is counted before the call and never refunded; the gate runs before the cache lookup, so a cache hit spends a model slot (lesson below).
- **#113, redact a provider failure reason before it leaves the engine (VERIFIED, PR text and tests such as `a_failed_follow_up_never_shows_a_secret_from_the_provider_error`).** `LlmError::Transport` and `InvalidResponse` carry provider free text (response excerpts, URLs). Both question paths now build the reason through one `provider_failure` helper that redacts once and clips to 500 characters. Lesson: provider error text is a leak channel, separate from answer and history redaction. Every error string that reaches a user, a chat reply or a log shipped elsewhere passes the redactor and a length cap. The fix covers the two question paths; other places that surface provider text were not audited.
- **Cache hits spend a model slot (STATIC).** The cache is consulted after admission (`triage/run.rs:59` before `:88`), so a cache hit consumes an hourly slot even though no model call happens. A cache must cost no model capacity.
- **Timeline kinds are stringly typed and carry control flow (STATIC).** Retry limits, failure-chain length and conversation boundaries are derived by parsing `incident_events.kind` prefixes (`triage.skipped`, `triage.failed`, `triage.followup`, `followup.cleared`; `triage/queue.rs:189-222`, `triage/history.rs`). Give event kinds a closed type; do not drive control flow from display strings.
- **Admission limits must be durable and say how they fail (STATIC).** The hourly cap is memory-only and resets on restart; the monthly budget is durable but fails open on a read error. State per limit whether it fails open or closed, and rebuild counters from durable history.
- **Model attribution and accounting are incomplete (STATIC).** A stored report, follow-up or chat answer does not record the model that produced it (`core/src/triage/report.rs:52-76`; usage stores `provider.id()` only, `triage/agent.rs:110-120`). A call that fails records no usage (`provider.complete(req).await?` returns first).
- **No hard per-call deadline (STATIC).** The LLM client has only an idle read timeout that restarts on each byte (`domovoi-llm/src/http.rs:20-30`); `max_seconds` is checked between turns (`triage/agent.rs:146-148`). One streaming call can run unbounded while bytes keep arriving.
- **Spool before state (STATIC; the #36 finding still present).** The agent scheduler applies `set_firing` in memory, logs a spool push failure, then saves check state anyway (`domovoi-agent/src/scheduler.rs:333-344`). A transition can be committed to state and lost from the spool, and the check will not re-emit it.
- **Notification state must follow delivery, and a read error is not default state (STATIC; #36).** The hub saves notification state (`first_alert_at`, `held`, `last_reminder_at`) before delivery succeeds (`hub/src/runtime/delivery.rs:55-67`; a failed delivery leaves state saying it was sent, `:111-125`). A state read error becomes default state (`:242-248`), which can produce a duplicate card. A quiet-hours release is persisted before sending (`dispatcher.rs:207-212`).
- **Restart catch-up runs before agents reconnect (STATIC; not reproduced at runtime).** `ActionService::spawn` runs at `hub/src/runtime/mod.rs:230`, before listeners serve. `AgentLink::skills` returns empty for an agent that has not connected (`agent_server/registry.rs:263-269`), so `revalidate` (`actions/executor.rs:325-332`) finds no skill and rejects the approval as "the skill is no longer on the host". The recovery tests use `FakeAgentLink`, which keeps skills while connected, so they cannot see it. This is code reading; a failing test is needed to confirm it (D36 in `docs/rework/01-repository-reality.md`).
- **Approvals have no age (STATIC).** An `Approved` action carries no age limit at dispatch; catch-up can run an approval made days earlier without checking that the incident is open (`executor.rs:219-262`).
- **Audit and state diverge in one direction (STATIC).** `record_from` writes state, then appends audit; an audit failure is only logged (`actions/mod.rs:127-143`). The loop-guard reload reads `dispatched` audit rows (`recover.rs:139`), so a lost row undercounts after a restart.
- **Capability policy by wrapper is a convention (STATIC).** `PolicyLink` filters skills for every consumer that is handed it, but `server.link()` stays public and no arch-check rule prevents a new consumer from using the raw link.

## Review process for future implementation

For every substantial PR in the rework, use a repeatable review checklist:

1. Read the full PR description and discussion, not just the diff summary.
2. Identify invariants the change touches and name the authoritative owner for each state transition.
3. Ask what happens under duplicate requests, concurrent requests, stale cache, disconnect, process restart and partial persistence.
4. Ask whether missing/failed data is distinguishable from valid empty data.
5. Check untrusted inputs: model output, history, tool arguments, external metadata, logs, webhook payloads and generated shell commands.
6. Verify that every UI async path has loading, success, failure, retry and stale-response behavior.
7. Require regression tests for each discovered edge case; do not weaken existing security gates to make a test pass.
8. Record unresolved review findings in the relevant design document/open-question list, with an owner or decision milestone.

## Rework invariants

Testable invariants taken from investigation G, with the test each one needs. Each is phrased as a test name. "Carried by" names the decision in `docs/rework/03-architecture-decisions.md` (`RW-NN`) that carries it; where 03 has no decision for it, the target-architecture document that does is named (`04` section numbers), and where nothing carries it the row says so. "Evidence" is the status of the underlying finding at baseline `64117f0`: VERIFIED (the safeguard exists and was confirmed in code or PR text), STATIC (the gap was found by code reading; no failing test reproduces it) or UNVERIFIED.

| ID | Test name | Carried by | Evidence |
|---|---|---|---|
| INV-W1 | `stale_write_conflicts_and_changes_nothing` (two concurrent commands on one aggregate) | RW-03 | VERIFIED for actions (status-name guard); STATIC for incidents (unguarded update) |
| INV-W2 | `each_aggregate_has_exactly_one_writer_module` (arch-check rule) | RW-03 (best-effort scan), RW-04 | STATIC |
| INV-W3 | `token_rotation_on_a_proposed_action_goes_through_the_owner` | RW-04 | STATIC |
| INV-A1 | `audit_failure_leaves_the_action_row_unchanged` (audit store fails on append) | RW-03, RW-27 | STATIC |
| INV-R1 | `no_dispatched_action_is_dispatched_again_after_restart` (fake agent `runs == 0`) | RW-09, RW-30 | VERIFIED (mechanism and tests exist) |
| INV-R2 | `approved_action_runs_once_after_the_agent_reconnects` (link reports skills as unknown, not empty, while disconnected) | RW-09, RW-30 | STATIC (D36; not reproduced) |
| INV-R3 | `outcome_unknown_is_a_distinct_status_shown_to_the_operator` | RW-09 | STATIC |
| INV-R4 | `approval_older_than_its_ttl_or_for_a_resolved_incident_is_rejected_at_dispatch` | RW-08 | STATIC |
| INV-C1 | `admission_survives_restart` (hourly and monthly limits) | `04` section 16 (persisted `llm_admission`); no RW in 03 | STATIC |
| INV-C2 | `a_limit_declares_fail_open_or_fail_closed_when_its_history_is_unreadable` | `04` section 16; no RW in 03 | STATIC |
| INV-P1 | `cached_and_chat_proposals_pass_the_same_check_with_the_hosts_current_enabled_skills` | RW-06, RW-07, RW-08 | VERIFIED (#110) |
| INV-P2 | `a_failed_duplicate_lookup_stores_nothing` | no named decision; RW-03 supplies the mechanism | STATIC |
| INV-P3 | `approve_request_carries_the_displayed_plan_hash_and_a_mismatch_conflicts` | RW-08 | STATIC |
| INV-S1 | `editing_the_entry_script_after_approval_yields_plan_mismatch` (plan hash v2 includes entry bytes) | RW-08 | STATIC |
| INV-X1 | `a_disabled_capability_is_refused_by_every_consumer` (one test per consumer) | RW-07, RW-08 | VERIFIED for current consumers |
| INV-X2 | `the_raw_agent_link_is_unreachable_outside_the_policy_wrapper` | RW-08 covers callers of the execution function only; reads are not covered | STATIC |
| INV-X3 | `installing_a_capability_never_enables_it_for_a_host` | RW-26 | UNVERIFIED |
| INV-E1 | `agent_refuses_any_request_outside_its_local_catalogue_or_hash` (hostile hub fake) | RW-09, RW-12 | VERIFIED |
| INV-H1 | `a_request_without_a_host_gets_no_host_tools_and_an_offline_host_gets_none` | RW-20 | VERIFIED |
| INV-H2 | `no_mutation_derives_its_target_from_the_host_selector` | RW-20, RW-23 | VERIFIED |
| INV-D1 | `check_state_commits_only_after_the_spool_write` (failing spool, next run re-emits) | `04` section 15 durability fixes; no RW in 03 | STATIC |
| INV-D2 | `notification_state_records_sent_only_after_the_platform_accepted` | `04` section 15; RW-15 (`effects` outbox) | STATIC |
| INV-D3 | `unreadable_notification_state_is_an_error_not_default_state` | `04` section 15 | STATIC |
| INV-Q1 | `an_answer_for_a_cleared_conversation_is_rejected_with_a_typed_result` | RW-20 | STATIC |
| INV-Q2 | `a_failed_history_read_fails_the_request` | RW-20 | STATIC |
| INV-Q3 | `a_pending_question_is_keyed_by_request_id_not_by_text` | RW-20 | STATIC |
| INV-M1 | `every_stored_model_output_records_provider_and_model` | RW-06 | STATIC |
| INV-M2 | `call_deadline_enforced` (wall clock, independent of byte arrival) | RW-06 | STATIC |
| INV-M3 | `failed_call_records_usage` | RW-06 | STATIC |
| INV-M4 | `auth_and_missing_model_errors_are_classified_and_surfaced` | RW-06 | STATIC |
| INV-V1 | `a_reject_reason_is_sent_only_in_a_form_the_peers_version_can_decode` (matrix over versions) | RW-17 | VERIFIED |
| INV-V2 | `version_refusals_are_recorded_only_for_authenticated_peers` | RW-17 | VERIFIED |
| INV-U1 | `a_failed_read_returns_unknown_or_error_never_an_empty_value` (one injected failure per reader) | RW-09 covers agent availability only; otherwise no named decision | STATIC |
| INV-U2 | `removal_or_recovery_is_inferred_only_from_two_successful_observations` | no named decision; RW-14 holds facts and provenance | VERIFIED for host profiles |
| INV-K1 | `a_dismiss_that_stores_a_rule_never_leaves_the_incident_closed_without_it` (failing close) | no named decision | VERIFIED (rule first); STATIC for the audit-after-dismiss error |
| INV-K2 | `a_retry_limit_applies_only_to_its_own_reason` | RW-15 (typed events) | VERIFIED |
| INV-I1 | `everything_sent_to_a_provider_is_redacted_and_capped_at_send_time` (history included) | RW-06; `04` section 16 (prompt budget) | STATIC (follow-up replay is not re-redacted) |
| INV-I2 | `structured_output_is_parsed_whole_unknown_fields_rejected_and_count_capped` | RW-06, RW-07 | VERIFIED |
| INV-I3 | `one_admission_gate_admits_every_model_call_and_a_refusal_consumes_no_slot` | `04` section 16; no RW in 03 | VERIFIED (#112, test `a_question_refused_as_busy_takes_no_run_of_the_hour`) |
| INV-F1 | `every_mutation_call_site_is_wrapped_by_the_feedback_helper` (per call, not per file) | not carried by 03; `web/DESIGN.md` lint | STATIC |
| INV-F2 | `every_editable_form_blocks_navigation_while_dirty` | not carried by 03 | STATIC |
| INV-F3 | `a_failed_load_renders_a_load_error_never_an_empty_state` (page tests inject a failing request) | RW-21 covers presentation only; not a named rule | STATIC |

Added by the #112, #113 and verification pass:

| ID | Test name | Carried by | Evidence |
|---|---|---|---|
| INV-N1 | `provider_error_redacted` (every error string that reaches a user, chat reply or external log is redacted and clipped) | RW-06 | VERIFIED for the two question paths (#113); STATIC for other paths |
| INV-N2 | `cache_hit_uses_no_slot` | `04` section 16; no RW in 03 | STATIC |
| INV-N3 | `timeline_event_kinds_are_a_closed_type_not_parsed_strings` | RW-15 | STATIC |
| INV-N4 | `date_sensitive_tests_use_a_fixed_clock` (web and Rust) | AGENTS.md rule 11 for Rust; web not carried | UNVERIFIED |
