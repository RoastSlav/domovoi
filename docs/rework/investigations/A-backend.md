# A. Backend architecture investigation

Scope: ARCHITECTURE.md, ADRs 0002/0003/0009/0011/0019 (also 0014, 0016 for context), crates core, engine, hub, store (API only), checks, chat, llm, config, with proto/agent touched where they bound the design. Read-only. Paths are repo-relative. Labels: FACT (read in code), INFERENCE (follows from facts), RECOMMENDATION.

## 1. Module boundaries and dependency edges vs ARCHITECTURE.md section 4

- FACT: Cargo edges match the table. Engine depends only on core (`crates/domovoi-engine/Cargo.toml`); store/llm/chat/checks/runner depend only on core; hub depends on core, config, proto, engine, store, llm, chat, checks; agent on core, config, proto, checks, runner. The rules are encoded in `xtask/src/arch/rules.rs:19-145` (same table). No production-edge drift.
- FACT: engine has `llm` and `testkit` as dev-dependencies (`domovoi-engine/Cargo.toml`), an adapter reaching into the app layer in tests only. R1 says "normal deps", so legal, but the engine's gate tests are therefore coupled to the concrete llm crate rather than only `ScriptedLlm`.
- FACT (doc drift, small): section 4 lists `checks` features `host`/`net` only; the code has a third, `sources` (`domovoi-checks/src/lib.rs:14-19`; hub and agent enable it). Section 6 port table omits ports that exist: `SettingsRepo`, `UserRepo`, `SessionRepo`, `NotifyStateRepo`, `AdvisoryRepo`, `FactRepo`, `TriageCacheRepo`, `UsageRepo`, `InteractionSink`, `SignalSink`, `LogSource` (`domovoi-core/src/ports/repos/mod.rs`, `ports/agent_link.rs:140ff`). Section 6 says `Clock` is implemented "by engine (system)"; the real `SystemClock` is in the binary (`crates/domovoi/src/init.rs:135`). Section 9 lists a `digests` table; migrations create `digest_facts`, no `digests` (`domovoi-store/migrations/0001_init.sql`). Section 5.3 state diagram has no `Dismiss`, which is implemented (`domovoi-core/src/incident/transition.rs:43,167`). ADR 0016 (suggestions) is "Proposed" and nothing implements it: `AdvisoryKind` has no `Suggestion` (`domovoi-core/src/advisory.rs:10-32`), engine has no `suggest` module (`domovoi-engine/src/lib.rs:20-33`).
- FACT: the hub is the only place engine services meet adapters. `HubRuntime::assemble` (`domovoi-hub/src/runtime/mod.rs:171-347`, ~180 lines in one function) builds Store, PatternRedactor, IncidentManager, AgentServer, PolicyLink, TriageService, ActionService, chat adapters, ApiState, Dispatcher, Schedulers. Fits R6.
- FACT (intra-hub coupling, not covered by arch-check): the chat authorizer is built on HTTP API state: `auth/interactions/*` imports `crate::api::{hosts, profile, system}`, `ApiState`, and `api::incidents::latest_triage` (`domovoi-hub/src/auth/interactions/commands.rs:9`, `buttons.rs:12`, `mod.rs:30`). `auth` therefore depends on `api`, and `api` depends on `auth` (`Authed`, `web_approval`). INFERENCE: HTTP handlers double as the application-service layer for the hub; there is no hub-level "use case" layer between transport (axum, Discord) and engine.
- FACT: invariants INV-1 is enforced mechanically: `HumanApproval::new` allowed in `hub/src/auth/` and testkit (`xtask/src/arch/rules.rs:168-175`; scan in `xtask/src/arch/scan.rs:105-116`); `restore` only in store (`domovoi-store/src/repos/actions.rs:58`). Only call sites found: `hub/src/auth/approval.rs:21,33`.
- RECOMMENDATION: add a hub-internal "app" module (or promote to engine) for operations that API and chat both perform (ack/mute/dismiss/ask/approve), and update ARCHITECTURE sections 4/5.3/6/9 to match.

## 2. Domain model, lifecycles, enforcement points

Core is pure data + state machines (`domovoi-core`, ~6.7k lines, no async runtime; deps: serde, jiff, uuid, sha2, regex, async-trait only).

### Incident (`domovoi-core/src/incident/`)
- FACT: `IncidentStatus` = Pending | Open | Acked | Muted{until} | Resolved (`state.rs:17`). Fields are `pub(super)`; only `Incident::new_pending` and `apply()` mutate status (`state.rs:78`, `transition.rs:119`). Transition is a pure function `(incident, event, policy, now) -> changes`, matching ADR 0011.
- FACT: `Incident` carries a nullable `verdict`, `history` (tracked value series), `members` (grouped fingerprints), `source` (`state.rs:78-110`). It has no field for triage state, active plan, phase or linked actions. Triage state lives in the timeline (below), actions point at the incident via `actions.incident_id` only.
- FACT: single writer enforced: `IncidentManager` task owns `active: Vec<Incident>` cache (`domovoi-engine/src/manager/actor.rs:48-52`); every mutation arrives as `Command` (`manager/command.rs:12-99`), `decide()` is pure (`manager/decide.rs:435-499`), writes are applied, then events published on a `broadcast` (`actor.rs:139-185`). Even triage results, follow-ups, verdicts and tool-use records go through the manager (`Command::AttachTriage|SetVerdict|RecordFollowUp|RecordToolUse|ClearFollowUps`).
- FACT (weakness): writes in one command are applied one by one with no DB transaction (`actor.rs:142-150`, e.g. UpdateIncident, InsertSignal, Timeline); on failure the cache is reloaded and nothing is published, but earlier writes of the same command stay in the store. Store has `begin()` only in accounts/hosts/incidents insert paths (`domovoi-store/src/repos/incidents.rs:76,275`).
- FACT (weakness): per command the manager clones the whole active set twice (`actor.rs:243`, `decide.rs:442`). Fine at homelab scale; not a design to carry per-phase data.

### Signal / check result
- FACT: `Signal` (source, host, check, target, status Firing|Resolved, severity, summary, `Evidence` map, class Normal|Immediate); `Fingerprint = hash(host, check, target)` excludes volatile values (`domovoi-core/src/signal/mod.rs`, `fingerprint.rs`). `SourceKind` = BuiltIn | Webhook{kind} | Probe | Agent (`signal/mod.rs:21-33`).
- FACT: checks are a trait `Check { key, default_cadence, class, run(ctx) -> CheckReport }` with a hard-coded list in `Registry::build` (`domovoi-checks/src/framework/registry.rs`, 14 host checks + 4 container checks). A recheck for verification runs a check by key (`AgentLink::run_check`, `agent_link.rs:106ff`).

### Host / agent
- FACT: `Host{id,name,kind Local|Remote|External,labels}`, `AgentIdentity` (`domovoi-core/src/host.rs`). Agent `Hello` carries `capabilities: Vec<String>` and `skills: Vec<SkillSummary>` (`domovoi-proto/src/messages/handshake.rs:164-170`); the agent sends `capabilities: Vec::new()` (`domovoi-agent/src/link/handshake.rs:47`), so the field is unused.

### Action / approval (`domovoi-core/src/action/`)
- FACT: `ActionStatus` = Proposed | Approved | Denied | Expired | Dispatched | Rejected{reason} | Succeeded | Failed{reason} | Verified | VerifyFailed (`request.rs:15`). Transitions are methods on `ActionRequest` that check then mutate (`transition.rs:65-150`); approval needs a `HumanApproval` value by move, destructive needs web typed confirmation (`transition.rs:75-84`).
- FACT: `ActionRequest` is hard-shaped around "run one skill on one host": `{incident, host, skill: SkillName, risk, args: BTreeMap<String,String>, plan_hash, ...}` (`request.rs:130-146`). `PlanHash = sha256(skill, manifest hash, host, canonical args)` (`approval.rs:13-24`).
- FACT: single writer for actions (ADR 0019): `ActionService::spawn` joins `writer::run` and `executor::serve` in one task (`domovoi-engine/src/actions/mod.rs:205-230`). Commands: Approve, Deny, ExpireDue, Dispatch, Reject, Complete, Verified (`writer.rs:35-73`). Store guard `ActionRepo::update_from(action, expected_status)` refuses stale writes (`ports/repos/actions.rs:48`). Plain `update` is gone from the port.
- FACT (gap vs ADR 0019): triage code writes actions directly, outside the writer: `proposals::store` calls `actions.insert` and `actions.update_from` (token rotation) (`domovoi-engine/src/triage/proposals.rs:115,138`), and the follow-up path calls the same (`triage/followup.rs:252`). Insert of a new `Proposed` row cannot race a decision, and the rotate is guarded, so safe in practice; but the "only the writer writes actions" claim in ADR 0019 is not literally true.
- FACT (gap): `record_from` does `update_from` then `audit.append` then publish as three separate effects; an audit-append failure is only logged (`actions/mod.rs:127-153`). INV-7 guarantees append-only, not that every transition has an entry.
- FACT: the incident's `Verdict::Fixed` is set after a verified action by a separate command to the manager (`actions/recheck.rs:92-98`); action state and incident state are updated in different writers with no cross-entity atomicity.

### Triage report / proposal / outcome
- FACT: `TriageReport{headline,summary,cause,evidence,verdict,next_step,manual_steps,proposals,details}` with `validate()` caps (`domovoi-core/src/triage/report.rs:52ff`). `ActionProposal{skill,args,reason}` (`report.rs:39`). `TriageOutcome` = Report | Cached | Skipped | Failed (`report.rs:139`).
- FACT: there is no triage-run entity. The outcome becomes a timeline entry of kind `triage.report|cached|skipped|failed` with the outcome JSON in `detail` (`domovoi-engine/src/manager/annotate.rs:52-90`); tool use is `triage.tools`, follow-ups are `triage.followup`, clears `followup.cleared`, human actions `human.action`, `human.dismissed`, `status.changed`, `severity.changed`, `verdict` (`annotate.rs:44,200`, `triage/history.rs:8-13`, `decide.rs:152ff`, `dismiss.rs:48-58`).
- FACT (important): control flow depends on those strings. The retry logic decides "already triaged", "failure chain length" and the reload filter by scanning `entry.kind` prefixes (`triage/queue.rs:189-221,249`); `incident_context.rs:26` filters `starts_with("triage.")`. State machine of "triage" exists only implicitly in these scans.

### Notes, baselines, acks, advisories, facts
- FACT: `Note{id,host,title,body_markdown,tags,updated_at}` with FTS5 (`domovoi-core/src/note.rs`, migration 0001). The "host profile" is generated by the hub into a Note tagged `host-profile`/`generated` plus JSON under `host.<id>.profile` in the settings table (`domovoi-hub/src/profile/mod.rs:1-14`, `triage/context.rs:30,200-211`). Facts about hosts are therefore free-text markdown or untyped settings JSON.
- FACT: `Advisory` kinds are a closed enum mirrored by a SQL `CHECK`, so each new kind needed a table rebuild migration (`migrations/0003`, `0004`; `advisory.rs:10-32`). `DigestFact` (`digest_facts` table) is typed key/label/value/unit per host and time (`ports/repos/knowledge.rs`, FactRepo) - the nearest thing to "facts with provenance" today, but it has no source/confidence/supersedes.
- FACT: `AckRule`, `MaintenanceWindow`, `BaselineEntry` are repos in `knowledge.rs`; they only suppress notifications (`engine/src/suppress.rs`, `decide.rs` `notify_for`).

### Chat
- FACT: two kinds. (a) Incident follow-up: stored in the incident timeline (`triage.followup`), replayed with a 10-turn / 16 KiB cap (`triage/followup.rs:48-51`). (b) General chat: history supplied by the caller each time (`api/chat.rs`), 20 turns / 16 KiB (`triage/chat.rs:23-26`). Both use the same ReadTool loop and admission gate, neither can approve.

## 3. Where business logic lives

- FACT: engine (8.5k lines) holds: correlation (`correlate.rs`, pure), notify policy (`notify/*`, pure), cards/digest/advisory builders (pure), incident manager, triage (queue, workers, context, tools, agent loop, proposals, cache), actions (writer, executor, recover, recheck, audit). Good separation of pure `decide` from IO shell for incidents and notify.
- FACT: hub (18.5k lines, the largest crate) holds transport and also real logic:
  - skill enable/disable policy (`agent_server/skill_policy.rs`, 446 lines; `PolicyLink` decorates `AgentLink`) - application policy living in an adapter layer.
  - host profile collection/diff/render and its scheduler (`profile/*`, ~900 lines) - produces knowledge (notes) from tool output.
  - advisories evaluation scheduling (`runtime/schedulers.rs:185ff`), digest assembly (`digest.rs`), notification delivery bookkeeping (`runtime/delivery.rs` 430, `dispatcher.rs` 362), settings layering/apply/rollback (`settings/*`, ~2k lines), chat command semantics (`auth/interactions/commands.rs`).
  - API handlers do orchestration, e.g. `retry_triage` spawns a triage run outside the queue (`api/incidents.rs:467-488`), bypassing queue concurrency but not the admission gate.
- FACT: duplication/near-duplication: three loops implement "provider chain, converse, record usage, build opening, cut/redact": `triage/run.rs:168-193`, `followup.rs:198ff`, `chat.rs:181ff`; each repeats the fallback loop and failure-reason joining rather than sharing one "model session" abstraction. Timeline-kind filtering (`kind.starts_with("triage.") && != followup && != tools`) is repeated in `queue.rs:189-221` twice.
- FACT: loop-guard and admission both keep rolling windows in process memory (`actions/mod.rs:114` `dispatches`, `triage/run.rs:25` `Limits.starts`). The guard is reloaded from the audit log at start (`actions/recover.rs:139`); the hourly model-run window is not (`Limits::default()` at `triage/mod.rs` spawn), so a restart (including every settings save, see 6) resets the hourly cap.
- Tight couplings:
  - FACT: `Shared` in triage holds 15 dependencies and is passed to every function (`triage/mod.rs:100-130`); tool execution, context building and admission all reach into `shared.deps` directly. Not a god object, but every new capability would add another dep.
  - FACT: ReadTool is wired in five places: core enum (`triage/tools.rs:8`), typed args (`tool_call.rs:54`), JSON schema (`tool_schema.rs`), engine dispatch (`engine/triage/tools.rs:95-162`, store-backed vs agent-backed split), agent handler (`domovoi-agent/src/requests/tools.rs:70-157`) plus proto `RunTool`. Adding one tool touches core, engine, proto, agent.
  - FACT: `AgentLink` is a trait with exactly four verbs (`run_tool`, `run_skill`, `run_check`, plus `presence`/`skills`) - capabilities cannot be extended without changing core, proto, hub link and agent.

## 4. Background workers and state flow

All in-process tokio tasks; no external broker. Flow is bus-and-DB, not work-queue.

- Bus: `broadcast::Sender<DomainEvent>` capacity 4096 (`runtime/mod.rs:175`). `DomainEvent{kind, incident (full clone), notify}` (`engine/src/events.rs`). Lag is handled per consumer by a counter and a reload (`triage/queue.rs:86-93`, `actions/executor.rs:62-70`, `dispatcher.rs:143-150`).
- Tasks (FACT, from `runtime/mod.rs`, `serve.rs`, `schedulers.rs`):
  1. IncidentManager + its own ticker (30 s Tick via `mpsc::WeakSender`, `manager/actor.rs:280-309`): TTL resolve, mute expiry.
  2. Triage: one `serve` task + N workers (default 2) + `retry_deferred` task over an in-memory `VecDeque<Incident>` queue (cap 64) and an in-memory `deferred` vec (`triage/queue.rs`, `triage/mod.rs:100-130`).
  3. Actions: writer loop + executor loop joined; executor spawns one task per approved action (`actions/executor.rs:32-41`), with a `running` claim set (`:108-125`).
  4. Dispatcher: consumes the bus and delivers per incident in order using a oneshot chain `Sequencer` (`runtime/dispatcher.rs:23-66`).
  5. Schedulers (hub): minute tick (dispatcher.tick + `expire_due` + digest), advisories, daily retention 03:30, daily backup 04:00, heartbeat, update check (`schedulers.rs:85-129`).
  6. Profile scheduler (collect one host per minute) (`profile/mod.rs` `spawn`).
  7. Agent server: WebSocket listener, per-agent session tasks, presence loop (`serve.rs:46-64`); chat gateway tasks (`serve.rs:74-86`).
- Hand-offs:
  - Signals: webhook/agent/probe -> `SignalSink::submit` -> manager mpsc. Backpressure is a bounded queue (`manager/handle.rs` `COMMAND_QUEUE`).
  - Incident confirmed -> `TriageRequested` event -> triage queue (event-driven, volatile) -> worker -> `attach_triage` command back to the manager -> `TriageCompleted` event -> dispatcher edits the card.
  - Proposal -> `ActionRepo.insert` (DB) -> card with chat token -> human click -> `ActionHandle.approve` (mpsc + oneshot) -> `ActionApproved` event -> executor task -> writer commands -> `AgentLink.run_skill` -> `ActionChanged` events.
  - Timer jobs are DB-polling: approval expiry sweep (`ExpireDue` every minute, store query by `expires_before`), notification re-notify/quiet-hours release (`dispatcher.tick`), digest (compares `digest_last_sent_at` setting).
- FACT (restart behaviour): after a restart the actions side recovers from the DB (`recover.rs`: approved are restarted, dispatched are failed with a "hub restarted" reason, loop guard reloaded). The triage side does not: `reload()` is called only on bus lag (`queue.rs:92,227`), never at start; the queue and deferred retries are lost, so incidents that were open and awaiting triage at the crash stay without a triage entry until someone presses retry. The manager does reload its active set at start (`actor.rs:80,118-135`).
- FACT: `Succeeded` actions awaiting verification are not resumed: `verify()` sleeps in memory (`recheck.rs:65`) after `ask_complete` (`executor.rs:281-284`); a restart in that window leaves the action `Succeeded` forever and the incident unverified. `dispatched` ones are failed rather than reconciled with the agent (`recover.rs:204-230`).
- INFERENCE: the system is "event-driven with DB as the recovery source for the action path only"; the ADR 0011/0019 single-writer pattern is the repeated idiom and works well, but it models entity state, not multi-step processes.

## 5. LLM integration

- Port: `LlmProvider { id, model, supports_tools, complete(LlmRequest) -> LlmResponse }` with `Part::{Text,ToolCall,ToolResult}`, `Usage`, `LlmError::{Transport,RateLimited,Auth,InvalidResponse,Unavailable}` (`domovoi-core/src/ports/llm.rs`). Port is non-streaming; adapters stream internally and assemble (`domovoi-llm/src/anthropic.rs:72-85`, `anthropic_stream.rs`, `openai_stream.rs`).
- Adapters: Anthropic, OpenAI-compatible, CLI (`domovoi-llm/src/factory.rs`). CLI reports `supports_tools() == false` (`cli.rs:192`) and gets pre-fetched tool output inlined (`triage/run.rs:208-239`, `tools::prefetch` in `triage/tools.rs:185-205`).
- ReadTool catalogue: closed enum of 11: DockerPs, DockerLogs, DockerInspect, DockerEvents, HostOverview, Journal, ListeningPorts, SmartReport, IncidentHistory, NotesSearch, RunReadSkill (`domovoi-core/src/triage/tools.rs:8-21`). The model's JSON is parsed to a typed, bounded `ReadToolCall` (`tool_call.rs:265`), unknown names yield an error result and a `ToolUse{raw}` record (`triage/agent.rs:57-80`). `RunReadSkill` is the generic extension point: it accepts any host skill with `risk == Read` and validated args (`triage/tools.rs:149-159`).
- Triage run (`triage/run.rs:54-132`): admit -> latest signal -> evidence digest -> cache lookup (`(fingerprint, evidence_digest)` within `cache_days`, plus a resolved prior incident must exist `run.rs:136-156`) -> build context -> provider chain -> parse report (one repair retry `run.rs:265-287`) -> redact -> `proposals::create` -> cache put -> attach via manager.
- Loop (`triage/agent.rs:130-211`): up to `max_tool_calls` (15), `max_seconds` (90 s deadline checked only after each turn), `max_output_tokens` (2000); once exhausted it sends "Budget exhausted" and one more turn. `messages.clone()` per turn; no token counting.
- Questions/chat: `TriageHandle::{follow_up, chat}` take a permit from a semaphore of size `concurrency` (`try_acquire`, so a second concurrent question is refused as Busy) and pass the same admission gate as `Purpose::Question` (`followup.rs:150-171`, `chat.rs:142-156`).
- Admission gate (PR #112): `triage/admission.rs`. `admit(shared, Purpose)` checks monthly budget (UsageRepo month total, `:44-55`) then a rolling-hour deque of run starts in memory; `Purpose::Triage` may use all `per_hour_runs` (default 6), `Purpose::Question` leaves `reserved_for_incidents` (default 2) free (`:73-103`). Triage refusal defers a retry to when a slot frees (`run.rs:59-79`, `queue.rs:94-125`). FACT: `budget_spent` fails open on a read error (`admission.rs:52`); `admit` counts a start before the model call and never refunds it on failure.
- Prompt size limits (FACT): context bundle = 7 parts, each cut to 4 KiB (`context.rs:15,63-70`); each tool result redacted and cut to 8 KiB (`tools.rs:31,87-93`); follow-up history 10 turns / 16 KiB; chat overview 8 KiB, question 4 KiB, turn 2 KiB, history 16 KiB (`chat.rs:20-35`). No total-prompt cap: 15 tool results x 8 KiB plus bundle (~28 KiB) plus history can reach roughly 170 KiB of input; no truncation of earlier messages.
- Provider fallback: `chain = [primary] + fallbacks` where fallbacks = per-provider `fallback_models` plus `secondary` (`triage/mod.rs` `chain()`, `runtime/mod.rs:367-400`). Fallback is whole-attempt (restart the conversation per provider) and any error counts (`run.rs:168-193`). FACT: engine never retries or honours `RateLimited{retry_after}`; the adapter parses it (`domovoi-llm/src/http.rs:135-146`) but grep finds no use in engine. Degraded mode: 3 consecutive failures raise `TriageDegraded` advisory (`run.rs:290-312`).
- Safety (FACT): redactor on every tool result and context and report (`tools.rs:87`, `context.rs:71`, `run.rs:100`); proposals only become `Proposed` actions after `validate_args` against the host's current `SkillSummary` and `applies_to` (`proposals.rs:37-53`); gate tests in `domovoi-engine/tests/gates.rs` (read-only tool set, injection, shell string, secrets, failure leaves card).
- Follow-up proposals use an out-of-band text protocol (`PROPOSALS:` + JSON array on the last lines, `followup.rs:20,120-140`) instead of structured output, parsed with `serde_json::from_str(...).unwrap_or_default()`.
- RECOMMENDATION: extract one `ModelSession` (provider chain, usage accounting, budget, deadline, history trimming) used by triage/follow-up/chat and by any future phase runner; add a total-prompt budget and honour `retry_after`.

## 6. Error handling and config patterns

- Errors (FACT): `thiserror` enums per boundary: `StoreError{NotFound,Conflict,Backend}`, `LlmError`, `LinkError`, `TransitionError`, `ActionError`, `ApprovalError`, `CommandError`, `TriageError`, `CheckError`, `RuntimeError`. Port errors flatten to `String` inside variants (`StoreError::Backend(String)`, `CommandError::Store(String)`, `TriageError::Store(String)`, `LlmError::Transport(String)`); causes and error identity are lost, and "refused because stale" is detected by `StoreError::Conflict` message match in places (`actions/writer.rs` `stale()`).
- Policy (FACT): fail open for alerts (`triage_failure_still_leaves_a_card` gate), fail closed for actions (`revalidate`, `reserve_dispatch` in `executor.rs:297-341`). Many background paths log-and-continue (`tracing::warn!`) on store errors (admission, cache put, usage record, audit append), which is deliberate but means silent loss of accounting and audit rows.
- Config (FACT): four layers (defaults < stored in DB < `hub.toml` < env), secret indirection, per-field provenance, JSON-schema-derived docs (`domovoi-config/src/layered.rs`, `provenance.rs`, `hub/mod.rs`). Saving settings triggers a supervised in-process runtime restart with rollback (`domovoi-hub/src/settings/apply.rs`). INFERENCE: every UI settings save drops all in-memory worker state (triage queue, deferred retries, admission window, `running` set); with the gaps in section 4 this is a recurring data-loss surface, not just a crash edge case.
- Tunables are mostly compile-time constants, not config: triage caps (`PART_BYTES`, `MAX_RESULT_BYTES`, `TOOL_DEADLINE`, `DEGRADED_AFTER`, `FAILED_RETRY`), action settings (`settings::actions()` defaults), proposal TTL (1 h).
- Retention (FACT): signals 30 d, timeline (`incident_events`) 90 d, triage_cache 90 d; incidents, audit, digest facts never purged (`domovoi-store/src/purge.rs:12-30`). ARCHITECTURE says "transcripts 90 days" - consistent, but this means the timeline is not a permanent log.

## 7. Fitness for the proposed target

### 7.1 Phase-based incident orchestration (investigate -> propose -> approve -> execute -> verify, resumable)
- Existing base worth reusing:
  - Pure state machines + single writer idiom (`incident/transition.rs`, `action/transition.rs`, `manager/decide.rs`, `actions/writer.rs`): the exact pattern to apply to a new `Run`/`Plan` entity.
  - `ActionRequest` states already cover propose -> approve -> dispatch -> succeeded -> verified, with plan-hash binding and store-guarded transitions, and restart recovery for Approved/Dispatched (`actions/recover.rs`).
  - Event bus + bounded workers; `Clock` port; `AgentLink::run_check` already offers verification.
- What blocks it:
  - No entity represents the whole remediation. "Investigate" is an ephemeral call stack (`run::triage`), results are timeline strings; "propose" produces N independent actions with no ordering/dependencies; "verify" is a side task spawned after `Succeeded` with in-memory sleep (`recheck.rs:56-100`).
  - Resume is incomplete: triage queue not rehydrated (`queue.rs:66-95`), verification not resumed (`executor.rs:281-284`), dispatched actions failed rather than reconciled (`recover.rs:204`).
  - Incident status machine is about alert lifecycle (Pending/Open/Acked/Muted/Resolved) and is intentionally independent of remediation; phase must be a separate state (do not overload `IncidentStatus`, it is persisted with a SQL CHECK).
  - Manager commands are incident-keyed and there is no per-incident serialisation point for long operations; phase steps would need their own writer (copy ADR 0019) keyed by run id.
  - No transactional boundary spanning run-state + action rows + audit + incident verdict (see section 2).
- Must be replaced: stringly-typed triage state in `incident_events.kind`; the in-memory `Queue`/`deferred` pair; the `TriageOutcome`-only persistence model. Must be added: a `Run` table (id, incident, phase, step cursor, status, started/updated, owner session) written only by a run writer, with the triage report and proposals as typed phase outputs.

### 7.2 Capability registry (discoverable, policy-gated typed capabilities replacing fixed action kinds)
- Existing base: `SkillManifest`/`SkillSummary` are already a typed capability descriptor (name, description, risk Read/Write/Destructive, `ParamSpec`s, `applies_to` checks, timeout, `verify`, manifest hash) discovered from agents at `Hello` (`domovoi-core/src/skill/manifest.rs:109,172`, `domovoi-proto/.../handshake.rs:164-170`), validated by `validate_args` (`skill/validate.rs`), bound by `PlanHash`, narrowed per host by `SkillPolicy`/`PolicyLink` (`hub/src/agent_server/skill_policy.rs:32,144`). `RunReadSkill` already makes read capabilities data-driven. `Risk` + typed confirmation + `HumanApproval` is the policy skeleton.
- What blocks it / must be replaced:
  - Two parallel catalogues: closed `ReadTool` enum (compile-time, 5-place wiring, `triage/tools.rs`) and runtime `SkillSummary`. Read tools like `docker_logs` are not manifests, so the model sees two vocabularies.
  - `ActionRequest.skill: SkillName` + `args: BTreeMap<String,String>` + `risk` cannot express non-skill capabilities (hub-side effects, config change, composite/multi-step, preconditions, expected side effects, revert). `RevertSpec` exists in the manifest but is never read by engine/hub (grep `revert` in `domovoi-engine/src`, `domovoi-hub/src` finds nothing).
  - `AgentLink` has fixed verbs; `Hello.capabilities` is a dead `Vec<String>`.
  - Policy is scattered: per-host disable in hub, risk checks in `ActionRequest::approve`, `applies_to` in `proposals::check`, loop guard in executor, agent re-validation (`domovoi-agent/src/requests/skills.rs:98`). No single "may this principal invoke this capability on this target now" decision point or decision record.
  - INV-9 wording ("actions come only from ActionProposal{skill,args}") and `ReadTool` closed-enum gates would need an ADR amendment: "closed set" becomes "closed schema, open registry".
- RECOMMENDATION: keep manifest + hash + plan-hash + agent re-validation; generalise `skill` to a `CapabilityRef` and put one `PolicyDecision` function (pure, in core/engine) in front of propose, approve and dispatch; fold read tools into the registry as `Risk::Read` capabilities with hub-side or agent-side providers.

### 7.3 Knowledge store (facts with provenance)
- Existing base: `NoteRepo` + FTS5 (`knowledge.rs`), `FactRepo`/`DigestFact` (typed, per host, time-stamped), host profile JSON in settings with a diff (`profile/diff.rs`), `Baseline` (learned expected containers/units/ports, with `learned_at`/`last_seen_at`), `EntityHistory` on incidents, `triage_cache`, `Verdict` on resolved incidents used as "similar incidents" (`context.rs:168-194`).
- What blocks it: facts are either free markdown (notes), untyped JSON blobs (profile in `settings`), or digest facts with no source/author/confidence/validity/supersession; no link from a fact to the signal/tool output/model turn that produced it; the model can read notes but nothing writes model-derived findings back (by design, INV). `NotesSearch` is the only retrieval path (FTS only, no scoping beyond host). `settings` is being used as a document store (`host.<id>.profile`, `host.<name>.skills`).
- Must be new: `fact(id, subject, predicate/kind, value, source{tool|check|human|model}, observed_at, valid_until, supersedes, confidence)` table and port; keep `notes` as human-authored runbooks, stop using notes as a machine-output container (`generated` notes).

### 7.4 Append-only experience log
- Existing base: `audit_log` has the right guarantees (triggers reject UPDATE/DELETE/replace, `0001_init.sql`), `incident_events` is append-ordered by `seq`, `AuditEvent{at,actor,kind,subject,detail}`.
- What blocks it: `audit_log` is for security audit (actions, auth, settings), written best-effort; `incident_events` cascades on incident delete and is purged at 90 d (`purge.rs:79`) and is not append-only at the DB level; `kind` is free text, `detail` untyped JSON; no outcome/lesson linkage (what was tried, whether it worked, on which host/check) and no schema version. Cache entries are keyed by evidence digest, not by outcome.
- RECOMMENDATION: a separate `experience` table (seq, at, run_id, incident_id, host, check, event kind enum, typed payload, schema_version) with the same triggers; written by the run writer in the same transaction as the run state change; derive "similar past incidents" and cache validity from it; keep audit_log for who/what authorised.

## 8. Abstractions: retain / refactor / replace / remove

Retain
- Hexagonal split, ports, `domovoi-testkit` fakes, `Clock`, arch-check and gate tests (INV-1..12). Low coupling, strong enforcement, fakes make new phase logic testable.
- Pure `decide`/transition functions + single-writer tasks (ADR 0011/0019) and store-guarded `update_from`.
- `PlanHash`, `HumanApproval` (move-only token, constructor-restricted), `Principal`, `Risk` and destructive-needs-typed-confirmation.
- `SkillManifest`/`SkillSummary`/`ParamSpec`/`validate_args`, agent-side re-validation, runner as the only spawner.
- `Redactor` on every outbound path; admission gate concept (one place for model caps); `LlmProvider` port and adapters.
- Layered config with provenance and schema docs.

Refactor
- Triage service: split into (a) `ModelSession` (chain, usage, deadline, history), (b) context builder, (c) report parser/proposals, (d) scheduler/queue. Make queue rehydrate from the DB at start (not only on lag).
- Action executor/recheck: make verification a persisted step (state `Verifying` with due time) resumed on start; reconcile `Dispatched` with the agent by action id instead of failing it blindly (needs agent-side idempotency check; see 9).
- Hub app layer: extract shared operations from `api/*` and `auth/interactions/*`; move `SkillPolicy` decisions into a policy module the engine can call (it already sits behind `PolicyLink`, so the seam exists).
- `ActionRequest`: generalise skill + args to capability reference + typed args + preconditions; keep the transition methods.
- Timeline: promote `triage.*` kinds to a typed enum (serde-tagged) in core; keep the table, change `kind` from free text to a validated set.
- Rolling windows (admission, loop guard) to be reconstructable from the store.
- `DomainEvent` carries a full `Incident` clone per event; fine now, but phase events should carry ids and let consumers load.
- Doc drift list in section 1.

Replace
- `ReadTool` closed enum + `ReadToolCall` + `tool_schema` + agent `match` -> registry-driven read capabilities (keep typed bounds via `ParamSpec`).
- Stringly timeline control flow (`queue.rs:189-221`, `incident_context.rs:26`) -> explicit run/step records.
- `AdvisoryKind` SQL CHECK + enum for every hygiene finding -> a kind registry (or drop the CHECK) so adding finding types does not need table rebuilds.
- `settings` table as document store for machine-derived data (`host.<id>.profile`) -> knowledge store.
- `PROPOSALS:` text protocol in follow-ups -> structured output through the same report/proposal path.
- Restart-everything-on-settings-save for runtime-only knobs (caps, budgets, prompts) -> live-reloadable policy handle.

Remove / consolidate
- `Hello.capabilities: Vec<String>` (dead) unless the registry repurposes it.
- `RevertSpec`/`confirm_window` if the target will not implement revert; otherwise implement (it is dead data today).
- Duplicate fallback loops in followup/chat (after `ModelSession`).
- ADR 0016 text if the experience log supersedes it; otherwise implement as consumer of the log.

## 9. Unresolved questions

1. Does the agent make `run_skill` idempotent by `action` id (so a hub can safely re-ask after restart), or can a skill be mid-flight or finished while the hub marks it failed? Read `domovoi-agent/src/requests/skills.rs` and `runner` for result retention; not done.
2. How large can real triage prompts get with the CLI provider (inlined prefetch) vs tool-capable providers? No total cap exists; no measurement in repo (`crates/domovoi-engine/tests/drills` not read).
3. Are `incident_events` rows ever written outside the manager (e.g. by delivery/dispatcher)? Checked `append_event` callers only in the manager path; a grep for callers outside `manager/` was not exhaustive.
4. What does `domovoi/src` supervisor do with in-flight triage/action work on a settings-triggered restart (`hub run` drain), and is the 10 s triage grace (`serve.rs:106-110`) enough for a 90 s run? Not traced.
5. Why `actions.insert` from triage bypasses the writer: intentional (ADR 0019 is silent about creation) or an oversight? No ADR text covers it.
6. Multi-hub or HA is out of scope in ADR 0003; whether the target needs more than one active runner per data dir is unspecified (HubLock enforces one process, `runtime/lock.rs`).
7. Webhook-only hosts (`HostKind::External`) have no agent; how phases would execute (or not) for them is undefined in current code.
8. Performance headroom of the single manager at the target scale was not measured; clone-per-command (`actor.rs:243`, `decide.rs:442`) is the first thing to profile.
