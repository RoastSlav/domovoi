# 02 — Remaining work map (waves E–M)

Planning only. This map derives every ID, title, wave and dependency from the canonical roadmap and the
current prompt index; it invents no ID and no wave. It does not edit `06`, `07`, the prompt files, the
source, the migrations or `docs/design-pack/`, and it contains no implementation brief.

Authority order used: `docs/rework/07-implementation-prompt-index.md` (IDs, §2 table, §3 wave order,
§4 contracts, §6 file status, §7 migrations) → `06-implementation-roadmap.md` (milestone objectives and
named tests) → `03`/`04`/`04b` (interfaces, tables, invariants) → `05` (migration and protocol rules) →
`99` (decisions and gates) → `prompts/README.md` and `prompts/_blocks.md` (contract ownership and
authoring standard).

## 1. Checkout and delta since `01-inventory.md`

| | |
|---|---|
| Branch / HEAD | `phase7-inventory` at `f8acfb8` ("docs: record the prompt-pack inventory for the remaining waves") |
| Base | `origin/overhaul-investigation` `bca079d` (durable effects #136); the map was first written against `55ab5b7` |
| Worktree | only untracked `pr-body.md` (pre-existing); no staged or unstaged change |
| Canonical docs | no commit between Prompt 00 and Prompt 01 changes them; the prompt 15 status rows in `07` are corrected with this map (status only) |
| Migrations | `0001`–`0010`; next free number is `0011`, read from the directory at branch cut (`07 §7`) |
| Protocol | `PROTOCOL_VERSION = 1.1` (`crates/domovoi-proto/src/version.rs:17`) |
| Prompt documents | 14 of 51; 13 are A–D, plus `24-capability-contracts.md` |
| Prompt 15 | merged into `overhaul-investigation` as #136 (`bca079d`); no migration (its columns came with `0006`) |

**Delta: none material.** No inventory finding needs refreshing. Prompt 15 merged after Prompt 00 (#136),
so the last implementation prerequisite in wave D is satisfied; nothing else in the repository changed.
The inventory's own §1 describes the `durable-timers` checkout it was written in; it was committed
unchanged to this branch, so that description records the inspected tree rather than the current branch
name.

The LLM integration work the task mentions is present in this tree, so no E–M brief should re-specify it:
`UsageSource`, `Usage.cost_micros`, `prompt_bytes` (default 128 KiB), `LlmCapability`,
`LlmError::CapabilityRejected`, `prices_usage()` (`domovoi-core/src/ports/llm.rs` via the adapters),
Anthropic `Thinking`/`RedactedThinking`, OpenAI `reasoning`, CLI allowance charging, and the run deadline
all exist (`crates/domovoi-llm/src/{anthropic,openai,cli}.rs`, `crates/domovoi-llm/tests/llm/*`). The
reconciled list in `prompts/17-llm-reliability.md:26-36` matches. Only prompts 31, 32, 35 and 36 read
LLM code; if the owner's separate integration process lands more, only those four briefs need a factual
refresh.

## 2. Waves versus milestones

Waves are execution batches; milestones are the roadmap's units of acceptance. They do not coincide.

| Wave | Prompts | Milestone(s) completed or advanced |
|---|---|---|
| E | 20 | M1 (start): wire 1.2, agent journal |
| F | 21, 22, 23 | M1 (finish): reconcile, plan hash v2/Authorization, single boundary |
| G | 24 (written), 25, 39 | M2 (contracts, registry) and the M7 evaluator that M2 unblocks |
| H | 26, 27, 28, 29 | M3 (operation aggregate, Investigate, SSE, use-case layer) |
| I | 30, 31, 34, 36, 41, 40, 58 | U1, M4, M5 (facts), M6, P1 (agent), M7 (shadow), M3 removal |
| J | 32, 35, 37, 42, 33 | M4, M5 (experience), U3, P1 (hub), U2 spike |
| K | 43, 48, 49, 50, 51, 38, 52 | U4 pages, U2 canvas and playback |
| L | 44, 47 | H1 (three identities) |
| M | 45, 46 | M8 (L2 autonomy), M9 (containment) |

`∥` in the index means no forced order, never concurrent execution: the owner runs one branch and one PR
at a time (`07 §1`, `prompts/README.md:11-12`).

**Model suitability.** The letters are the index's own S/M/L (`07 §1`). S and M are routine brief drafting
with the normal review. L means the brief itself needs a careful reviewer, and for 20, 22, 25, 26, 27, 31,
39, 41, 45, 46, 47 and 55 the design surface (protocol, authorization, evaluation, executor, privilege,
containment) should be reviewed before the prompt is launched, not after.

## 3. Wave E — M1 start (prompt 20)

**Roadmap objective (06 §M1).** One coordinated protocol 1.1 → 1.2 upgrade; agent journal with epoch;
`action_status`; mutation rate limiter; the 1.2 fields the later milestones need.

**Documents.** None; `07 §6.2` lists 20 as unwritten.

**Already in code.** Protocol 1.1 and the message modules (`messages/{handshake,work,stream}.rs`); agent
spool (`crates/domovoi-agent/src/{spool.rs,production.rs,local.rs}`); hub `agent_server/{mod,compat}.rs`;
`runtime/dispatcher.rs`; `core/ports/agent_link.rs` with `HostAvailability`; runner argv spawn; the
delivery durability from 16. No journal, no epoch, no `action_status`, no limiter, no 1.2 frames.

| ID | Remaining work | Owns / migration | Consumers |
|---|---|---|---|
| 20 | Complete RW-42 wire list with frozen golden frames for 1.1 and 1.2 (`hello` extensions incl. `journal_epoch`, `catalog_hash`, ceiling fields, `helper_status`, `helper_identity`, `skill_runner_status`, `runs_as_root`, `boot_id`; `run_capability`, `refused`, `action_status`, `get_catalog`, `transfer_*`, `agent_status`, `attest_safety`/`safety_attestation`, `unsupported`); fsynced agent journal (`accepted`/`started{pid,pid_start}`/`finished`, `abandoned`) with dedup by `execution_id` and orphan handling; `action_status` answers; RW-38 limiter with boot-id ages | wire 1.2 (complete RW-42 contract); agent journal file; no hub migration named | 21, 22, 25, 41, 47 (`07 §4`) |

Helper and attestation handlers are defined here but exercised only in 47; `transfer_*` is present but
unused until 41.

**Dependencies.** 13–19 merged, including 15 (#136); no migration named for the hub — the agent journal
is a file on disk. Verify at drafting whether the hub stores `journal_epoch` in a column.

**Components.** `domovoi-proto` (all), `domovoi-agent` (journal, limiter, runtime), `domovoi-hub`
agent_server + dispatcher, `domovoi-testkit` (golden frames, crash fixtures), `docs/book` protocol page.

**Validation.** `duplicate_dispatch_runs_once`; `agent_rate_limit_applies_to_human_kind`; every RW-38
test; 1.1/1.2 golden frames; a 1.2 hub rejecting a 1.1 agent as `Incompatible`. The `04 §15.2`
crash-window tests belong to 21, which owns the reconcile path they exercise.

**Blockers.** None. Type: implementation, cross-cutting. Model: **L** — careful reviewer, strongest
available model.

## 4. Wave F — M1 finish (prompts 21, 22, 23)

| ID | Work the prompt must specify | Owns / migration | Key existing code | Named validation |
|---|---|---|---|---|
| 21 | `OutcomeUnknown` as a terminal outcome; reconcile on `hello`; epoch change, `not_found`, orphan and journal-lost rules; never auto-fail a `Dispatched` execution | Outcome representation; **migration expected** — `investigations/D-protocol.md:175` names a store text column, `03`/`04` do not. Verify, do not invent | `runtime/dispatcher.rs`, `agent_server`, `actions/executor.rs`, `actions/recover.rs` | `crash_after_accepted_is_outcome_unknown`, `crash_after_started_is_outcome_unknown`, `finished_durable_reply_lost_reconciles`, `hub_crash_before_commit_reconciles_from_journal`, `epoch_change_is_outcome_unknown`, `not_found_with_matching_epoch_is_not_run` |
| 22 | Plan hash v2 (contract+version, `impl_hash`, canonical target identity, canonical args, `execution_id`); approval binding columns; `Authorization{Human,Policy}` with `PolicyPermit` type only; CI enforcement of both constructors | `ExecutionId`, `Authorization`, plan hash v2, binding columns; **migration** (`05:110`) | `core/src/action/approval.rs` (`PlanBinding` v1), hub `auth/`, `clippy.toml`, `xtask/src/arch/` | `approval_bound_to_displayed_plan_hash`, `v1_actions_keep_v1_binding_after_upgrade`; arch rule; v1 actions labelled |
| 23 | One `execute(AuthorizedExecution)` path; one `collect_evidence` read boundary; arch rules that stop transports reaching `AgentLink` | No contract, **no migration** | `engine/actions/{executor,writer}.rs`, `core/ports/agent_link.rs`, hub transports | `cargo xtask arch-check`; existing action/evidence suites |

**Consumers.** 22 → 23, 26, 45 (`07 §4`). 23 → 24 (ordering only), 25, 26. 21 → 22, which depends on it;
26 reuses its reconcile rules for `Dispatch` rather than owning them again.
**File overlap.** 22 and 23 both edit `engine/src/actions/*` and `core/src/action/*`; run 22 before 23.
**Blockers.** None beyond 20. Type: implementation. Model: 21 M, 22 **L**, 23 M.

## 5. Wave G — M2 and the evaluator (24, 25, 39)

**Objective (06 §M2, §M7).** Capability contract types and registry replace `PolicyLink`; target
identity resolution; a pure policy evaluator with locked Deny and hard limits.

**24 exists and is usable.** `prompts/24-capability-contracts.md` owns `ContractId`, `EffectClass`,
`CapabilityContract`, the index, ceiling summary, built-ins and the skill floor. It adds no table, no
migration and no wire (`24:510-519`). Its dependency on 23 is ordering only (`24:7`). **Preserve it.**

**Already in code.** `ReadTool` (38 files), `PolicyLink` (5 files), skills manifests, `settings` table.
`core/src/capability/` does not exist; no registry, no availability states, no evaluator.

| ID | Remaining work | Owns / migration | Consumers | Named validation |
|---|---|---|---|---|
| 25 | Registry: availability (`Unavailable(helper_missing)`, `HostIncompatible`), catalogue from `get_catalog`, replace `PolicyLink` at the boundary, target identity resolution, read-only inventory UI, skill floor in `settings` | capability registry and `Availability`; no new table named (floor in `settings`) | 26, 41, 49 (index deps) | `disabled_capability_refused_at_boundary`, `target_identity_change_refused`, `container_name_grammar_enforced`; R-7 test |
| 39 | `domovoi_engine::policy::{evaluate, Decision, Rule}`: specificity order, locked Deny, hard limits, publication helpers, simulator entry point | evaluator API; **no migration** (40 stores versions) | 40, 43, 45 | all 22 tests of `04 §7.7` |

**Dependencies.** 25 needs 23 + 24 + wire 1.2; 39 needs 24. Order between 25 and 39 is free (`07 §3`).
**Components.** New `core/src/capability/`, `engine/src/policy/`, hub registry service, `web/src/features`
inventory view; no new crate (`04 §1`).
**Blockers.** None. Type: implementation. Model: 25 **L**, 39 **L** (security judgement), 24 M.

## 6. Wave H — M3 operation aggregate (26, 27, 28, 29)

**Objective (06 §M3).** `operations`, `steps`, `step_edges`, `executions`, `operation_incidents`; one
writer with a pure `decide`; effect runner; Investigate operation behind `orchestration.enabled`;
journal-backed SSE; use-case services; legacy projection.

**Already in code.** `incident_events` and `api/events.rs` (broadcast, no ids/replay), `manager/*`
writers, `actions/{executor,writer,recheck}.rs`, `triage/{queue,run,proposals}.rs`; the effect lifecycle
arrives with 15, which merged as #136. No operations tables, no trace schema, no use-case
layer (`auth/interactions/*` imports `crate::api::*`).

| ID | Remaining work | Owns / migration | Consumers |
|---|---|---|---|
| 26 | Aggregate types and tables; writer + execution module; pure `decide`; effect runner; budgets; seeded simulator; trace schema golden fixtures | `Operation`, `Step`, `StepKind`, `StepOutcome`, edges + **migration** for 5 tables; trace schema | 27, 28, 29, 30, 38, 52 |
| 27 | Triage as an Investigate operation behind the flag; parity suite; legacy projection; step approval endpoint with v2 hash | Investigate operation and step approvals; no migration stated | 30, 31, 58 |
| 28 | Journal SSE: `id = seq`, replay, `reset`, `as_of_seq`; copy `incident_events` into `journal`; retire the old table | SSE contract; **migration/copy step** (RW-15 copy rules) | 30, 36, 51 |
| 29 | Hub application services; module-edge rule forbidding transports importing each other | use-case services (no data contract); no migration | all later hub transports (RW-19) |

**Named validation.** 26: seeded simulator, `operation_resumes_after_restart`,
`failed_verification_starts_new_cycle`, `budget_exhaustion_pauses`, `no_blind_retry_of_failed_step`,
`legacy_action_completes_under_original_id`. 28: `sse_replays_missed_events`,
`sse_resets_beyond_retention`, `lagged_subscriber_loses_nothing`. 27: the parity suite, and with the flag
off behaviour equals the baseline. 29: `cargo xtask arch-check`.

**Dependencies.** 26 needs 22's `ExecutionId`/`Authorization`/plan hash v2 and 15's effect lifecycle;
27 and 28 and 29 each need 26 (order among them free). 27 before 30, 31 and 58; 28 before 30, 36 and 51.
**File overlap.** 26 creates `engine/src/operations/*`; 27 rewrites `engine/src/triage/*` and
`actions/*`; 28 replaces `api/events.rs`; 58 later deletes `triage/queue.rs` and `actions/recheck.rs`.
**Blockers.** None. Type: implementation, security-adjacent. Model: 26 **L**, 27 **L**, 28 M, 29 M.

## 7. Wave I — backend and first UI slice (30, 31, 34, 36, 41, 40, 58)

**Objective (06 §M4, §M5, §M6, §P1, §M7, §U1).** Closed meta-tools and a plan validator; knowledge
facts; durable conversations; package backend on both sides; policy shadow; the Operations UI; removing
the legacy triage path.

**Already in code.** Triage pipeline (`triage/{tools,prompt,context,agent,complete,chat}.rs`), chat page
with `sessionStorage` (`web/src/features/chat/conversation.ts`), notes + `repos/knowledge.rs`,
`digest_facts`, rules and `PolicyLink`, approvals feature, incidents/hosts/skills pages, web kit
(`web/src/components`) and design guards. No meta-tool enum, no `ContextSource`, no facts/entities, no
durable conversations, no packages, no policy versions, no Operations pages.

| ID | Remaining work | Owns / migration | Consumers |
|---|---|---|---|
| 30 | Shell/nav, Operations list and detail, step tree, inspector, structured approval card, banners, Playwright smoke | approval card component; no migration | 33, 37, 51 (shell); all K pages |
| 31 | Closed meta-tool enum, plan validator, target resolution, remove the `PROPOSALS:` text protocol | `ContextSource` port; no table named | 32, 35 |
| 34 | Facts, relations, entities; `metric_samples` rename (new table + view); profile backfill; management-path facts and confirmation; knowledge API | knowledge API; **migrations** (tables + rename) | 35, 46, 48 |
| 36 | Conversation aggregate and writer, streaming, scope, stale replies, draft retention, panel + workspace | conversation aggregate; **migration** `conversations`, `messages` | none listed in `07 §4` |
| 41 | Agent package verification/limits/link rejection, local keys, install/uninstall/list over `transfer_*` | agent-side package state (files); no hub table | 42 |
| 40 | Policy versions, shadow recording, simulator, publication report, step-up | policy versions; **migration** `policy_versions` (+ decisions) | 43, 45 |
| 58 | Remove triage queue and `recheck.rs`, flip the flag default, expire legacy actions | none; gate = parity suite green | — |

**Named validation.** 31: `model_meta_tools_are_closed` gate, `invalid_plan_step_is_refused_with_reason`,
`missing_evidence_is_unknown_not_false`, `model_target_resolved_or_refused`, a novel-incident drill.
34: `inferred_fact_never_becomes_confirmed_without_user`, `retracted_fact_not_retrievable`,
`conflicting_sources_kept_as_disputed`, `network_change_marks_management_paths_stale`,
`owner_cannot_confirm_uncovered_management_path`. 36: `closing_panel_does_not_stop_operation`,
`stale_reply_discarded`, `global_scope_is_explicit`, arch rule `operator_has_no_executor`. 41: all
`04 §11.2`. 40: all `04 §7.7` plus `shadow_rules_never_enforced` and the two-week shadow acceptance.

**Dependencies and seams.** 30 needs 27 + 28; 31 needs 27; 34 needs 11 + 27; 36 needs 28; 41 needs 20 +
25; 40 needs 27 + 39; 58 needs 27 + 28 and the parity gate. Order inside the wave is free subject to
those edges, except 41 before 42 (wave J) and 31 before 32 (wave J).

**Interface seam to state in two briefs.** 34 stores management paths and the confirmation command, but
"confirm only covered paths" needs a verified helper attestation, which is 47's work (RW-37). 34 must
depend on a coverage check port and must not fake verification; 47 supplies the verifier. Do not close
R-10 on the strength of 34's tests.

**Blockers.** None for drafting. Type: implementation; 58 is removal. Model: 30 M, 31 **L**, 34 M, 36 M,
41 **L**, 40 M, 58 M.

## 8. Wave J — context, experience, visibility, packages hub, graph spike (32, 35, 37, 42, 33)

**Objective (06 §M4, §M5, §U3, §P1, §U2).** Layered context with one provider loop; experience search;
visibility infrastructure; hub side of packages; the R-1 spike.

**Already in code.** Triage context/prompt modules and the integrated LLM behaviours (see §1); the notes
feature and `repos/knowledge.rs`; `digest_facts`; theme/motion tokens; no visibility primitive, no install
jobs, no graph.

| ID | Remaining work | Owns / migration | Consumers |
|---|---|---|---|
| 32 | Layered context, item statuses, evidence dedupe, per-layer caps, one provider-chain loop for triage/follow-up/chat | none; consumes `ContextSource` | 45 |
| 35 | `resolution_summary_fields` revisions + FTS, `search_experience`, similar past operations | none listed; **migration** for summary fields/FTS | 48 |
| 37 | Visibility categories, `useVisibility`, Detailed/Standard/Essential preferences endpoint | visibility categories; preferences storage (table not named; verify `settings`) | 38, 43, 48, 49, 50, 51 |
| 42 | Hub package index, owner import (UI + `domovoi package import`), store, relay, jobs, audit | no contract listed; **migration** `install_jobs` | 49 |
| 33 | R-1 spike: library choice under CSP/no-inline-style/bundle budget; evidence + `web/DESIGN.md` amendment proposal | R-1 evidence | 38 (gate), 52 |

**Named validation.** 32: caps and statuses (M4 list). 35: `experience_search_is_host_scoped`,
`experience_block_labelled_not_authorization`, `code_owned_summary_fields_reject_user_edits`,
`summary_edit_goes_through_operation_writer`, `stale_summary_edit_rejected`. 37:
`p0_visible_in_every_profile` (axe runs on every page test). 42: all `04 §11.2` hub cases. 33: measured
evidence, not a library opinion.

**Dependencies.** 32 needs 31; 35 needs 27 + 31 + 34; 37 needs 30; 42 needs 41; 33 needs 30. 35 before
48; 32/35/37/42/33 have no forced order among themselves. 37 must land before any page that consumes
visibility.
**Blockers.** R-1 is open and only 33 resolves it; 38 and 52 must not be drafted as if accepted.
Type: implementation (32, 35, 37, 42), research/validation (33). Model: 32 M, 35 M, 37 M, 42 M, 33 M.

## 9. Wave K — U4 pages and U2 canvas/playback (43, 48, 49, 50, 51, 38, 52)

**Objective (06 U4, U2; 04 §13).** Policy, knowledge, capability, audit/retention, attention and search
pages; the graph canvas and, last, playback.

**Already in code.** The web kit, tokens, tone/icon registries, `QueryBoundary`, `useActionFeedback`,
`describeError`, `/dev/design`, axe and bundle-budget checks; existing incidents/hosts/settings pages as
patterns. No policy, knowledge, capability, audit or graph routes (`web/src/app/routes.tsx`).

| ID | Remaining work | Depends on | Notes |
|---|---|---|---|
| 43 | Policy editor, effective-policy explanation, simulator UI, publication report | 37, 40 | locked-rule styling; step-up flows already in 19's `settings/step_up.rs` |
| 48 | Knowledge browser/detail, proposals, management paths view | 37, 34, 35 | the management-path **coverage** view needs 47 (index `§4` lists 48 as a 47 consumer) |
| 49 | Capability Center, owner import, install wizard, ceiling and helper views, key-enrollment screen | 37, 25, 42 | ceiling/helper/key-enrollment views need 47 (index `§4` lists 49 as a 47 consumer) |
| 50 | Audit page (legacy and chained) and retention settings | 37, 09, 57 | smallest prompt; 09 and 57 are merged |
| 51 | Control Room, attention centre, global search | 37, 28, 30 | consumes SSE contract and the shell |
| 38 | Graph canvas: interactive trace graph, keyboard model, motion, reduced motion | 33, 37; **gate R-1 accepted** | moves `web/DESIGN.md` planned tokens into the kit in the same PR |
| 52 | Historical playback | 38 | last U2 deliverable (D-25) |

**Validation.** Each page: axe on what the test leaves on screen, `QueryBoundary` states, action feedback,
`node scripts/screens.mjs` at four widths in both themes, `npm run size` inside budget, 250-line file
limit, no inline style/raw button. 38/52 additionally: keyboard model, reduced motion, at most five nodes
animated per burst (`DESIGN.md:380-388`).

**Blockers and one ordering inconsistency.** 38 needs R-1 accepted. 48 and 49 are listed in `07 §4` as
first consumers of 47's helper protocol and `SafetyIdentity`, but `07 §3` places them in wave K, before
wave L's 47. Resolve before drafting those two: either (a) move 47 ahead of the helper-dependent page
slice, (b) split 48/49 into a first slice without helper/safety views and a follow-up that consumes 47,
or (c) record an explicit deferral of those views. This map does not reorder the index; it flags the
mismatch as the only material ordering conflict found.

## 10. Wave L — H1 three identities (44, 47)

**Objective (06 §H1).** Control agent as `domovoi-agent`, skills as `domovoi-skill`, privileged work only
through the root helper; signed safety attestation; installer migration.

**Already in code.** Root agent unit (`deploy/systemd/domovoi-agent.service`, `install.sh`), agent runtime
starting skills, wire 1.2 handlers for `helper_status`/`attest_safety` defined by 20. No helper, no skill
runner, no enrollment, no attestation.

| ID | Work | Gate | Kind |
|---|---|---|---|
| 44 | ADRs for the three identities and for helper attestation; record every item 54/55 left unproven | 54 and 55 evidence, or unproven items recorded (`07 §6.4`) | documentation |
| 47 | `domovoi helper serve` (socket activation, peer-uid, typed groups incl. no pause, `[safety]`, key lifecycle, state files, `safety.attest` in the `04b` encoding); `domovoi skill-runner` as `domovoi-skill`; agent routing of skills and privileged work; hub key enrollment, attestation verification, covered/uncovered view; unit sandbox; installer migration; selftest; O-30 behaviour | **O-30 answered; R-3, R-9, R-10 accepted**; 20, 22, 44 merged | implementation |

**Named validation.** All RW-37 and RW-39 tests (`helper_attestation_signature_verifies`,
`report_parser_rejects_duplicates_unsorted_and_noncanonical`, `config_hash_matches_vectors`,
`route_state_hash_matches_vectors`, `helper_rejects_wrong_peer_uid`, `helper_has_no_pause_operation`,
`fresh_install_enables_read_groups_only`, `package_install_cannot_write_helper_paths`, …), plus
`agent_runs_unprivileged_after_install` and `checks_report_helper_unavailable_not_failure`, and the six
acceptance conditions of `06 §H1`.

**Split option.** `07 §2` and `§3` allow 47a/47b/47c in order; the index does not define the boundaries.
Choose them at drafting (candidate seams: helper process + attestation; skill runner + agent routing;
installer + hub enrollment), and keep O-30/R-3/R-9/R-10 as the gate for whichever slice needs them.

**Blockers.** O-30 open; R-3, R-9, R-10 open; 54 and 55 must produce evidence first. R-10 stays a
validation gate until 47's implementation and tests exist — attestation is not "validated" by design
text (`03:858-859`).

## 11. Wave M — M8 and M9 (45, 46)

**Objective (06 §M8, §M9).** Bounded autonomy with permits and guardrails; then L3 containment.

**Already in code.** `HostAvailability` (`core/src/ports/agent_link.rs`), action dispatch and the runner,
and the `ReadTool` catalogue. No host ceiling, no `PermitIssuer`, no permit, no guardrail, no circuit
breaker, no containment, and no helper protocol until 20 defines it and 47 implements it.

| ID | Work | Gate |
|---|---|---|
| 45 | `PermitIssuer`, hard limits, guardrails, circuit breaker, global pause, first L2 contracts `unit.restart` and `container.restart`; authorization attribution and pause switch in the UI | **R-8 accepted; H1 complete**; deps 32, 40, 47 |
| 46 | `firewall.block_source` with kernel timeout, two-step apply with 30 s confirmation, verified and unverified human paths, management-path exclusions, reconciliation, drills | **R-2, R-10 accepted**; deps 45, 34, 47 |

**Named validation.** 45: `no_permit_without_policy_allow`,
`agent_refuses_policy_dispatch_outside_ceiling`, `circuit_breaker_suspends_rule`,
`pause_blocks_new_permits`, `unit_restart_refused_when_unit_not_in_helper_allowlist`,
`unit_config_change_invalidates_approval`, injection drill. 46: the `04 §9.1` drills and
`block_expires_with_agent_stopped`, `unconfirmed_block_expires_in_30s`, `excluded_address_refused`,
`model_cannot_trigger_containment`, `active_containment_reconciled_after_agent_restart`.

**Planning question to settle before drafting 45.** `container.restart` needs helper `docker.write`, and
D-24 requires a narrow helper with its own ADR; index row 44 names only the identity and attestation
ADRs. Either 44 covers the Docker-write helper or 45 excludes `container.restart`. Do not invent the ADR
inside 45.

**Blockers.** R-8 (45), R-2 and R-10 (46). Unattended containment stays disabled until the drills pass
(D-22); none of this may be described as production-ready before evidence exists.

## 12. Definitive document work list

**Draft (37).** 20, 21, 22, 23, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42,
43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 58. No new IDs are needed: every remaining item
already has an ID in `07 §2`. (38 IDs are planned for waves E–M; 24 exists, so 37 are missing.)

**Preserve unchanged (1).** `prompts/24-capability-contracts.md` — complete, correct dependency (23,
ordering only), no schema or wire.

**Reconcile (0).** No E–M prompt document exists to reconcile, and Prompt 17 is already reconciled.

**Status-update (documents: 0; index rows: listed in `01-inventory.md §7`).** The stale items are index
launch-status rows and migration claims, not prompt-file content. They belong in a later status-only
change to `07`, which this task does not make.

**No document needed.** Waves A–D (13 docs, implemented); research items folded into prompts (R-4 → 16,
R-5 → 17, R-6 → 08/20, R-7 → 25); the items in `06` "Deferred and after-v1", which get no prompt because
no roadmap prerequisite needs them: owner-signed approvals and policy bundles, offline emergency permits,
third-party package registries, agent self-update, `container.pause`/`unit.stop_temporary`, the other
containment candidates, protocol support for older agent generations, the revoked-build list, host tags,
multi-user/RBAC, advanced audit anchoring, customizable Control Room widgets, integrations and providers,
and vector/graph databases.

## 13. Execution order and drafting batches

Launch order is a single sequence; the slashes below are order-flexible, not concurrent. Each batch is at
most three briefs, and each brief may only be written against the merged work named as its prerequisite.

| Batch | Prompts | Write after (merged) | Why grouped |
|---|---|---|---|
| R-0 | 53, 54, 55, 56 | nothing | research prompts; writable now; they gate L and M |
| EF-1 | 20 | 15 (merged, #136) | wire 1.2 and agent journal are one contract |
| EF-2 | 21, 22 | 20 | both hub-side M1 consumers of the journal; 22 after 21 |
| EF-3 | 23 | 22 | boundary rules against the final authorization types |
| GH-1 | 25, 39 | 20, 23, 24 | registry and evaluator; independent, both consume 24 |
| GH-2 | 26 | 22, 25 | operation aggregate needs ExecutionId and the registry |
| GH-3 | 27 | 26 | Investigate operation and step approvals |
| GH-4 | 28, 29 | 26 | SSE and use-case layer share the hub wiring but not files |
| IJ-1 | 30, 31 | 27, 28 | Operations UI and meta-tools |
| IJ-2 | 34, 36 | 27, 28 | knowledge facts and durable conversations |
| IJ-3 | 40, 58 | 27, 39 and a green parity suite | shadow policy; legacy removal |
| IJ-4 | 41, 42 | 20, 25 (and 41 for 42's brief) | package agent side, then hub side |
| IJ-5 | 32, 35 | 31, 34 | layered context and experience search |
| IJ-6 | 37, 33 | 30 | visibility categories and the R-1 spike |
| KM-1 | 43, 48, 50 | 37, 40, 34, 35 | policy, knowledge, audit/retention pages |
| KM-2 | 49, 51 | 37, 25, 42, 28, 30 | capability centre and control room |
| KM-3 | 38, 52 | 33 accepted, 37 | graph canvas, then playback |
| KM-4 | 44 | 54, 55 evidence | ADRs |
| KM-5 | 47 (or 47a/b/c) | 20, 22, 44, O-30, R-3/R-9/R-10 | identity work |
| KM-6 | 45 | 32, 40, 47, R-8 | autonomy |
| KM-7 | 46 | 45, 34, 47, R-2, R-10 | containment |

Adjustments to the proposed E–F / G–H / I–J / K–M grouping, and why: E–F is four prompts, so it splits
into three batches; G–H is five (24 exists) and splits by contract owner; I–J is twelve and splits by
consumer cluster; K–M is thirteen and splits by gate. R-0 sits outside the waves because 53–56 are
writable now and must not wait for E.

## 14. Blockers, scope of impact, and facts to refresh at drafting time

| Gate / open item | Source | Blocks only |
|---|---|---|
| Parity suite not yet green | 06 §M3 acceptance | 58 |
| O-30 open | `99:42-46` | 47's skill-runner refusal rule and selftest; P1 package guidance |
| R-1 open | `99:54` | 38, then 52 — not 33 |
| R-3, R-9 open | `99:56`, `99:58` | 47 acceptance |
| R-10 open | `99:59` | 47 acceptance and 46; stays open until tests exist |
| R-8 open | `99:57` | 45 |
| R-2 open | `99:55` | 46 |
| 47 before 48/49 (index §4 consumers vs §3 order) | `07:207` vs `07:188` | drafting of 48 and 49 helper/safety views |
| D-24 Docker-write helper ADR | `99:35`, `06 §M8` | `container.restart` in 45 |

Facts that are stated loosely in the canonical documents and must be verified in code at drafting time,
not invented:

1. **OutcomeUnknown persistence (21).** `03`/`04` define the outcome; the store column and migration
   appear only in `investigations/D-protocol.md:175`. Check the current schema before writing 21.
2. **Migration presence.** Named or implied: 21 (status), 22 (binding columns), 26 (five tables), 28
   (`incident_events` copy), 34 (knowledge tables, `metric_samples`), 35 (summary fields/FTS), 36
   (`conversations`, `messages`), 40 (`policy_versions`, decisions), 42 (`install_jobs`). Not named: 20,
   23, 25, 27, 29, 30, 31, 32, 33, 37, 38, 39, 41, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 58. Treat
   "not named" as "verify", never as "no schema change".
3. **Wire 1.2 field list (20).** `03 RW-42` is authoritative; the index scope is a summary. Golden
   frames freeze it, and any later change is a coordinated RW-42 decision.
4. **47's split boundaries** are not defined anywhere; choose them at drafting.
5. **UI prompts (30, 37, 38, 43, 48, 49, 50, 51, 52)** must move the `web/DESIGN.md` "Planned (added with
   the operation UI)" entries into the kit, `/dev/design` and tests in the same PR (`DESIGN.md:365-368`),
   and stay inside the guardrails in `AGENTS.md`.
6. **LLM-touching prompts (31, 32, 35, 36)** read the integrated code, not the pre-integration branch;
   only their details would need a refresh if the separate integration process changes that code again.

## 15. Scope of this document

No prompt was drafted, no canonical document was edited, no source, test, migration or design-pack file
was touched. The only change is this file. Before drafting begins, the owner should settle the two
planning questions in §14 (48/49 versus 47, and the Docker-write ADR for `container.restart`) and decide
whether 53–56 are written now.
