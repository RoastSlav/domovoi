# Lessons from Domovoi's Closed Pull Requests

**Repository reviewed:** `RoastSlav/domovoi`  
**Review date:** 2026-11-12  
**Scope:** all 110 closed pull requests returned by GitHub for this repository at review time (PRs #1–#111, excluding numbers that were not closed PRs). Titles and descriptions were reviewed across the full set. Conversation comments were checked where present; the substantive maintainer follow-up comments on #107 and #110 were included. Code changes were inspected selectively where the description exposed concurrency, security, persistence, protocol, UI-system or operational-recovery risks. Routine dependency-only and release-only changes were classified but not treated as product-design evidence.

This document is a lessons-learned record, not a requirement to preserve the current implementation. The rework may replace mechanisms, but should not repeat known failure modes or discard proven product requirements without a reason.

## Executive summary

The repository's history contains several important lessons for the target design:

1. **Concurrency and durable state need explicit ownership.** Action decisions and action transitions were vulnerable to races until a single-writer model and guarded updates were introduced (#107, #109). Cached proposals also needed to be revalidated and converted back into live actions (#110).
2. **Restart recovery is part of correctness, not an operational afterthought.** Do not blindly replay actions whose execution outcome is unknown. Reconstruct rate limits and pending state from durable history; distinguish approved-but-not-started from dispatched-but-uncertain work (#109).
3. **An AI answer, cached report, notification or UI card is not the source of truth.** Revalidate against current capabilities, policy and target state at the point of action. UI affordances must reflect the current action token/state (#93, #107, #110).
4. **The UI needs explicit state design and quality gates.** Loading, empty, no-match, offline, partial failure, retry, unsaved changes, pending mutation and mutation failure are distinct states, not variations of an empty page (#100–#105, #111).
5. **A design system must be enforceable.** Semantic tokens, centralized status vocabulary, accessible components, contrast tests, axe checks, mutation feedback and bundle budgets prevent visual and interaction drift (#98, #100, #105).
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

**Apply to the rework:** incidents, operations, approvals, autonomy decisions, capability installation, agent enrollment, and long-running orchestration should each have explicit transition ownership. Do not automatically put all domains in one global writer; use the smallest ownership boundary that guarantees serializable decisions.

### 2. Crash recovery must distinguish safe resume from unknown execution

**Evidence:** #109 recovers approved actions once at startup, fails actions still marked dispatched with an explicit “check the host before running it again” reason, and rebuilds the hourly dispatch guard from audit history. #36 (closed without merge) proposed durable spool/notification hardening and corruption handling.

**Failure mode:** a process may crash after an external command started but before its result was persisted. Replaying it can duplicate a destructive or non-idempotent action. Resetting an in-memory rate limiter at restart can bypass safety limits. Conversely, failing every pending item loses approvals that were durable but not yet dispatched.

**Target rule:** persist operation checkpoints and action lifecycle transitions. On restart, classify work as safe-to-resume, safe-to-retry, needs-reconciliation, or outcome-unknown. Never replay outcome-unknown external actions automatically unless the operation is explicitly idempotent and its idempotency contract is proven. Reconstruct safety counters from durable events. Surface reconciliation needs in the UI.

**Design consequence:** operation execution requires durable step identity, idempotency classification, start/result records, deadlines, cancellation semantics and recovery behavior. A command's exit code alone is not proof of the external system's final state.

### 3. Revalidate at every trust boundary

**Evidence:** #6 validates skill manifests and typed arguments; #10 rechecks executable paths and permissions immediately before running; #13 makes the agent revalidate incoming requests locally; #22 revalidates before dispatch; #93 constrains model-proposed skills and arguments; #110 revalidates cached proposals against current skills.

**Failure mode:** a capability may have been removed or changed since a model generated a proposal. A cached report may be stale. A script path may have been replaced after catalogue loading. A plan may change between approval and execution.

**Target rule:** treat LLM output, cache entries, chat buttons, hub requests and UI state as untrusted or potentially stale. Revalidate capability identity/version, target, typed arguments, current policy, plan hash and local host constraints immediately before execution. Approval must bind to the exact plan that was reviewed. Any material change invalidates approval and requires a new decision.

**Design consequence:** a plan is a versioned immutable object or snapshot; the execution request references that exact plan. The UI should clearly show when a plan changed and why previous approval no longer applies.

### 4. Keep policy, knowledge, and experience separate

**Evidence:** #22 and #107 show the need for explicit approval and guarded state transitions; #81/#82 add host profile context; #66/#63 add known-condition rules; #91 stores conversation history; #110 shows cached triage output must not itself authorize a new action.

**Lesson:** preferences, profiles, notes, learned patterns, historical approvals and model conclusions are useful context, but none should silently grant authority. A prior successful action is not permission to repeat it. A known condition is not the same as an autonomy policy.

**Target rule:** maintain distinct stores and APIs for environment knowledge, authorization policy and operational experience. Policy evaluation is deterministic and separately auditable. Knowledge can explain why an action is useful, but cannot make it allowed.

### 5. Model failure is a normal control-flow path

**Evidence:** #54 increased API timeouts for slow local models; #71 added streaming and configurable timeout; #89 introduced provider fallback; #90 added ordered model fallback chains; #94 handles providers without tool calling by gathering read output before asking the model.

**Target rule:** model calls need explicit deadlines, cancellation, streaming, provider/model attribution, usage accounting, retry classification and fallback policy. Distinguish transport failure, timeout, rate limit, authentication failure, missing model, invalid structured output and tool-call incompatibility. Do not retry every error identically.

**Design consequence:** an operation can remain in a visible “waiting for model” or “provider unavailable” state. The UI must not imply that reasoning is progressing when the provider is stalled. Fallback model identity must be recorded in experience history.

### 6. Durable delivery and offline behavior

**Evidence:** #13 implements a signal spool; #36 proposes making check-state commits contingent on durable spool writes, validating/repairing corrupted spool state, retaining notification state until delivery succeeds and preserving scheduler errors. #50/#52 address Discord delivery and quiet-hour behavior. #87/#88 add bounded log retrieval over the agent link.

**Target rule:** distinguish “created”, “queued”, “sent”, “acknowledged”, “failed” and “expired” for messages and events. Persist work before acknowledging it. Use bounded retries with backoff and dead-letter/error visibility. Do not discard state merely because delivery failed. Offline agent data must be bounded and have explicit retention.

**Design consequence:** the hub/agent link, operation event stream, notifications and emergency local autonomy need durable delivery semantics and reconciliation after reconnect.

### 7. Protocol and deployment compatibility are product behavior

**Evidence:** #9 defines a typed, versioned protocol with frame bounds and explicit unknown-message handling; #86 adds minimum supported release checks with a human-readable reason; #72 detects Docker/Podman; #34 documents/handles unavailable host checks in container mode.

**Target rule:** protocol evolution must be additive where possible, bounded, explicitly versioned and testable against compatibility matrices. Unsupported operations should return a typed explanation. Do not let incompatibility turn into an endless reconnect loop. Detect runtime capabilities rather than assuming the installation mode.

**Design consequence:** UI should show hub/agent versions, compatibility state, runtime type, missing host permissions and unsupported capabilities with actionable remediation.

## Lessons that directly affect the UI plan

### 8. Page and component states must be designed, not improvised

**Evidence:** #100 established a component kit; #103 explicitly added loading, empty, error and offline states; #104 distinguishes no notes from no search matches, adds retry, unsaved-change warnings and mutation outcomes; #105 adds disabled reasons and mutation feedback; #111 refines overview, incident, host and settings hierarchy.

**Target rule:** every page, tab, table, graph, form and async action must specify at least: initial/loading, loaded, empty, no-results, partial-data, stale-data, offline, permission-denied, error/retry, submitting, success and failure states where relevant. “Empty” must never be used to conceal a failed request.

**UI specification addition:** page specs must include state matrices and transitions, not only static content and happy-path interactions.

### 9. Mutations need clear feedback, reversibility and protection against accidental loss

**Evidence:** #104 adds a single save bar, unsaved counts, discard, field-level errors, connection-test feedback, typed host-name revocation, delete confirmation and Undo with exact expiry restoration. #103 reports mutation results and requires typed confirmation for agent revocation. #105 bans mutations without feedback except documented exceptions.

**Target rule:** every mutation must tell the user what changed, whether it succeeded, and what remains pending. Destructive or hard-to-reverse actions require clear consequence previews and confirmation appropriate to risk. Prefer Undo for reversible changes. Preserve user input on failure. Warn before navigating away from unsaved work.

**UI specification addition:** each action definition includes preconditions, confirmation level, pending state, success state, failure state, retry behavior and undo/rollback availability.

### 10. Context should follow the user, but global scope must remain explicit

**Evidence:** #64/#101 introduce global host selection and a selector that follows page navigation; #94 gives host-scoped chat read tools only when a host is selected and no tools for an all-hosts conversation; #103 makes host tabs deep-linkable through query parameters.

**Target rule:** selected host/environment context should persist across related pages and be visible. Global/all-hosts scope must be explicit and must not silently inherit a previous host's tools or permissions. Deep links should preserve meaningful tabs/filters and be shareable/bookmarkable where safe.

**Design consequence:** global Operator context, task scope, incident scope and capability target must be separately visible. Never infer a destructive target only from a hidden global selector.

### 11. Show what the AI actually used and preserve the conversation

**Evidence:** #69 exposes skills/tools used on incident details; #79 adds a manual-steps checklist; #81/#82 provide host profile context; #91 stores follow-up conversation with generation/race protection and chronological ordering; #92 shows the conversation, pending state and clear confirmation; #93 allows structured action proposals; #94/#95 create general host chat.

**Target rule:** the user should be able to inspect evidence, tools/capabilities invoked, manual steps, model answers and action proposals as separate structured elements. Preserve conversation context across turns, but make clearing/reset behavior explicit. A request still in flight must not be silently erased or resurrected by a late response.

**Design consequence:** Operator workspace should be built around a durable task/conversation identity, with linked orchestration, evidence and action history. Chat is not just a transient message list.

### 12. Live views need bounded refresh, useful controls and performance budgets

**Evidence:** #87/#88 bound in-memory logs, redact before storage and again at the API/agent boundary, and fetch remote logs in chunks. #103 adds live toggle, search/highlighting, jump-to-latest, wrapping, copy and show-older; polls host list only while waiting for enrollment. #105 memoizes large tables and sets bundle/CSS budgets.

**Target rule:** real-time views need explicit live/pause state, bounded windows, pagination/virtualization, search, copy/export and clear freshness indicators. Poll only while a user-visible condition requires it; prefer event streams where appropriate. Keep the interface responsive under large histories and frequent updates.

**Design consequence:** orchestration graph and event timeline need incremental updates, bounded rendering and stable selection/scroll behavior. New events must not unexpectedly move the user's current inspection target.

### 13. Technical language should be explained, not erased

**Evidence:** #65 adds explanations for pages and jargon; #111 keeps detailed evidence and operational labels while improving hierarchy.

**Target rule:** retain precise terminology (orchestration, capability, policy, verification) and pair it with concise explanations. Do not dumb down the detailed view. Use configurable information density and progressive disclosure, with the detailed experience as the reference design.

### 14. A design system needs automated enforcement

**Evidence:** #98 centralizes design tokens, themes and status tones; #100 creates shared feedback/hint/state/form components; #105 adds WCAG AA contrast checks, axe checks, voice/tone lint, mutation-feedback rules, disabled-reason requirements, bundle-size budgets and memoization checks.

**Target rule:** define semantic tokens and shared component contracts, then enforce them in CI. Avoid raw colors and one-off status interpretations. Components should expose accessible names, focus behavior, disabled reasons and consistent feedback. Accessibility and performance are acceptance criteria, not final polish.

**Important nuance:** shared components should support consistent behavior without forcing every page into the same layout. Reuse the design language and interaction contracts; don't let the component kit dictate the information architecture.

## Capability, onboarding and operations lessons

### 15. Capability lifecycle needs more than a list of scripts

**Evidence:** #83 exposes skill README, limits and recent runs; #84 allows enable/disable per host; #103 shows risk, timeout, applies-to and last-used; #44 installs starter skills by default and suggests restart after upgrade.

**Target rule:** future capability management should distinguish discover, inspect, compatibility, install, enable, configure, authorize, update, disable, uninstall and audit. These are different operations. Installing a capability must not grant it permission. Per-agent/host compatibility and enablement must be explicit.

### 16. Host profiles and environment discovery are valuable, but provenance matters

**Evidence:** #81 collects and stores host profile; #82 injects profile into triage/chat/notes; #67 adds expected SSH logins and a login time window; #72 detects runtime type.

**Target rule:** use discovered host facts to tailor questions and diagnostics, but show source, freshness and uncertainty. Auto-discovered information must not silently become user-confirmed constraints or permissions. Profiles should be editable and linked to the evidence that produced them.

### 17. Known conditions should be understandable and scoped

**Evidence:** #63 provides UI management with plain-language condition rules; #66 supports dismissing an incident as a known condition; #104 shows expiry, creator, readable phrases, operator-specific inputs and Undo.

**Target rule:** known conditions are explicit, scoped, time-bounded when appropriate and reviewable. The UI should explain exactly what will be suppressed and until when. A known condition suppresses expected noise; it must not conceal a security event or authorize a remediation action.

### 18. Installation and upgrade flows need operational safety

**Evidence:** #33 deployment methods; #32 guided first run and UI-managed settings; #43 LAN/integration setup; #44 starter capabilities and restart guidance; #96 safe release discovery; #97 checksum-verified in-place upgrade that does not silently restart services.

**Target rule:** setup and upgrade flows must explain prerequisites, what will change, how to recover and whether a restart is required. Pin versions, verify downloaded artifacts and never execute untrusted release metadata as shell input. Separate downloading/replacing a binary from restarting services.

## Unmerged closed PRs: useful signals, not current behavior

Seven closed PRs were not merged: #1–#4 and #14 are dependency updates; #36 is a substantive hardening/audit PR; #38 is an attempted `v0.1.0` release PR. Do not assume their changes are in `main`.

- **#36 — signal and notification delivery hardening (not merged):** proposed durable-spool ordering, corrupted-spool repair/rejection, notification retention until delivery succeeds, repair of missing discussion threads, digest paging, scheduler error visibility, generated API artifact checks and documentation/link audits. These are valuable review findings to evaluate independently against current `main`; do not treat them as implemented.
- **#38 — release `v0.1.0` (not merged):** release-plz generated a release PR that was closed. It is historical release automation, not evidence that a `v0.1.0` release shipped.
- **#1–#4 and #14:** dependency bumps closed without merge. No product behavior should be inferred from them.

## PR groups reviewed

The following index records the complete closed-PR set and the primary topic inferred from each title/description. Routine dependency and release PRs are included so the audit scope is explicit.

| PRs | Primary topic / lesson |
|---|---|
| #111 | Current UI overhaul: operator-oriented hierarchy, dense evidence, host views, responsive shell. |
| #110 | Revalidate cached proposals; deduplicate actions; rotate stale chat approval tokens. |
| #109 | Single writer for all action transitions; crash recovery; rebuild rate limits from audit. |
| #108 | No closed PR in the reviewed result set. |
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
| #97 | Checksum-verified in-place binary upgrade; no implicit restart. |
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

## What this review does not decide

- Whether to retain Rust, SQLite, the current agent protocol or any current UI component. The PR history supplies evidence, not a veto on the target architecture.
- The exact future operation/incident schema, workflow engine, graph store, capability package format or policy language. Those remain design tasks in the relevant pack documents.
- Whether every #36 hardening proposal should be merged into the future design. Its durability findings should be checked against the current code and tests before implementation.

## Additional lessons found in PR review comments

The PR descriptions often summarize the intended change; the maintainer's follow-up comments record edge cases discovered during review. These are particularly useful because they show where an apparently complete feature was still unsafe or misleading.

- **Never treat malformed or quoted model text as structured intent.** #93 found that extracting the first `[` through the last `]` could turn quoted prose into an action proposal. Parse a complete, explicitly delimited structured payload; reject surrounding prose and unknown fields. Prefer provider-native structured output when available. Also enforce proposal count/size limits, deduplicate within a batch, and address cross-request uniqueness at the store/serialization boundary.
- **Bound every model input, not only the visible prompt.** #94/#95 found that historical turns and skill catalogues could exceed budgets and that historical user/assistant messages could carry secrets. Redact every historical turn before sending it to a provider; cap each turn, total history, question, catalogue and host overview; drop oldest history deliberately and preserve complete question/answer pairs where possible. A global chat without a selected host should not receive host tools; offline hosts should receive no tools.
- **A chat must never lose a successful answer or user input.** #92 found a race where the response could arrive before history refreshed, leaving “Thinking…” stuck or making the answer disappear. Pending state should follow the request mutation; render the answer from the response if persistence lags; clear the composer only on success; retain input on failure; distinguish history-load failure from an empty conversation; block clear while a request is active and prevent a stale answer from reappearing after clear. Enforce the server's character limit in the client too.
- **Conversation clear needs generation semantics.** #91 prevents a slow in-flight answer from restoring a conversation after the user clears it. Requests capture a conversation generation and asking time; persistence rejects stale generations and history replays in asking order even if responses finish out of order. Apply the same pattern to any resettable long-running UI or task context.
- **Provider fallback must preserve diagnostic information.** #89 found that fallback could hide the primary provider's failure and that invalid model output could omit provider identity. Aggregate provider-specific failure reasons and include provider/model attribution in diagnostics and operation history.
- **Never trust release metadata just because it parses as a version.** #96 found tag-injection risk: a parser that stops at a prerelease/build separator can accept a version prefix while ignoring unsafe suffix characters. Validate the entire release tag against a strict allowlist before using it in URLs, UI or copyable shell commands. Pin the exact image version in generated Docker upgrade commands. Bound HTTP response bodies at the shared HTTP layer, not just one call site.
- **Protocol additions must be understood by old peers.** #86 found that sending a newly introduced rejection reason to an old agent could leave it in a reconnect loop. Only send a reason the peer version can understand; otherwise use a compatible terminal rejection. Authenticate the peer before recording a version refusal, so unauthenticated traffic cannot create misleading host events.
- **A disabled capability must be disabled on every path.** #84 found host-profile collection bypassing the skill policy wrapper even though triage and action execution used it. All consumers (profile collection, chat, incident investigation, scheduled jobs, etc.) must use the same policy-enforcing capability access layer. Concurrent per-host enable/disable updates need serialized read-modify-write behavior so one update cannot silently erase another.
- **Missing data must not be interpreted as deletion or recovery.** #82 found that a failed container or listening-port query could make the next profile claim all previous entities had disappeared. Compare snapshots only when both sides contain valid data; represent collection failure/unknown separately from an empty successful result. Do not infer removals from missing observations.
- **Generated host profiles need explicit limits and honest semantics.** #81 found read skills with required parameters being called with empty arguments, generated profile notes overwriting prior content, tool output breaking Markdown fences, profile refresh being synchronous/expensive, and editable generated notes being ambiguous. Skip skills that cannot be called safely without parameters; escape/fence arbitrary tool output; label generated data as a latest snapshot rather than a history if it is overwritten; decide whether generated records are editable; bound refresh work and add backoff for failed sections.
- **Tool-use history must distinguish not-run from failed.** #69 found invented model-supplied tool names, budget-refused calls counted as failures, missing prefetched calls for providers without tool support, and identical names across tool/skill categories being merged. Validate names against registries, record category and run identity, and use distinct outcomes such as succeeded, failed, refused/not-run, skipped and cancelled. Record partial/in-flight runs if the UI is expected to show them.
- **Network-security defaults must preserve access.** #67 found that a login-allowance window could accidentally suppress all SSH logins and that one bare IP address was rejected. Default an “expected login” window to LAN-only, require explicit opt-in for internet-origin logins, confirm before creating broad rules, support /32 and /128 single-address semantics, and derive active-window UI state from persisted rules so reloads do not lie.
- **Create suppression rules before closing the incident they suppress.** #66 found a race where a new signal could arrive between dismissing an incident and storing its known-condition rule. Validate and persist the rule first, then close/dismiss; if the later step fails, leave the rule visible and explain it. The UI must state that a rule can cover all severities for that check/target and must not offer dismissal while triage is still pending.
- **Retry budgets need typed reasons.** #60 found that cap-deferred retries and failure retries were being conflated, so an old failure chain could block a later retry that was only deferred by a concurrency cap. Track why a run was deferred and apply each limit only to the retry class it governs. Test mixed timelines and slot release behavior.
- **Do not let optional integration setup block a connection test for the wrong reason.** #53 found that a Discord test could fail validation because no approver list had yet been saved, even though the test itself should be able to reach Discord. Use narrowly scoped test-only placeholders where appropriate, explain their scope and ensure they are not persisted as real authorization.
- **Hardware diagnostics must not create side effects or leak identifiers.** #73 found that SMART health checks should not wake sleeping disks, serial numbers should be filtered before model/timeline exposure, and UPS command failures must not be rendered as “no UPS detected.” Use non-waking commands, bounded execution time and precise “unavailable/failed/not detected” distinctions.
- **Review findings should be classified, not silently ignored.** Several comments explicitly left non-blocking items as separate design decisions (for example, profile refresh concurrency, generated-note editability, per-section size caps, API body bounds and cross-request proposal uniqueness). Preserve these as open design questions when the affected feature is selected; do not assume “reviewed” means “resolved.”

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
