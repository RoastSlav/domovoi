# G. Lessons from closed pull requests: verification

Source index: `docs/design-pack/18-lessons-from-closed-pull-requests.md` (doc 18).
Method: read doc 18, read PR bodies and comments (`gh pr view`) for #36, #107, #109, #110, #112, #113, then read the current code. Nothing was executed; every verdict is from reading code and PR text. Paths are under `crates/` unless noted. Line numbers are at `64117f0` plus the dirty tree.

Verdicts: VERIFIED (PR says it, code still does it), PARTIALLY, NOT VERIFIED, WRONG.

## Corrections to doc 18 itself (read first)

- Doc 18 stops at #111. #112 (one admission gate) and #113 (redact provider failure text) are merged and missing. Total closed PRs is 113, not 110.
- Doc 18 says "#108: no closed PR in the reviewed result set". Wrong: #108 is `chore: release v0.0.10`, merged.
- Doc 18 §3 and the summary say approval "binds to the exact plan". The binding is to a manifest hash, not to script contents (L3 below).
- Doc 18 calls #36 "durability findings to evaluate against current main". They are still open on main (L9). That is the most actionable item in the document.
- Doc 18 §14 "enforced in CI" overstates two rules: the mutation-feedback lint is file-level, and unsaved-change guarding exists only in the notes editor (L16).

## Lessons verified against code

### L1. One writer owns every action transition; stale writes conflict
- Evidence: #107, #109. `domovoi-engine/src/actions/writer.rs:95` (`run` loop), `:132` (`process`), `:429` (`stale` maps `StoreError::Conflict` to `ChangedMeanwhile`). Guarded SQL `domovoi-store/src/repos/actions.rs:173-178` (`WHERE id = ? AND json_extract(status,'$.type') = ?`). `ActionRepo::update` is gone from the port; `update_from` is the only action write (`domovoi-core/src/ports/repos/actions.rs:48`). Executor never writes (`actions/executor.rs:219-295` only calls `ask_*`).
- Verdict: VERIFIED. Safeguard exists.
- Caveat A: the guard compares status name only, not a version or `updated_at`. Two writes from the same status (token rotation at `triage/proposals.rs:115` vs approve, both expect `proposed`) are ordered only because the writer is single. Rotation bypasses the writer: it calls `update_from` from the triage task. It cannot overwrite a decision (status guard) but it can overwrite another rotation or race an approve whose `clear_chat_token_hash` it then un-clears.
- Caveat B: the incident store has an unguarded `IncidentRepo::update` (`domovoi-core/src/ports/repos/incidents.rs:78`, used at `domovoi-engine/src/manager/actor.rs:259`). Single-writer there is by convention (ADR 0011), not by the store.
- Rule: INV-W1. For every aggregate, a stale write (expected state or version no longer matches) returns Conflict and changes nothing, tested with two concurrent commands. INV-W2. Exactly one module may write each aggregate; an arch-check rule fails the build otherwise. INV-W3. Rotating or editing any field of a `Proposed` action goes through the same owner as approve and deny.

### L2. Audit and state must not diverge (new facet of L1)
- Evidence: #109 says "the audit entry could describe a state the store did not hold". Fixed in one direction only: `actions/mod.rs:127-138` writes state, then `announce` (`:140-143`) appends audit and only logs on failure (`tracing::error!`).
- Verdict: PARTIALLY. State without audit is now possible. This matters because the loop-guard reload reads `dispatched` audit rows (`actions/recover.rs:139`), so a lost row undercounts after a restart.
- Rule: INV-A1. A state transition and its audit row commit in one transaction, or the transition is refused. Test with an audit store that fails on append: the action row stays unchanged.

### L3. Crash recovery: approved resumes, dispatched fails, never replay
- Evidence: #109, ADR 0014. `actions/recover.rs:27` (`RESTART_REASON`), `:204` (`fail_abandoned`), `actions/executor.rs:88` (`catch_up`, `MAX_CATCH_UP_RUNS`), `recover.rs:139` (`dispatch_history`). Tests `domovoi-engine/tests/services/recovery.rs:163,193,222,272`.
- Verdict: VERIFIED for the mechanism, with four gaps found in code.
- Gap 1 (likely real bug, not run): `AgentLink::skills` returns empty when the agent never connected in this session (`domovoi-hub/src/agent_server/registry.rs:263-269`, port doc `domovoi-core/src/ports/agent_link.rs:135`). `ActionService::spawn` runs at `domovoi-hub/src/runtime/mod.rs:230`, before the listeners serve. `catch_up` therefore runs with no agents connected, `revalidate` (`executor.rs:325`) finds no skill, and the approval is rejected with "the skill is no longer on the host". `wait_for_agent` (`:344`) is only reached when the skills list was non-empty. The recovery tests use `FakeAgentLink`, which keeps skills while connected, so they cannot see this.
- Gap 2: "outcome unknown" is a string in `Failed { reason }` (`domovoi-core/src/action/request.rs:34`), not a status. A link error or deadline during `run_skill` (`executor.rs:279-282`) is also recorded as plain `Failed`, though the command may have run.
- Gap 3: an `Approved` action has no age limit. Catch-up runs an approval made days earlier, with no check that the incident is still open or the check still firing (`executor.rs:219-262`).
- Gap 4: the loop-guard slot is claimed before `wait_for_agent`, so an offline-agent failure consumes a slot (`executor.rs:244`).
- Rule: INV-R1. After a restart, no action that reached Dispatched is dispatched again (assert `runs == 0` on the fake agent). INV-R2. Approved-before-restart actions run once after the agent reconnects, using a link that reports skills as unknown (not empty) while disconnected. INV-R3. Outcome-unknown is a distinct status shown to the operator. INV-R4. An approval older than a configured TTL, or whose incident resolved, is rejected at dispatch.

### L4. Safety counters rebuilt from durable history
- Evidence: #109 (`reserve_dispatch`, `executor.rs:297`, one lock for check and claim; reload at `recover.rs:139`).
- Verdict: VERIFIED for the action loop guard.
- Not applied to the triage hourly window: `triage/admission.rs:70-98` keeps `starts` in memory, so a restart resets the hourly model cap that #112 added. The monthly budget is durable (`usage.month_total`, `admission.rs:50`) but fails open on a read error (`:56-59`).
- Rule: INV-C1. Every rate or budget limit survives a restart (restart the service in a test and assert the count). INV-C2. State explicitly whether a limit fails open or closed when its history cannot be read.

### L5. Stale approval and cached proposals revalidated (#110)
- Evidence: #110 body and review comment. `triage/run.rs:88-93` (cache hit calls `proposals::create` with current `link.skills`), `triage/proposals.rs:48-60` (`check`: skill present, `applies_to`, args), `:115` (token rotation). Tests named in the PR exist in the engine suite.
- Verdict: VERIFIED. Current skills come from `PolicyLink`, so disabled skills drop too.
- Weaknesses: (a) the duplicate lookup is `Page::first(50)` and a listing error becomes an empty list (`proposals.rs:67-91`), so a store hiccup stores duplicates; (b) the cache is consulted after admission, so a cache hit spends an hourly slot (`run.rs:59` before `:88`), contrary to "the cache never calls the model"; (c) approve carries no plan hash (`domovoi-hub/src/api/actions.rs:104-123` takes only `confirm_text`), so the server cannot tell what the operator saw. It relies on the stored action being immutable.
- Rule: INV-P1. A cached or chat-origin proposal becomes an action only through the same `check` as a fresh one, with the host's current enabled skills. INV-P2. A failed duplicate lookup never stores a possibly duplicate action. INV-P3. The approve request carries the plan hash the UI displayed; a mismatch is a conflict.

### L5b. Approval binds the manifest, not the script (overstated in doc 18)
- Evidence: `domovoi-core/src/skill/manifest.rs:146` hashes the serialised manifest only. `domovoi-runner/src/catalog/checks.rs:100-111` (`recheck_entry`) re-checks folder mode and that the entry still resolves to the same path, not contents. Agent compares `PlanHash` at `domovoi-agent/src/requests/skills.rs:98`.
- Verdict: PARTIALLY (doc 18 §3 and #10). Editing `run` after approval executes under the approved hash.
- Rule: INV-S1. The plan hash includes a digest of the entry file's bytes, computed by the agent at run time; changing the file after approval yields PlanMismatch.

### L6. Capability policy on every consumer (#84)
- Evidence: `domovoi-hub/src/agent_server/skill_policy.rs:157-221` (`PolicyLink` filters `skills`, refuses `run_tool` read skills and `run_skill`), `:32-45` and `:93-107` (`updates` mutex serialises read-modify-write). Consumers all get `policy_link`: `domovoi-hub/src/runtime/mod.rs:216,220,235,269`; profile `profile/collect.rs:51,193`. Tests: `skill_policy.rs:283-420`, `domovoi-hub/tests/hub/profile.rs:264`.
- Verdict: VERIFIED.
- Weak points: (a) enforcement is a wrapper the consumer must be handed; `server.link()` stays public and `AgentAdmin::skills` (`api/mod.rs:118`) is deliberately unfiltered, so a new consumer wired to the raw link bypasses policy, and no arch-check rule prevents it; (b) `run_check` is not policy-filtered (`skill_policy.rs:186-195`); (c) the policy is hub-side only, the agent never learns of it; (d) it is keyed by host name (`host_skills_key`).
- Rule: INV-X1. A disabled capability is refused by every consumer (triage, follow-up, chat, profile, executor, scheduled jobs). One test per consumer. INV-X2. The raw link type is not constructible outside the policy wrapper (type-level or arch-check). INV-X3. Installing a capability never enables it for a host without an explicit switch.

### L7. Agent remains an enforcement boundary
- Evidence: `domovoi-agent/src/requests/skills.rs:61-125` (catalogue lookup, risk allowed, args, plan hash, all re-done locally); runner `domovoi-runner/src/exec/program.rs:10-32` (canonical path, shell refused, executable bit).
- Verdict: VERIFIED. See L5b for the content gap.
- Rule: INV-E1. The agent refuses any request that is not in its local catalogue or does not match its hash, whatever the hub sent (agent-side test with a hostile hub fake).

### L8. Host and global scope (#94, #95, #101)
- Evidence: `domovoi-hub/src/api/chat.rs:118-133` (`tool_host` is `None` for no host or an offline host), `domovoi-engine/src/triage/chat.rs:149-162` (no skills without a host; only read-risk skills offered), `agent.rs:130-175` (`read_tools()` offered whenever the provider supports tools; calls are scoped by `Scope`). Web: `web/src/features/chat/ChatPage.tsx` keys the conversation by host; selector `web/src/app/HostContext.tsx` is a view filter, and mutations take explicit ids.
- Verdict: VERIFIED.
- Notes: the selected host persists in `localStorage` across sessions (`HostContext.tsx:6-27`), so a new session starts host-scoped; the header tag says so. A deleted host name stays stored and gives a 404 from chat.
- Rule: INV-H1. A request with no host id gets no host tools; an offline host gets none (hub test exists in `tests/hub/api_chat.rs`). INV-H2. No mutation derives its target from the selector; the target is in the request body or URL.

### L9. Signal and notification durability (#13, #36 not merged, #50, #52)
- Evidence, agent side: ack only after hub store accepts (`domovoi-hub/src/agent_server/session.rs:90-105`); agent spool with bounds (`domovoi-agent/src/spool.rs:14-22`). On open, unreadable spool lines are skipped with a warning and a corrupt `spool.json` becomes `Meta::default()` (`spool.rs:66-95`).
- #36 finding still open: `scheduler.rs:333` applies `set_firing` in memory, `:340-343` logs a spool push failure, then `:344` saves check state anyway. A firing or resolved transition can be committed to state and lost from the spool, and the check will not re-emit it.
- Notification side: `domovoi-hub/src/runtime/delivery.rs:55-67` saves the plan's state (`first_alert_at`, `held`, `last_reminder_at`) before delivery; a delivery that fails after retries leaves the state saying it was sent (`:111-125`). `delivery.rs:242-248` turns a state read error into default state, so a store error can cause a duplicate card. `dispatcher.rs:207-212` persists a quiet-hours release before sending. Mitigations that do exist: per-incident ordering (`dispatcher.rs:104-118`), lag recovery from the store (`:143-153`, `:162-189`), and a card fallback when the summary fails (`:240-262`).
- Verdict: PARTIALLY. Ordering and lag recovery are good; ack-after-persist is good; the #36 ordering and retention bugs are still present. Doc 18 correctly says "not implemented" but should say "confirmed present".
- Rule: INV-D1. A check's state is not committed until its signals are durably spooled; a failing spool leaves the old state (agent test with a failing spool, then assert the next run re-emits). INV-D2. Notification state records "sent" only after the platform accepted it; after a failed delivery the next tick or recovery retries. INV-D3. An unreadable state is an error, never default state.

### L10. Chat races and stale responses (#91, #92)
- Evidence: generation check `domovoi-engine/src/manager/annotate.rs:132-150`, `triage/history.rs` (`count_clear`, `asked_at` ordering), `followup.rs:181-188`. Web: clear is disabled while sending (`web/src/components/ChatThread.tsx:234`), the answer shows from the response when the turn is not remembered (`web/src/features/incidents/IncidentChat.tsx:35-45`), history load failure is distinct from empty (`:127-129`). Tests: `incidentChat.test.tsx:291,306`.
- Verdict: VERIFIED for the incident chat. The general chat (#94/#95) has no server conversation at all: it is `sessionStorage` per host (`ChatPage.tsx:34-70`), so there is no generation to protect.
- Weak points: (a) when the generation check drops an answer, `answer()` still returns `Ok(answer)` (`followup.rs:~308`), so a client can show a reply to a conversation another tab cleared; (b) a failed timeline read captures generation 0 (`followup.rs:182-188`), which wrongly drops the answer if any earlier clear exists; (c) the client decides a turn "landed" by matching question text (`IncidentChat.tsx:38`), so a repeated question can match an older turn; (d) tool use from a failed follow-up is not recorded (`followup.rs:235` returns before `record_tool_use`).
- Rule: INV-Q1. An answer whose conversation generation moved is rejected with a typed "conversation was cleared" result, not 200. INV-Q2. A failed history read fails the request, it does not guess a generation. INV-Q3. The client keys a pending question by a request id, not by text.

### L11. Model fallback and attribution (#89, #90, #71)
- Evidence: `triage/run.rs:~180-206` (`providers` loop, `provider_name`), reasons joined with provider and model; `followup.rs:69-75` and #113 redaction and 500-char clip, tests in `tests/services/redact.rs`. Idle timeout only: `domovoi-llm/src/http.rs:20-30` (`read_timeout`, restarts on each byte).
- Verdict: PARTIALLY.
- Failure attribution VERIFIED. Success attribution NOT present: `TriageReport` (`domovoi-core/src/triage/report.rs:52-76`) has no provider or model field, and `usage.record` stores only `provider.id()` (`triage/agent.rs:110-120`). A failed call records no usage (`provider.complete(req).await?` returns first).
- Every error falls through to the next provider identically (no classification, no breaker). The only whole-call deadline is `max_seconds`, checked between turns (`agent.rs:146-148`), so one streaming call can run unbounded while bytes keep arriving.
- Rule: INV-M1. Every stored model output (report, follow-up, chat) records the provider id and model that produced it. INV-M2. Each call has a hard wall-clock deadline independent of byte arrival. INV-M3. Usage is recorded for failed or partial calls if they report cost. INV-M4. Auth and missing-model errors are classified and surfaced, not only joined as text.

### L12. Protocol compatibility (#9, #86)
- Evidence: `domovoi-proto/src/version.rs:17-26` (`compatible`, one-minor window), `:107` (`at_least`), `domovoi-hub/src/agent_server/handshake.rs:140-148` (`refusal_reason` gives old agents the legacy `Incompatible` reason), `:194-215` (refusal recorded only after the signature check), frame cap `domovoi-proto/src/envelope.rs:11`.
- Verdict: VERIFIED.
- Note: `at_least` lets unparseable versions pass (`version.rs:104-108`), so an agent that reports `dev` skips the floor. Acceptable for authenticated agents; the rework should say so.
- Rule: INV-V1. A reject reason is sent only in a form the peer's version can decode (matrix test over versions). INV-V2. Version refusals are recorded only for authenticated peers.

### L13. Missing versus unavailable data (#82, #73)
- Evidence: `domovoi-hub/src/profile/diff.rs:75` (`collected`) and `:26-31` (compare only when both sides answered), tests `tests/hub/profile.rs:541,566`; `examples/skills/power_status/run` distinguishes "upsc could not list" from "no UPS detected"; `hardware_health/run:53` uses `smartctl -n standby`.
- Verdict: VERIFIED for those two paths.
- Same lesson broken elsewhere: (a) offline looks like "no skills" (L3 gap 1; triage proposals on an offline host drop as "not a skill on this host", `proposals.rs:48-52`); (b) `delivery.rs:242-248` (L9); (c) `proposals.rs:87` (L5); (d) `triage/run.rs:149` turns a cache read error into a miss (safe) and `admission.rs:56` turns a budget read error into "not spent" (fail open).
- Rule: INV-U1. Every read that can fail returns `Unknown` or `Err`, never an empty value, and a test per reader injects the failure. INV-U2. Removal or recovery is inferred only from two successful observations.

### L14. Suppression before closure (#66) and retry budgets (#59, #60)
- Evidence: `domovoi-hub/src/api/incidents.rs:343-398` (rule stored first, then `manager.dismiss`, then rule audit). Retry reason enum `triage/mod.rs:180`, per-reason limit `triage/queue.rs:206-222`, startup `reload` from the durable timeline (`queue.rs:~225`).
- Verdict: VERIFIED.
- Note: the rule audit runs after dismiss and its failure returns an error for an already-done dismiss (`incidents.rs:~395`). Retry state is in memory but is rebuilt from timeline kinds, which are plain strings (`triage.skipped`, `triage.failed`).
- Rule: INV-K1. A dismiss that stores a rule never leaves the incident closed without the rule (rule first, tested with a failing close). INV-K2. A retry limit applies only to its own reason.

### L15. Untrusted model text (#93) and bounded model input (#94, #95, #112, #113)
- Evidence: `triage/followup.rs:116-142` (one complete JSON array after the last `PROPOSALS:` line, `serde_json::from_str(...).unwrap_or_default()`), `ActionProposal` has `deny_unknown_fields` (`domovoi-core/src/triage/report.rs:37`), proposal cap and per-line clip (`followup.rs:16-24,242-249`). Chat bounds `triage/chat.rs:20-35` (overview, history, question, turn, catalogue bytes) with per-turn redaction (`chat.rs:60-67`). Admission `admission.rs:70-98`, semaphore in `followup.rs:152`, `chat.rs:~140`.
- Verdict: VERIFIED.
- Difference: the incident follow-up replays stored turns without re-redacting (`followup.rs:~84-107`), relying on redaction at write time; the general chat redacts every replayed turn. Both are bounded by turn count and size only in chars for follow-up and bytes for chat.
- Rule: INV-I1. Anything sent to a provider (history included) passes the current redactor and a size cap at send time. INV-I2. Structured output is parsed whole, unknown fields rejected, count capped. INV-I3. One gate admits every model call, a refused request consumes no slot (tests `a_question_refused_as_busy_takes_no_run_of_the_hour`).

### L16. Mutation feedback and error states (#100 to #105)
- Evidence: rules `web/eslint/domovoi-rules.js:7-15` and DESIGN.md "Checks that will fail your PR". Undo with expiry `web/src/features/rules/DeleteRuleDialog.tsx`, test `rules.test.tsx:369`. Leave guard `web/src/features/notes/useLeaveGuard.tsx:16`.
- Verdict: PARTIALLY.
- The feedback rule passes a file if it imports `useActionFeedback`, `InlineMessage` or `InlineError` anywhere (`domovoi-rules.js:10,128`), so one reported mutation excuses another unreported one in the same file. `ChatPage.tsx:56` uses `mutateAsync` and reports through the thread, with a failure view and kept input (test `chat.test.tsx:108`).
- Unsaved-change blocking exists only in the notes editor (`useLeaveGuard` is imported by `NoteEditor.tsx` only), though doc 18 lists it under settings.
- Rule: INV-F1. Every mutation hook call site is wrapped by the feedback helper (check per call, not per file). INV-F2. Every editable form blocks navigation while dirty. INV-F3. A failed load renders a load error, never an empty state (page tests inject a failing request).

### L17. Other lessons spot-checked
- #96 release tag allowlist: `domovoi-proto/src/version.rs:97-103` (`is_release_tag`), used in `domovoi/src/cmd/upgrade/fetch.rs:48`. VERIFIED. #97 checksum: `upgrade/package.rs:13-30` compares a checksum from the same release origin, so it detects corruption, not a hostile release (overstated as "verified").
- #87/#88 logs: ring capacity 2000, redacted at capture and again in the API (`domovoi/src/log_ring.rs:15-75`, `domovoi-hub/src/api/logs.rs:54`), agent cap 2000 lines (`domovoi-agent/src/requests/mod.rs:43`). VERIFIED.
- #69 tool use outcomes: `triage/agent.rs:20-33` has `failed` and `refused` flags, names validated against `ReadTool::ALL`. VERIFIED (two booleans, no skipped or cancelled).
- #22 approvals human-only: type split in `domovoi-core/src/action/transition.rs:65-90` (destructive needs typed confirmation on web). VERIFIED.
- NOT VERIFIED: #67 SSH login window defaults; #53 Discord placeholder approver; #50/#52 delivery text; #19 webhook signing; #26 redaction coverage; #72 runtime detection; #81 profile fence escaping.

## Lessons the document missed (add to the rework)

1. Real agent connectivity is part of recovery (L3 gap 1). Port semantics "empty means never connected" collide with "not connected yet". Use a three-state capability view: Known, Unknown, Absent.
2. Audit and state need one transaction (L2).
3. Admission placement and persistence (#112): the gate runs before the cache, its window is memory-only, the budget read fails open. Add: cache hits cost no model slot; limits are durable; document fail-open or fail-closed per limit.
4. Provider failure text is a leak channel (#113). Every error string that reaches a user, a log shipped elsewhere or a chat reply passes the redactor and a length cap. Doc 18 covers history redaction but not error redaction.
5. Approvals have no age. A restart can run a week-old approval (L3 gap 3).
6. Plan hash covers the manifest only (L5b).
7. Capability policy by wrapper is a convention (L6). Make the raw link unreachable.
8. Releases and docs: `release-plz` PRs (#38 closed unmerged, then merged per-version PRs) show the release path is a PR stream; the doc lumps these as noise. They matter only for the compatibility floor constants in `domovoi-proto/src/version.rs:35-69`, which must move with each breaking release.
9. Generated-file drift: `.github/workflows/ci.yml` classifies `openapi` changes, but AGENTS.md says `cargo xtask ci` leaves out `gen:api` and `format:check`. #36 asked for a local check; the gap is documented, not closed.
10. Date-sensitive tests: #107's comment records a web test that failed because fixtures were relative to the real clock. Rework tests must use fixed clocks in web tests too.
11. Timeline kinds are stringly typed and carry state (`triage.skipped`, `triage.failed`, `followup.cleared`, `triage.followup`). `queue.rs:~190-222` and `history.rs` parse prefixes to decide retries and conversation boundaries. Give them types.
12. Tool-use records are lost when a follow-up fails (L10 d), so failed attempts leave no evidence of what the model read.

## Lessons in doc 18 that are wrong or overstated

- "All 110 closed PRs" and "#108 not closed": wrong (see top).
- §1 "single-writer ... guaranteed": true for actions; incidents rely on convention (L1 caveat B), and token rotation writes outside the writer.
- §2 "Do not blindly replay ... failing everything pending loses approvals": the code resumes `Approved`, but that path is probably broken on a real hub (L3 gap 1), so the claim is stronger than the evidence.
- §3 "approval must bind to the exact plan": binds manifest, not script (L5b); the approve request carries no plan reference (L5c).
- §5 "Fallback model identity must be recorded in experience history": a target rule, but doc 18 presents #89/#90 as establishing it. Only failure text carries attribution (L11).
- §6 "retain notification state until delivery succeeds": accurate as a lesson, but not implemented anywhere (L9).
- §9 "Warn before navigating away from unsaved work" and "#104 unsaved-change warnings": only the notes editor (L16).
- §14 "enforced in CI": the mutation-feedback and disabled-reason lints are structural file-level checks (L16).
- §18 "checksum-verified": integrity from the same origin, not authenticity (L17).
- Item 11 of the comment section, "#91 ... apply to any resettable context": the general chat has no server conversation at all (L10).
- "Hardware diagnostics must not ... " and "Network-security defaults" are carried from PR text; only #73 was checked (L13, L17).

## Rework invariants, as one checklist

W1 stale write conflicts; W2 one owner per aggregate enforced by arch-check; W3 token rotation through the owner; A1 state and audit atomic; R1 no redispatch after restart; R2 approved survives restart with unknown-skills link; R3 outcome-unknown status; R4 approval TTL and incident-open check; C1 limits survive restart; C2 limits declare fail-open or closed; P1 all proposals go through one `check`; P2 failed duplicate lookup stores nothing; P3 approve carries plan hash; S1 plan hash includes entry bytes; X1 disabled capability refused by every consumer; X2 raw link not reachable; E1 agent enforces locally; H1 no host id means no host tools; H2 no mutation target from selector; D1 check state commits only after spool write; D2 "sent" recorded after acceptance; D3 unreadable state is an error; Q1 stale conversation answers are rejected typed; Q2 failed history read fails the request; Q3 pending request keyed by id; M1 provider and model recorded on every output; M2 hard call deadline; M3 failed-call usage; M4 classified provider errors; V1 reject reasons decodable by peer; V2 refusals only for authenticated peers; U1 reads return Unknown or Err, never empty; U2 removal needs two good observations; K1 rule before close; K2 retry limits per reason; I1 redact and cap everything sent to a provider; I2 structured output parsed whole; I3 one admission gate; F1 feedback per mutation call; F2 dirty forms block navigation; F3 load failures never render as empty.

## Closed and unmerged PRs worth keeping on the list

- #36: see L9. Still the best checklist of durability bugs.
- #38: unmerged v0.1.0 release PR, no product signal.
- #1 to #4, #14: dependency bumps. #3 and #4 (typescript 7, @types/node 26) were refused; worth knowing before any toolchain bump.
