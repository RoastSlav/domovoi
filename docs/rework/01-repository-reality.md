# 01. Repository reality (baseline commit 64117f0)

Snapshot of baseline 64117f0; decisions live in 03; this file records facts only.

Evidence-based map of the current implementation. Sources: `investigations/00-inventory.md` and reports A to G (referred to as INV, A, B, C, D, E, F, G), spot-checked against the code (see "Corrections"). INFERENCE and STATIC mean code reading only: nothing was executed and no failing test reproduces the item. Paths are repo-relative. FACT unless marked INFERENCE. Line numbers are as reported at 64117f0.

## 1. Summary

- Domovoi is a self-hosted server overseer: one Rust binary (`domovoi`) is hub, agent and CLI; a React SPA is embedded in the hub.
- Agents (one per guarded host) run built-in checks (14 host checks, 4 container checks) and emit signals over an outbound WebSocket to the hub. Webhooks and probes also produce signals.
- The hub correlates signals into incidents (single-writer manager), applies notification policy, delivers cards to Discord and the web UI, and runs LLM triage.
- Triage uses a closed read-only tool set (11 `ReadTool`s) through Anthropic, OpenAI-compatible or CLI providers, with an admission gate (hourly cap, monthly budget). Output is a report plus proposed skill actions.
- Actions are host-owner-written skills (`skill.toml`), risk read/write/destructive. Only an authenticated human can approve (`HumanApproval`, INV-1; enforced by a call-site text scan and by the type having no `Deserialize`, not by compile-time sealing, see section 8). The hub dispatches; the agent re-validates (catalog, `allow_risk`, args, plan hash) and runs through the runner (argv only).
- After a successful skill the hub runs a verification re-check and sets the incident verdict.
- Persistence is SQLite (WAL, sqlx, 5 migrations); the audit log is append-only by trigger. FTS5 search over notes and incident titles.
- Security posture: single admin account, Argon2id, cookie session + CSRF, Discord allowlist, enrolled ed25519 agents with pinned hub certificate; architecture rules enforced by `cargo xtask arch-check` (Cargo edges plus line-based source scans) and gate tests run by the normal test suite.
- Not present: any phase/run entity for remediation, capability registry beyond skills, knowledge facts with provenance, autonomy/policy engine, operations/orchestration UI, browser E2E tests.
- Size: 13 crates + xtask, about 60k non-test-file LOC (inline test modules included); about 941 Rust test attributes; 69 web test files.

## 2. Crates

LOC = non-test `src/` files, inline `#[cfg(test)]` included (INV method; re-measured for all 13 crates, values identical). Tests = `#[test]`/`#[tokio::test]` attribute counts. Dependency direction is enforced by `xtask/src/arch/rules.rs:19-145`; Cargo edges match ARCHITECTURE.md section 4 (A s1).

| Crate | Responsibility | Internal deps (normal) | LOC | Tests | Key entry points |
|---|---|---|---|---|---|
| `domovoi` (bin) | Composition root: hub, agent, CLI (`init`, `upgrade`, `check config`, `selftest`) | agent, config, core, hub, runner | 2104 | 36 | `crates/domovoi/src/main.rs`, `init.rs` (SystemClock `:135`, tracing, metrics `:58-95`), `cmd/`, `local_link.rs` |
| `domovoi-core` | Pure domain types, state machines, ports | none | 6691 | 102 | `incident/{state,transition}.rs`, `action/{request,transition,approval}.rs`, `triage/{tools,tool_call,report}.rs`, `skill/manifest.rs`, `ports/` |
| `domovoi-proto` | Wire protocol: envelope, 23 message kinds, versions | core | 1324 | 22 | `envelope.rs:18`, `messages/mod.rs:31,78-125`, `version.rs:17`, `auth.rs` |
| `domovoi-engine` | Application services, no I/O: manager, notify, triage, actions, digest | core | 8457 | 202 | `manager/actor.rs`, `manager/decide.rs`, `triage/{mod,run,queue,admission,agent}.rs`, `actions/{mod,writer,executor,recover,recheck}.rs` |
| `domovoi-store` | SQLite repos + audit, migrations, backup, purge, FTS | core | 2533 | 15 | `db.rs`, `repos/`, `backup.rs`, `purge.rs`, `fts.rs`, `migrations/0001..0005` |
| `domovoi-hub` | HTTP API, SSE, SPA, auth, ingest, agent server, runtime wiring | chat, checks(net,sources), config, core, engine, llm, proto, store | 17137 | 248 | `runtime/mod.rs:171-347` (`HubRuntime::assemble`), `router.rs`, `api/mod.rs:~305`, `agent_server/`, `auth/`, `ingest/`, `settings/`, `profile/` |
| `domovoi-agent` | Outbound link, scheduler, spool, request server with re-validation | checks(host,net,sources), config, core, proto, runner | 4569 | 51 | `runtime.rs:219-227`, `link/`, `scheduler.rs`, `spool.rs`, `requests/skills.rs`, `requests/tools.rs` |
| `domovoi-checks` | Built-in checks, probes, webhook parsers | core | 6525 | 79 | `framework/registry.rs`, `commands.rs`, `containers/`, `sources/` (features host/net/sources) |
| `domovoi-runner` | Skill catalog + sandboxed process execution; only crate that spawns | core | 936 | 32 | `skill_run.rs:24`, `exec/spawn.rs:41`, `catalog/` |
| `domovoi-llm` | Anthropic, OpenAI-compatible, CLI providers (streaming assembled internally) | core | 1741 | 24 | `factory.rs`, `anthropic.rs:72-85`, `cli.rs:94-96,192`, `http.rs:135-146` |
| `domovoi-chat` | Discord gateway and notifier; card to message | core | 1485 | 26 | `discord/components.rs:34-47`, `discord/events.rs:131-141` |
| `domovoi-config` | Typed hub/agent config, env override, secret indirection, provenance | core | 2845 | 58 | `hub/mod.rs:28`, `hub/llm.rs`, `agent.rs:18`, `layered.rs` |
| `domovoi-testkit` | In-memory ports, FakeClock, ScriptedLlm, recording gateway (dev only) | core, proto | 4244 | 16 | `clock.rs`, `llm.rs`, `link.rs:71`, `repos/mod.rs:75`, `contracts/` |
| `xtask` | arch-check, ci, openapi, docs-config, docs-links | none | 2093 | 30 | `xtask/src/main.rs`, `arch/rules.rs`, `ci.rs:9-156` |

- FACT: `checks` has a third feature, `sources` (`domovoi-checks/src/lib.rs:14-19`), not in ARCHITECTURE section 4.
- FACT: engine has `llm` and `testkit` as dev-dependencies only (legal under R1).
- FACT (INFERENCE on meaning): hub-internal coupling is not covered by arch-check: `auth/interactions/*` imports `crate::api::{hosts,profile,system}`, `ApiState`, `api::incidents::latest_triage`; `api` imports `auth`. HTTP handlers double as the application-service layer (A s1).
- FACT: the hub holds real logic: skill policy (`agent_server/skill_policy.rs`, 446 lines), host profile (`profile/*`, ~900 lines), settings layering/apply (`settings/*`, ~2k), delivery bookkeeping (`runtime/delivery.rs`, `dispatcher.rs`), chat command semantics (`auth/interactions/commands.rs`).

## 3. Runtime topology

All in-process tokio tasks. No broker. Flow is event bus plus DB. One hub process per data dir (`hub.lock`, `runtime/lock.rs`).

### 3.1 Hub

| Task | Spawn site | Role |
|---|---|---|
| IncidentManager (single writer) + 30 s Tick | `engine/src/manager/actor.rs:48-52,280-309` | Owns `active: Vec<Incident>`; all mutations as `Command` (`manager/command.rs:12-99`); `decide()` pure (`decide.rs:435-499`); applies writes, then publishes. Reloads active set at start (`actor.rs:80,118-135`) |
| Triage service | `engine/src/triage/{mod,queue}.rs` | One `serve` task (bus to queue), N workers (default 2), `retry_deferred`; in-memory `VecDeque<Incident>` (cap 64) and `deferred` vec |
| Action service (single writer + executor) | `engine/src/actions/mod.rs:205-230` | `writer::run` and `executor::serve` joined in one task; executor spawns one task per approved action (`executor.rs:32-41`), `running` claim set (`:107-125`), up to 8 parallel |
| Dispatcher | `runtime/dispatcher.rs:128,168,286`, `delivery.rs:368` | Consumes bus; per-incident ordering via oneshot-chain `Sequencer` (`:23-66`); `recover` queues a card refresh for every open confirmed incident; delivery retry loop |
| Schedulers | `runtime/schedulers.rs:85-129,303-381` | Minute tick (dispatcher tick, `ExpireDue`, digest), advisories, daily retention 03:30, backup 04:00, heartbeat |
| Update check | `runtime/updates.rs:233` | 1 min after start then every 12 h |
| Profile scheduler | `profile/mod.rs:298` | Refresh at most one host profile per minute |
| Agent server | `runtime/serve.rs:46-64`, `agent_server/mod.rs:164-172,266` | Accept loop, task per connection, session loop (`session.rs:46`), presence loop (`watch_known_hosts`) |
| Web server | `runtime/serve.rs:67` | axum with graceful shutdown (15 s drain, `serve.rs:106-110`) |
| Chat gateways | `runtime/serve.rs:74-86` | One task per gateway (Discord) |
| Ad hoc spawns in handlers | `api/chat.rs:133`, `api/incidents.rs:441,478` | chat answer, follow-up question, retry triage (outside the queue; still passes admission) |

- Bus: `broadcast::Sender<DomainEvent>` capacity 4096 (`runtime/mod.rs:175`). `DomainEvent{kind, incident (full clone), notify}` (`engine/src/events.rs`). Lag handled per consumer by a counter and a reload (`triage/queue.rs:86-93`, `actions/executor.rs:62-70`, `dispatcher.rs:143-150`).
- Hand-offs: signal source -> `SignalSink::submit` -> manager mpsc (bounded `COMMAND_QUEUE`, `manager/handle.rs`). Incident confirmed -> `TriageRequested` event -> queue -> worker -> `Command::AttachTriage` -> `TriageCompleted` -> dispatcher edits card. Proposal -> `ActionRepo.insert` -> card with chat token -> human click -> `ActionHandle.approve` (mpsc + oneshot) -> `ActionApproved` -> executor -> writer commands -> `AgentLink.run_skill` -> `ActionChanged`.
- Timer jobs are DB polls: approval expiry (`ExpireDue` each minute), re-notify and quiet hours (`dispatcher.tick`), digest (`digest_last_sent_at` setting).
- Per-command the manager clones the active set twice (`actor.rs:243`, `decide.rs:442`) (A s2).
- Settings save triggers a supervised in-process runtime restart with rollback (`settings/apply.rs`); INFERENCE: all in-memory worker state is dropped on each save (A s6).

### 3.2 Agent

| Task | Site | Role |
|---|---|---|
| Link | `link/mod.rs:149,217` | Dial out, handshake, session loop, backoff 1 s to 60 s +-20%, reset after 60 s stable (`link/backoff.rs:6-12`); pings every `heartbeat_seconds` (0 means 10 s), dead after 3 unanswered (`link/mod.rs:39,230-238`) |
| Scheduler | `scheduler.rs:216,305` | Runs due checks, max 4 concurrent (`:186`); hourly digest facts (`:35`) |
| Request server | `requests/mod.rs:297` | Serves `run_tool`, `run_skill`, `run_check`, `get_logs` after re-validation; 4 concurrent, queue 32, then `busy` (`requests/mod.rs:39,161`); deadline per request, `kill_on_drop` (`:238,254`); request tasks independent of the link (`:314-319`) |
| Spool | `spool.rs:1-10` | JSONL spool for signals; 10 000 batches / 8 MiB cap |
| Relay | `runtime.rs:219` | Optional local webhook relay |
| Local mode | `agent/src/local.rs`, `agent_server/mod.rs:52,187` | Single-box: same handshake over in-memory transport, session hint `"local"` |

### 3.3 Live updates to browsers

- SSE `GET /api/v1/events` (`api/events.rs:88`): `incident` and `action` events only; payload `UiEvent{kind,event,incident_id,action_id?,status?}`; no `id:`, no replay; skips `TriageRequested`, `TriageDegraded`, `ActionApproved` (`events.rs:48-52`); a slow client silently loses events (`:76-80`). Keep-alive only.

## 4. Domain model and state machines

### 4.1 Incident (`domovoi-core/src/incident/`)
- `IncidentStatus` = Pending | Open | Acked | Muted{until} | Resolved (`state.rs:17`). Fields `pub(super)`; only `new_pending` and `apply()` mutate (`state.rs:78`, `transition.rs:119`). Pure `(incident, event, policy, now) -> changes`. `Dismiss` exists (`transition.rs:43,167`) although ARCHITECTURE 5.3 diagram omits it.
- Carries nullable `verdict`, `history`, `members`, `source` (`state.rs:78-110`). No field for triage state, plan, phase or linked actions.
- Writer: single (IncidentManager). Even triage results, follow-ups, verdicts and tool-use records go through it (`Command::AttachTriage|SetVerdict|RecordFollowUp|RecordToolUse|ClearFollowUps`).
- Persistence: one command applies writes one by one with no DB transaction (`actor.rs:142-150,174-181,254-262`); on failure the cache is reloaded, earlier writes stay. Only signal + link insert is atomic (`store/repos/incidents.rs:67-98`).

### 4.2 Signal
- `Signal{source, host, check, target, status Firing|Resolved, severity, summary, Evidence, class Normal|Immediate}`; `Fingerprint = hash(host, check, target)` (`core/src/signal/`, `fingerprint.rs`). `SourceKind` = BuiltIn | Webhook | Probe | Agent. Checks: trait `Check` with hard-coded `Registry::build` (`checks/src/framework/registry.rs`).

### 4.3 Action and approval (`domovoi-core/src/action/`)
- `ActionStatus` = Proposed | Approved | Denied | Expired | Dispatched | Rejected{reason} | Succeeded | Failed{reason} | Verified | VerifyFailed (`request.rs:15-42`). No unknown/indeterminate outcome.
- `ActionRequest{incident, host, skill, risk, args: BTreeMap<String,String>, plan_hash, ...}` (`request.rs:130-146`); transitions are methods (`transition.rs:65-150`); `approve` takes `HumanApproval` by move (`HumanApproval::new` and `restore` are `pub` in `domovoi-core`, `core/src/action/approval.rs:54`); destructive needs web typed confirmation equal to the skill name (`transition.rs:75-84`, `auth/approval.rs:20`).
- `PlanHash = sha256(skill, manifest hash, host, canonical args)` (`approval.rs:13-27`); manifest hash covers the serialized manifest, not the entry script (`manifest.rs:145-151`).
- Writers: approvals/denials/expiry/dispatch/complete/verified go through one writer task (ADR 0019; `actions/writer.rs:35-73`) with store guard `UPDATE ... WHERE json_extract(status,'$.type') = ?` (`store/repos/actions.rs:528-557`; port `update_from`, `ports/repos/actions.rs:48`). NOT single-writer for creation: triage inserts and rotates tokens directly (`triage/proposals.rs:115,138`, `triage/followup.rs:252`).
- `record_from` = guarded update, then `audit.append`, then publish; audit failure only logged (`actions/mod.rs:127-153`).
- `Verdict::Fixed` is set by a separate manager command after verification (`actions/recheck.rs:92-98`); no cross-entity atomicity.

### 4.4 Triage (no entity; events in the timeline)
- `TriageReport{headline,summary,cause,evidence,verdict,next_step,manual_steps,proposals,details}` (`core/src/triage/report.rs:52`); `TriageOutcome` = Report | Cached | Skipped | Failed (`:139`).
- Persisted as timeline entries in `incident_events` with kind strings `triage.report|cached|skipped|failed|tools|followup`, `followup.cleared`, `human.action`, `human.dismissed`, `status.changed`, `severity.changed`, `verdict` (`manager/annotate.rs:44-90,200`, `triage/history.rs:8-13`, `decide.rs:152ff`, `dismiss.rs:48-58`).
- Control flow reads those strings: retry, failure-chain length and reload filter scan `entry.kind` prefixes (`triage/queue.rs:189-221,249`); `incident_context.rs:26` filters `starts_with("triage.")`.
- Writer: manager (outcome attach); queue and counters are in memory.

### 4.5 Notes, profiles, baselines, facts, acks, advisories
| Item | Shape | Writer |
|---|---|---|
| Note | `{id, host?, title, body_markdown, tags, updated_at}`, FTS5; no version/history, hard delete (`core/src/note.rs`) | Web API (`api/notes.rs:171-181`: read-then-upsert, last-write-wins), notes import, profile refresh; not single-writer |
| Host profile | Generated note tagged `host-profile`/`generated` with stable id, plus JSON in `settings` under `host.<id>.profile` (`profile/mod.rs:226-237`) | Profile scheduler/refresh (hub) |
| Baseline | `(host, kind, key, criticality, learned_at, last_seen_at, managed)` (container/unit/port) | Checks/learn path; used only to suppress or flag |
| DigestFact | `(host,key,label,value,unit,observed_at)`, hourly per agent; no source/confidence | Agent digest messages via session (`agent_server/session.rs:101-107`) |
| AckRule / MaintenanceWindow | Suppress notifications only (`engine/src/suppress.rs`) | Web API, chat commands |
| Advisory | Closed enum mirrored by SQL CHECK (`core/src/advisory.rs:10-32`; rebuilt in 0003, 0004) | Schedulers/runtime |
| NotifyState | per-incident notifier memory (0002) | Dispatcher |

### 4.6 Chat
- Incident follow-up: stored in the timeline (`triage.followup`), replay capped 10 turns / 16 KiB (`triage/followup.rs:48-51`). General chat: history supplied by the client each call, 20 turns / 16 KiB (`triage/chat.rs:23-26`, `api/chat.rs`; client keeps it in `sessionStorage`). Both use the ReadTool loop and the admission gate; neither can approve. Follow-up proposals use an out-of-band `PROPOSALS:` text marker (`followup.rs:20,120-140`).

## 5. LLM path

- Port `LlmProvider{id, model, supports_tools, complete}`; `Part::{Text,ToolCall,ToolResult}`; `LlmError::{Transport,RateLimited,Auth,InvalidResponse,Unavailable}` (`core/src/ports/llm.rs`). Non-streaming port; adapters stream internally. CLI provider reports `supports_tools() == false` (`cli.rs:192`) and gets prefetched tool output inlined (`triage/run.rs:208-239`, `triage/tools.rs:185-205`). CLI preset passes `--tools ""` (`cli.rs:94-96`).
- ReadTool catalogue: closed enum of 11: DockerPs, DockerLogs, DockerInspect, DockerEvents, HostOverview, Journal, ListeningPorts, SmartReport, IncidentHistory, NotesSearch, RunReadSkill (`core/src/triage/tools.rs:8-21`). Parsed to bounded `ReadToolCall` (`tool_call.rs:265`); unknown names yield an error result. `RunReadSkill` accepts any host skill with advertised `risk == Read` (`engine/src/triage/tools.rs:149-159`). Wired in five places: core enum, typed args, JSON schema, engine dispatch (`engine/triage/tools.rs:95-162`), agent handler (`agent/src/requests/tools.rs:70-157`) plus proto `RunTool`.
- Triage run (`triage/run.rs:54-132`): admit -> latest signal -> evidence digest -> cache lookup `(fingerprint, evidence_digest)` within `cache_days` (default 7), requires a resolved prior incident (`run.rs:136-156`) -> build context -> provider chain -> parse report (one repair retry `:265-287`) -> redact -> `proposals::create` -> cache put -> attach via manager.
- Loop (`triage/agent.rs:130-211`): `max_tool_calls` 15, `max_seconds` 90 (checked after each turn), `max_output_tokens` 2000; on exhaustion "Budget exhausted" plus one more turn. Defaults in `triage/mod.rs:96-97`.
- Admission gate (`triage/admission.rs`, PR #112): `admit(shared, Purpose)` checks monthly budget (UsageRepo month total) then rolling-hour deque of run starts in memory. Defaults: `per_hour_runs` 6, `reserved_for_incidents` 2 (`config/hub/llm.rs:182-193`), `concurrency` 2. `Purpose::Question` leaves reserved slots free; follow-up/chat use `try_acquire` on a semaphore (second concurrent question is Busy; `followup.rs:150-171`, `chat.rs:142-156`). Triage refusal defers a retry to when a slot frees (`run.rs:59-79`, `queue.rs:94-125`).
- Budget read fails open (`admission.rs:52`); a start is counted before the call and never refunded; the window is not rebuilt at start (`Limits::default()`), so a restart (including each settings save) resets the hourly cap.
- Context assembly: 7 parts each cut to 4 KiB (`context.rs:15,63-70`); tool results redacted and cut to 8 KiB (`tools.rs:31,87-93`); chat overview 8 KiB, question 4 KiB, turn 2 KiB, history 16 KiB (`chat.rs:20-35`). No total prompt cap (INFERENCE from the constants: roughly 170 KiB reachable). Includes similar incidents via `incidents.search(query, 20)` with no host filter (`context.rs:168-194`), host notes and profile note (`:200-244`), skill catalogue text from the agent (`:248-267`).
- Fallback: `chain = [primary] + fallback_models + secondary` (`triage/mod.rs chain()`, `runtime/mod.rs:367-400`); whole-attempt restart per provider, any error counts (`run.rs:168-193`). Engine ignores `RateLimited{retry_after}` (adapter parses it, `http.rs:135-146`). Three consecutive failures raise `TriageDegraded` (`run.rs:290-312`). A failed triage still leaves a raw card (gate `triage_failure_still_leaves_a_card`).
- Safety: redactor on tool results, context and report (`tools.rs:87`, `context.rs:71`, `run.rs:100`); proposals become `Proposed` only after `validate_args` against the host's current `SkillSummary` and `applies_to`, always for `incident.host()` (`proposals.rs:37-53,128-130`); provider error text redacted and clipped to 500 chars (`followup.rs:56-62`).
- Duplication: three near-identical provider-loop implementations (`run.rs:168-193`, `followup.rs:198ff`, `chat.rs:181ff`); `Shared` holds 15 deps (`triage/mod.rs:100-130`). Tunables (`PART_BYTES`, `MAX_RESULT_BYTES`, `TOOL_DEADLINE`, `DEGRADED_AFTER`, proposal TTL 1 h `triage/mod.rs:54`) are compile-time constants.

## 6. Protocol (hub and agent)

- Transport: WebSocket over TLS 1.3; agent dials out; hub certificate self-signed, pinned by SHA-256 (`docs/adr/0003`; `agent/src/link/tls.rs:176-179`). JSON text frames; envelope `{v, id (UUIDv7), kind, reply_to?, payload}`; `MAX_FRAME_BYTES` 1 MiB (`proto/src/envelope.rs:11`); binary frames error; hub drops a link above 100 frames/s (`agent_server/ws.rs:16`). Single-box uses an in-memory transport.
- Auth: TLS authenticates the hub only. Application layer: `hello` -> `challenge` (32 bytes) -> ed25519 signature over `"domovoi-agent-auth-v1\0" || nonce || agent_id || RFC 9266 channel binding`, `verify_strict` (`proto/src/auth.rs`, `agent_server/handshake.rs:150-190`). Enrolment token single-use, hashed, 15 min, 1 s delay on a bad token; new enrolment revokes the previous agent (`handshake.rs:84-116`); `revoked_at` checked each handshake (`:162`).
- Versioning: `PROTOCOL_VERSION` 1.1 (`version.rs:17`). `compatible(agent, hub)` = same major, `agent.minor <= hub.minor <= agent.minor + 1` (`:22-26`, verified). `incompatible` is final on the agent (`agent/src/error.rs:93`, `link/mod.rs:163`). Second gate: `MIN_AGENT_VERSION = MIN_HUB_VERSION = 0.0.5` by release (`:49-60`, `handshake.rs:192-215`). `Welcome` carries `hub_version` only; agent never learns the hub's protocol minor. Practice: additive optional fields without a minor bump (ADR 0018).
- Message kinds (23, `messages/mod.rs:78-125`): enroll, enrolled, hello, challenge, challenge_response, welcome, reject, ping, pong, signals, signals_ack, digest_facts, run_tool, tool_result, get_logs, logs, run_skill, skill_result, run_check, check_result, config_update, error, unsupported; unknown kind decodes to `Unknown` and is answered `unsupported`. Unknown payload fields ignored, except `ReadToolCall` inside `run_tool`, parsed strictly (`messages/work.rs:301-322`).
- `Hello{agent_id, protocol, agent_version, facts, capabilities, skills, checks, docker}` (`handshake.rs:164-174`); agent sends `capabilities: Vec::new()` (`agent/src/link/handshake.rs:47`, verified); no consumer in the hub. Skills catalogue is read once at agent start (`runtime.rs:138`) and held per session in memory; no re-announce.
- Spool: signals only; JSONL, `batch_seq` survives restarts, replay on reconnect, dropped on `signals_ack`; evicts info firing first, resolved last (`spool.rs`). Hub acks after `sink.submit` succeeds (`session.rs:90-100`). Results of requests are not buffered.
- Liveness: only the agent pings; hub answers pongs and records `last_seen` on any frame (`session.rs:60,110`); presence loop raises `agent.down`/`host.down` after three silent heartbeats (`presence.rs`). INFERENCE: a half-open socket stays Connected until TCP fails (`registry.rs:106-110`).
- Action dispatch (`executor.rs`, `agent/src/requests/skills.rs`): reload action still Approved (`:219-232`) -> in-memory claim -> `revalidate` against advertised skill (`:325-341`) -> loop guard 3 per skill per host per hour (`:297`, `actions/mod.rs:50`) -> recompute plan hash, persist `Dispatched` -> `wait_for_agent` up to 5 min (`actions/mod.rs:51`) -> `run_skill{action_id, skill, args, plan_hash, deadline = timeout + 30 s}` (`work.rs:120-131`) -> agent ladder: skill in local catalog, risk in `[policy] allow_risk` (default read+write, destructive opt-in; `config/agent.rs:68,269`), args vs local manifest, recompute plan hash -> runner `build_spec` argv, `SandboxExecutor` (relative paths and shell-named programs refused, cleared env, process group kill, optional `run_as`; no namespaces/seccomp/cgroups; `exec/mod.rs:20-57`) -> `skill_result` -> `Succeeded`/`Failed` -> verify re-check.
- Failure semantics: no retries on the action path; no idempotency key (agent only logs `action_id`, `skills.rs:110`). Link drop mid-action: `LinkError::Transport` -> `Failed(err)`, no verify (`agent_server/link.rs:34-46`, `executor.rs:286-288`). Agent keeps running; its late reply is discarded by the hub (`session.rs:117-146`, `registry.rs:193`). Hub restart: `Dispatched` actions failed with "the hub restarted while this ran; check the host before running it again" (`recover.rs:27,204`). Max 256 pending requests per agent (`registry.rs:19,160`).
- Version-skew hazards (D): a new `ReadTool` variant fails decode of the whole `run_tool` frame on old agents and ends the session (`work.rs:301-322`, `link/frame.rs:32`); a new `Risk` variant in `hello` fails decode on an old hub with no `reject` (`manifest.rs:31` has no `#[serde(other)]`; `handshake.rs:55`, `conn.rs:68-82`); a minor bump to 1.2 would refuse every 1.0 agent finally.
- `hello` size: 1 MiB cap with up to 8 KiB README per skill (`runner/src/catalog/checks.rs:17`).

## 7. Persistence

SQLite via sqlx, WAL, `synchronous = NORMAL`, `foreign_keys = ON`, busy_timeout 5 s (`store/src/db.rs:19-20,88-96`; NORMAL verified). Two pools on one file: writer max 1, reader 4 (`db.rs:21-22,145-162`). Conventions: ids TEXT UUID, timestamps unix ms, hashes BLOB(32), JSON in TEXT, enums as TEXT + CHECK, STRICT tables.

| Table | Kind | Notes |
|---|---|---|
| `hosts`, `agents`, `enrollment_tokens` | mutable | partial unique index one live agent per host; token consumed by `UPDATE ... RETURNING` (`repos/hosts.rs:208-226`) |
| `incidents` | mutable, whole-row overwrite (`repos/incidents.rs:155-179`) | scalar columns + `body` JSON (no schema version); partial unique index one active per fingerprint; `incidents_fts(title, summary_text)` |
| `signals`, `incident_signals` | append, purged 30 d | link has FK on `signal_id` only |
| `incident_events` | append by convention (no trigger) | AUTOINCREMENT `seq`, FK cascade to incidents; the timeline; purged 90 d |
| `actions` | mutable via guarded `update_from` | status is JSON; approval triple with CHECK; `chat_token_hash` UNIQUE; no FK on incident/host; history only in `audit_log` |
| `audit_log` | truly append-only | triggers reject UPDATE/DELETE/REPLACE (`0001_init.sql:133-156`); kept forever; no hash chain |
| `ack_rules`, `maintenance_windows`, `baselines`, `notify_state` | mutable | ack rules got `created_at/by` in 0005 |
| `notes` + `notes_fts` | mutable, no history | external-content FTS with triggers |
| `triage_cache` | upsert, purged 90 d | key `(fingerprint, evidence_digest)` |
| `llm_usage`, `digest_facts` | append, never purged | digest facts hourly per agent |
| `advisories` | mutable | kind CHECK; table rebuilt in 0003 and 0004 |
| `users`, `sessions`, `settings` | mutable | settings also holds host profile JSON and `host.<name>.skills`; `set_many` is a transaction (`accounts.rs:39-52`) |

- Concurrency: only optimistic guard is the action status string. No `version` column anywhere. Writer pool of 1 is the real mutual exclusion for every read-modify-write (`discard_pending` is a deferred tx, `incidents.rs:271-302`) (C INFERENCE). Two-handle race tested (`tests/sqlite/concurrency.rs:11-80`).
- Transactions: store exposes one repo per aggregate; engine composes them with no shared tx (see section 11 D06). `Store::purge` is six independent DELETEs (`purge.rs:330-372`).
- Retention (`purge.rs:287-372`, daily 03:30 `schedulers.rs:353`, `RetentionPolicy::default()` hard-coded): signals 30 d, incident timeline 90 d, triage cache 90 d, resolved advisories 30 d, ended windows, expired sessions. Never purged: incidents, audit, digest facts, llm usage, notes, baselines, actions, notify state.
- Backup (`backup.rs:7-43`, `db.rs:127-144`, `schedulers.rs:353-367`): `VACUUM INTO` `pre-migrate-<time>.db` before pending migrations (never pruned); nightly 04:00 `domovoi-*.db`, newest 7 kept; runs on the single writer connection; not verified, not encrypted, default `<data_dir>/backups`; failure only `tracing::warn!`, no advisory. `secret.key` and `tls/` separate (`docs/book/src/reference/backup.md:12-31`).
- Restore: no command or API; manual copy (`docs/book/src/install/upgrading.md:42`). No post-restore reconciliation.
- Recovery on start: manager reloads all `active` incidents (`actor.rs:119-137`). Actions: executor starts every `Approved`; `fail_abandoned` fails every `Dispatched`; loop guard rebuilt from audit dispatch rows of the last hour; scans retry 3x (`actions/recover.rs:20-53,99-110,139,204-230`). Triage queue is NOT rebuilt. No `integrity_check`/`quick_check` at open. Migrations run at open after pre-migrate backup; failure returns `StoreOpenError::Migrate` (`db.rs:154-157`).
- Search: FTS5 `unicode61 remove_diacritics 0`; query words split on non-alphanumerics, quoted, ANDed (`fts.rs:1-30`): no prefix, phrase, stemming, OR; `_` splits tokens. Notes: `bm25(10,1,1)`, snippet, filter `host_id IS NULL OR = ?`, re-rank in Rust (`repos/notes.rs:129-160`). Incident "similar" search queries title column only (`repos/incidents.rs:340-354`; `summary_text` is `check_key + target`, `:52-57`). Incident list text filter is `LIKE %x%` (`:241-242`). `Store::open` fails with `NoFts5` if FTS5 missing (`db.rs:150-153`).
- Migrations: 5 files, 331 lines (0001 init 282, 0002 notify_state, 0003 and 0004 advisory CHECK rebuilds, 0005 ack rule creator). Forward-only; R13 forbids editing merged ones. Weak FKs: only `agents`, `enrollment_tokens`, `incident_events`, `incident_signals.signal_id`.
- ADR 0016 (suggestions) is Proposed; no code (`AdvisoryKind` has no Suggestion; no engine `suggest` module).

## 8. Security mechanisms

| Mechanism | Implementation | Evidence |
|---|---|---|
| Web auth | One admin (setup once with 24 h single-use token); Argon2id; session 32 random bytes, only SHA-256 stored, cookie HttpOnly, SameSite=Strict, Secure on https; 12 h idle, 7 d absolute | `hub/src/auth/mod.rs:183-247`, `session.rs:9-65`, `password.rs` |
| Login throttle | Per source IP, in memory, 5 failures/min, lockout doubling to 15 min; `trusted_proxies` governs IP | `auth/limit.rs:9-62`, `auth/proxy.rs:11-24` |
| CSRF | Token `SHA256("domovoi-csrf:v1\n" + session_id)`, constant-time compare; `Origin` checked when present; no-cookie requests skip token and rely on `Authed` returning 401 | `auth/session.rs:36-41`, `auth/csrf.rs:43-64`, `api/mod.rs:252-266` |
| Re-auth | `confirm_password` only for secrets, `discord.allowed_user_ids`, `web.trusted_proxies`, `webhooks.*` | `settings/patch.rs:107-117` (verified: `cli.*` absent) |
| Unauthenticated | `/healthz`, `/metrics`, webhooks (own HMAC/bearer, body only, no timestamp), SPA | `api/system.rs:129-157`, `ingest/auth.rs:11-56` |
| Chat identity | Discord numeric user id from gateway event vs `allowed_user_ids`; hub never sees raw button strings | `chat/discord/events.rs:131-141`, `ids.rs:1-8`, `auth/interactions/mod.rs:184-219` |
| HumanApproval / INV-1 | `HumanApproval::new` is called only in `hub/src/auth/` (call sites `auth/approval.rs:21,33`; testkit dev-only); `restore` only in store (`repos/actions.rs:58`); no `Default`, no `Deserialize`. NOT compile-time sealed: both constructors are `pub` in `domovoi-core`, because Rust privacy cannot restrict a constructor to a module of another crate. The restriction is a line-based text scan in `xtask arch-check` for the strings `HumanApproval::new(` and `HumanApproval::restore(` outside allowed path prefixes (`xtask/src/arch/rules.rs:169-175`, `scan.rs:63-116`); a renamed import or a macro evades it. Decision reference: RW-11 | verified by grep and by reading `scan.rs` |
| Destructive | Web only, typed confirmation equals skill name; chat shows "Approve in web UI" | `auth/approval.rs:20`, `chat/discord/components.rs:45`, `buttons.rs:142` |
| Chat tokens | Single-use `chat_token_hash`, cleared on use (`writer.rs:249`); Deny does not check token (`writer.rs:265-279`) | `approvals.rs:28-34` |
| Plan hash | Computed at proposal, recomputed at dispatch on hub and on agent (INV-4); omits entry-script bytes and resolved target identity | `approval.rs:13-27`, `agent/src/requests/skills.rs:98-109` |
| Agent revalidation | Catalog, `allow_risk`, args, plan hash; runner permission checks (not group/world writable) | `skills.rs:62-125`, `runner/src/catalog/checks.rs:141-167` |
| Agent identity | ed25519 + channel binding, pinned cert, enrolment tokens, revocation | section 6 |
| Hub-side skill narrowing | `PolicyLink` hides disabled skills, narrow only | `agent_server/skill_policy.rs:1-8,32,144` |
| Secrets | `SecretString` config; UI-entered secrets ChaCha20-Poly1305 with path-bound AAD, key `<data_dir>/secret.key`; write-only via API; 20 `expose_secret` sites at point of use | `settings/crypto.rs:1-60`, `config/secret.rs` |
| Redaction | Registered secret values + 9 regexes + `key = value`; applied to tool results, context, reports, provider errors, log ring; `strip_env` removes Docker env values | `engine/src/redact.rs:11-27`, `checks/src/containers/mod.rs:167` |
| Audit | `audit_log` row per action transition (actor `user:`/`chat:`/`system:`, plan hash, host, skill, risk, status; approvals add channel; runs add exit code, argv hash, output SHA-256, 4 KiB excerpt); accepted chat interactions write `chat.<kind>`; port exposes append and list only | `engine/src/actions/audit.rs:11-190`, `auth/interactions/mod.rs:156-216` |
| Gate tests | INV-family: read-only tools, no approval without human, no model approve, secrets never reach model, injection creates no actions, settings need session+CSRF, Discord text cannot approve | section 10 |

- Plan hash and approval binding (restated): the hash covers the serialised manifest, not the entry script's bytes; the web approve request carries no hash (`api/actions.rs:104-123` takes `confirm_text` only), so the server cannot tell what the operator saw (D17).
- Sandbox limits: argv-only; what a skill script does inside is trusted (`exec/program.rs`). Redaction gaps (E, INFERENCE from regexes): JSON-quoted `"password": "x"`, URL userinfo, cookies, `Authorization: Basic`, unregistered host secrets.
- Prompt-injection surfaces: logs, journal, inspect, read-skill stdout, signal summaries, other incidents' titles (no host filter), notes (importable), agent-authored skill text, 7-day triage cache (E s3.3).
- ADR 0007 / INV-1 wording: "Only a Human principal can approve" (`ARCHITECTURE.md:320`).

## 9. Web UI (`web/`)

- Stack: React 19, react-router 8 (data router, used for leave guards: `App.tsx:117-119`; the only guard in use is `useLeaveGuard` in the notes editor, `features/notes/useLeaveGuard.tsx:16`, so other forms do not block navigation while dirty), TanStack Query 5, openapi-fetch with generated `schema.d.ts`, lucide behind `Icon`. No chart/graph/virtualisation/animation library.
- Routes (`app/routes.tsx:48-90`): public `/setup`, `/login`; behind `RequireSession` + `Layout`: `/` overview, `/incidents`, `/incidents/:id` (eager), `/chat`, `/approvals`, `/rules`, `/hosts`, `/hosts/:name` (tabs Overview/Profile/Checks/Logs/SSH logins), `/skills`, `/notes`, `/notes/:id`, `/digest`, `/logs`, `/settings`, `/settings/:section`, `/welcome`, `*`; `/dev/design` dev-server only. Nav groups Respond/Fleet/Knowledge/System (`nav.ts:167-220`). No operations, capabilities, policies, audit, notifications or topology routes.
- Shell (`Layout.tsx:36-110`): `HostProvider > PageTitleProvider > Sidebar + AppPalette + TopBar + main`; single column capped by `--content-max` 1200 px; no right-hand panel slot. `useLiveEvents()` called once (`:37`).
- Kit: about 70 components behind barrel `components/index.ts` (status, feedback, hints, buttons, states, layout, data incl. DataTable/Timeline, forms, overlays, CommandPalette). Absent: tree, split pane, drawer/inspector, graph canvas, virtual list, chart/sparkline, notification centre.
- Data layer: only door is `api` (openapi-fetch; CSRF middleware `api/client.ts:25-36`, 401 redirect `:51-62`); lint bans `fetch` outside `src/api` (exception `api/health.ts:19`). Hooks per domain in `api/hooks/`; keys centralised `api/queryClient.ts:5-33`; `staleTime` 10 s, retry once on 5xx only, mutations never retry. Errors become `ApiFailure` via `call()`; words only from `design/messages.ts`. API: 58 operations over 50 paths (openapi.json) = the INV table including `/healthz`, `/metrics`. Only incidents are cursor-paged (50/page); actions fixed `limit: 200`.
- Live layer: single `EventSource` (`api/events.ts:86`), events used only as cache-invalidation hints (`:34-44`), backoff 1 to 30 s, blanket invalidation on reconnect (`:88-93`), "paused" banner after 5 s (`Layout.tsx:90-94`). Hosts, host detail, logs, profile, updates, overview are polled (overview 30 s `overview.ts:7`; hosts 30 s `hosts.ts:15,47`). Toasts for action outcomes and status news derive from diffing refetched data (`useActionOutcomes`, `useStatusNews`; `OWN_CHANGE_MS` 2 s heuristic `incidents.ts:78-87`).
- Mutations: `useActionFeedback` wraps every `useMutation` (`components/useActionFeedback.ts:243-291`); the lint is file-level: a file passes if it imports `useActionFeedback`, `InlineMessage` or `InlineError` anywhere, so one wrapped mutation excuses an unwrapped one in the same file (`web/eslint/domovoi-rules.js:10,128`); four files carry a `// feedback-ok:` exemption that does not prove the wrapping. `QueryBoundary` drops a failed refetch when data exists (`QueryBoundary.tsx:143-189`); no stale/refreshing UI (no `isFetching`/`dataUpdatedAt` use).
- State: server state in Query; URL state via `useSearchParams`; prefs in `localStorage` (host, rail, theme, hosts view), chat transcripts in `sessionStorage` (`features/chat/conversation.ts:23-43`). No server-side per-user prefs.
- Guardrails (`web/DESIGN.md:359-401`, `eslint/domovoi-rules.js`, `src/guardrails.test.ts`): raw colour only in `tokens.css`; WCAG AA token-pair test (`styles/contrast.test.ts`); status vocabulary only in `design/tone.ts`; icons via `Icon`; no inline style, native `title`, raw `<button>`; files <= 250 lines; axe after every page test (`test/axe.ts`; colour contrast excluded in jsdom). Budget via `scripts/size.mjs` in `npm run build`: initial JS gzip <= 170 kB (now 154.8), chunk <= 120 kB (largest 110.7), CSS <= 30 kB (14.6). Motion tokens two values (120 and 180 ms), zero under reduced motion (`tokens.css:175-176,398-401`). CSP `default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'` (`hub/src/csp.rs:10`).
- Tests: 69 vitest files; no browser E2E in CI (Playwright only in manual `scripts/screens.mjs`).

## 10. Tests, CI, release, observability

### 10.1 Tests
| Kind | Where |
|---|---|
| Unit and integration with fakes | all crates; engine `tests/{engine,services,notify}/` (largest `services/limits.rs` 1382 lines), hub `tests/hub/`, agent `tests/agent/` |
| Property | `core/tests/fingerprint_properties.rs`, `engine/tests/engine/properties.rs`, `runner/tests/gates.rs:14` |
| Protocol snapshots | `proto/tests/protocol/` + 20 insta snaps; `version.rs:8` pins 1.1; no old-agent fixture test |
| Store contracts | `testkit/src/contracts/*` (18 suites) run on InMemoryStore and SQLite; `store/tests/sqlite/{migrations,concurrency,purge,search,paging}.rs` |
| Gates (5 files) | core (6: closed ReadTool, no approval without human, destructive not from chat, audit has no mutation API, no Deserialize on approvals), engine (6), hub (9), agent (2: `hub_cannot_run_skill_not_in_local_catalog`, `plan_hash_mismatch_never_executes`), runner (shell-injection proptest) |
| Whole process | `domovoi/tests/{single_box,agent_hub,settings_apply,cli}.rs` (unix only); `hub/runtime/tests.rs:62`; `selftest --offline` |
| Drills | `engine/tests/drills/` (6 YAML cases, scored 100, pass 80); live drills manual `drills-live.yml` |
| Testkit fakes | FakeClock, FakeProcessExecutor (no hang/timeout), ScriptedLlm, FakeAgentLink (deadlines recorded, not enforced, `link.rs:~68`), InMemoryStore with `fail_next` (single failure only, `repos/mod.rs:75`) |

- Not covered (F): crash-consistency sweeps, outcome-unknown, agent idempotency, wire compatibility against a released version, upgrade from N-1 database, refusal of newer-schema DB, signal-storm/soak, browser E2E, coverage gate, mutation/fuzz testing.
- `xtask arch-check` (`xtask/src/arch/rules.rs`, `scan.rs`): R1/R2 deps and pure crates, R3/R4 spawn only in runner, INV-1 call sites, R5 testkit dev-only, R7/R9 anyhow, R8 500-line cap, R10 docs, R11 no openssl, R13 append-only migrations. The source rules (R3, R4, R2 path bans, INV-1) are line-based text matches in `scan.rs`, not type-level checks, and they exempt test paths. CI runs the gate tests as ordinary tests, not through arch-check.

### 10.2 CI and release
- `ci.yml` (572 lines): path classifier that fails open; jobs pr-title, web, web-build, docs-links, metadata, rust (fmt, clippy `-D warnings`, nextest workspace, doctests, arch-check `--base`, openapi/config-docs drift, rustdoc, embed-ui), deny, msrv, musl, actionlint, shellcheck, installer (ubuntu 24.04, debian 12), units verify, coverage (main only, no threshold). Plus codeql, scorecard, docs, docker smoke, drills-live (manual). nextest 30 s slow-timeout, terminate after 2 min (`.config/nextest.toml`). Dependabot weekly.
- Release (`release.yml`, ADR 0008): release-plz opens release PR; merge tags and creates release (`:63-83`); then web build, static musl amd64/arm64 with embedded UI, tarball + sha256 + SLSA attestation, GHCR images `domovoi` (distroless) and `domovoi-agent` (alpine). Tag-before-build can leave a release without binaries (recovery via `workflow_dispatch` with `tag`). One shared version for all crates.
- Deploy: `install.sh` (366 lines; verifies SHA-256, idempotent, never writes config or starts service), systemd units, compose files, Unraid template. `domovoi upgrade` swaps the exe and prints restart commands (`cmd/upgrade.rs`); it compares the archive with a `.sha256` file fetched from the same release origin (`cmd/upgrade/package.rs:13-30`), which detects corruption, not a compromised release, and it does not check the release attestation. No agent auto-update. Hub update check `runtime/updates.rs`.

### 10.3 Observability
- Logging: `tracing` + `EnvFilter`, human or JSON, stderr; in-memory `LogRing` for the web logs page, scrubbed by `PatternRedactor` (`domovoi/src/init.rs:58-85`, `log_ring.rs`). No spans or per-incident correlation id.
- Metrics (Prometheus, `init.rs:89-95`, `engine/src/telemetry.rs:5-82`): `domovoi_signals_total`, `_incidents`, `_triage_runs_total`, `_llm_tokens_total`, `_llm_cost_usd_total`, `_actions_total`, `_agents_connected`, `_event_bus_lagged_total`, `_delivery_failures_total`, `_incident_flaps_total`, `_session*`. No writer-queue depth, action latency, approval wait, store latency, reconnect count.
- Health: `/healthz` returns constant `"ok"` (`api/system.rs:133`, verified); no readiness. Self-monitoring: advisories (HeartbeatMissing, TriageDegraded, AgentOutdated, DeliveryFailed, SettingsApplyFailed, StoredSecretUnreadable) and a hub heartbeat ping to an external URL (`schedulers.rs:30`).

## 11. Known defects and latent risks

Severity H/M/L is a judgement from the source reports (INFERENCE where the report says so). Sources: A backend, B frontend, C persistence, D protocol, E security, F testing/ops, V verified directly during this compilation.

| ID | Description | Evidence | Sev | Source |
|---|---|---|---|---|
| D01 | Discord Approve/Deny buttons name no skill, host or args; card text is model prose, so a prompt-injected model writes what the human approves | `core/src/card.rs:32-48`, `chat/discord/components.rs:39-40`, `engine/src/cards.rs:111-157` (no skill/args/proposal references; V) | H | E |
| D02 | `RunSkill` carries no authorization proof; agent cannot tell a human-approved dispatch from any hub-originated one; plan hash is public data | `proto/src/messages/work.rs:120-131`, `agent/src/requests/skills.rs:62-125` | H | E |
| D03 | CLI provider `command`/`args` not marked sensitive: admin session can set an arbitrary program run by the hub (and drop `--tools ""`) without password re-entry | `settings/patch.rs:107-117` (V), `config/hub/llm.rs:66-80`, `llm/src/cli.rs:94-96` | H | E |
| D04 | Approved actions re-run at startup (and on bus-lag catch-up) with no age or incident-open check; `dispatch` and executor ignore `expires_at` | `actions/executor.rs:62-105,219-290` (no `expires_at` reference; V), `action/transition.rs:104-118` (V), `recover.rs:123-130` | H | E, C |
| D05 | Agent has no idempotency by `action_id` and no outcome-unknown state; lost reply or hub restart collapses to `Failed` prose | `agent/src/requests/skills.rs:110` (V), `action/request.rs:15-42`, `recover.rs:204`, `agent_server/link.rs:34-46` | H | D, F |
| D06 | Audit and timeline writes are not transactional with state: action `update_from` then `audit.append` (failure only logged); incident writes applied one by one | `actions/mod.rs:127-153`, `manager/actor.rs:142-150,174-181,254-262`, `store/purge.rs:330-372` | H | A, C, E |
| D07 | Triage queue and deferred retries are not reloaded at start (only on bus lag); open untriaged incidents stay untriaged until retry; each settings save restarts the runtime | `triage/queue.rs:66-95,227` (V), `decide.rs:125-148` emits `TriageRequested` only on state change (V), `settings/apply.rs` | H | A |
| D08 | Read-risk is self-declared by the skill manifest; an advertised `read` skill runs unattended with no plan hash | `engine/src/triage/tools.rs:149-159`, `agent/src/requests/skills.rs:129-173` | H | D, E |
| D09 | Verification sleeps in memory; a restart in that window leaves the action `Succeeded` and the incident unverified | `actions/recheck.rs:65` (V), `executor.rs:281-284` | M | A |
| D10 | Admission gate fails open on budget read error; run start counted before the call and never refunded; hourly window not rebuilt at start | `triage/admission.rs:52` (V), `triage/mod.rs` `Limits::default()` | M | A |
| D11 | No total prompt cap (15 results x 8 KiB + context + history); no truncation of earlier messages | `triage/context.rs:15,63-70`, `tools.rs:31`, `agent.rs:130-211` | M | A |
| D12 | Protocol minor rule strands agents: agent must be 0 or 1 minor behind and never ahead; `incompatible` is final (agent stops retrying); a 1.2 bump refuses all 1.0 agents; agent never learns hub minor | `proto/src/version.rs:22-26` (V), `agent/src/error.rs:93`, `link/mod.rs:163`, `handshake.rs:332-339` | M | D |
| D13 | Skew poison pills: new `ReadTool` kills old agent's session; new `Risk` in `hello` makes old hub close without `reject`, agent retries forever | `messages/work.rs:301-322`, `link/frame.rs:32`, `core/skill/manifest.rs:31`, `hub/agent_server/handshake.rs:55` | M | D |
| D14 | SSE has no event ids or replay; broadcast is lossy; host/check/settings events have no push path; client treats events as invalidation hints only | `api/events.rs:48-52,76-80`, `web/src/api/events.ts:34-44` | M | B |
| D15 | `/healthz` is a constant string; a wedged writer still reports healthy; no readiness | `api/system.rs:133` (V) | M | F |
| D16 | Approvals and audit are unauthenticated DB rows: `HumanApproval::restore` trusts three columns; audit append-only only via droppable triggers, no hash chain | `store/repos/actions.rs:58,173-202`, `0001_init.sql:145-153` | M | E |
| D17 | Plan hash omits entry-script bytes and resolved target; web approve request does not carry the plan hash the user saw; hub `revalidate` uses cached hello catalogue | `core/skill/manifest.rs:145-151`, `executor.rs:329`, `api/actions.rs:104-124` | M | E |
| D18 | Hello `capabilities` is dead: always empty, no consumer, yet documented as meaningful | `agent/src/link/handshake.rs:47` (V), `proto/handshake.rs:164`, `ARCHITECTURE.md:253` | L | A, D |
| D19 | Hub never pings; half-open link stays Connected and dispatch ends as timeout after skill timeout + 30 s | `session.rs:60,110`, `registry.rs:106-110`, `executor.rs:344` (INFERENCE) | M | D |
| D20 | Hub-side no re-validation of offered `SkillSummary`; skill catalogue read once at agent start, no re-announce | `agent/runtime.rs:138`, `registry.rs:263` | L | D |
| D21 | Rejected and unauthorised attempts not audited; web approvals lack IP/session; no agent-local execution record; UI-shown content not recorded | `auth/interactions/mod.rs:185-190`, `buttons.rs:142-143`, `agent/requests/skills.rs:110` | M | E |
| D22 | Redactor misses JSON-quoted secrets, URL userinfo, cookies, Basic auth, unregistered host secrets; logs flow to cloud providers | `engine/src/redact.rs:11-27` | M | E |
| D23 | Similar-incident titles enter prompts with no host filter; notes (importable) and 7-day triage cache are persistent injection channels | `triage/context.rs:168-194,200-244`, `triage/run.rs:80-93` | L | E |
| D24 | Login throttle per IP, in memory, shared proxy IP when `trusted_proxies` unset; no MFA; webhook HMAC has no timestamp or nonce; `/metrics` open | `auth/limit.rs:9-62`, `ingest/auth.rs:28-45`, `api/system.rs:142-157` | L | E |
| D25 | Backup: runs on the single writer connection, not verified or encrypted, failure only logged (no advisory), pre-migrate files never pruned, no restore command or reconciliation | `store/backup.rs:7-43`, `schedulers.rs:362-367` | M | C |
| D26 | `synchronous = NORMAL` can lose last commits on power loss; no integrity check at open; no `version` column on any mutable aggregate; notes `replace` and profile refresh last-write-wins | `db.rs:93`, `api/notes.rs:171-181`, `profile/mod.rs:226-237` | L | C |
| D27 | Retention drift: timeline thinned at 90 d while docs say incidents kept; `digest_facts`, `llm_usage`, audit unbounded; retention not configurable | `purge.rs:287-372`, `ARCHITECTURE.md` section 9 | L | A, C |
| D28 | Triage writes `actions` directly (insert, token rotate) outside the ADR 0019 writer | `triage/proposals.rs:115,138`, `triage/followup.rs:252` | L | A |
| D29 | Engine ignores `RateLimited{retry_after}`; fallback restarts the whole conversation; follow-up `PROPOSALS:` text protocol parsed with `unwrap_or_default` | `llm/src/http.rs:135-146`, `triage/run.rs:168-193`, `followup.rs:120-140` | L | A |
| D30 | Stringly-typed timeline kinds drive triage control flow; `incidents.body` has no schema version; advisory kinds need table rebuilds | `triage/queue.rs:189-221`, `incident_context.rs:26`, `migrations/0003,0004` | L | A, C |
| D31 | Container name args to Docker API validated by length only (unverified path-injection risk); skill scripts may shell internally | `core/triage/tool_call.rs:241-258`, `agent/requests/tools.rs:86-96` | L | E |
| D32 | Hub-internal layering: `auth` depends on `api`; handlers are the app layer; `assemble` is one ~180-line function | `auth/interactions/*`, `runtime/mod.rs:171-347` | L | A |
| D33 | Test gaps: no browser E2E, no wire-compat or N-1 upgrade test, no crash-point or fault injection, FakeAgentLink cannot lose a reply, coverage not gated | F s1, s2, s6 | M | F |
| D34 | Frontend: refresh failure invisible; polling vs SSE split makes "Live" dot only cover incidents/actions; actions capped at 200; timeline keyed by index, summary-only; 24 of 94 data-using files use `QueryBoundary` | `QueryBoundary.tsx:143-189`, `hosts.ts:15`, `api/hooks/actions.ts:20`, `Timeline.tsx` | L | B |
| D35 | `RevertSpec`/`confirm_window` exist in manifest but are never read by engine or hub | grep `revert` in `engine/src`, `hub/src` finds nothing (A) | L | A |
| D36 | STATIC (code reading; not reproduced at runtime, not a confirmed defect). Restart catch-up appears to run before any agent reconnects (`ActionService::spawn` at `hub/src/runtime/mod.rs:230`, before the listeners serve); `revalidate` reads the live link's skill list, which is empty for a not-yet-connected host (`agent_server/registry.rs:263-269`), so valid approvals would be rejected as "skill no longer on the host". Recovery tests use `FakeAgentLink`, which keeps skills while connected and so cannot show it. A failing test is needed to confirm. Decision reference: RW-09, RW-30 | `actions/executor.rs:48` (catch-up at start), `:329-332` (V) | H if confirmed | G |
| D37 | STATIC. Durability findings from unmerged PR #36 still present on main: the agent scheduler saves check state even when the spool push fails; the hub saves notification state before delivery succeeds and a failed delivery leaves state saying it was sent; a state read error becomes default state (possible duplicate card); quiet-hours release is persisted before sending. Decision reference: RW-03, RW-15 (04 section 15) | `agent/src/scheduler.rs:333-344`, `hub/src/runtime/delivery.rs:55-67,111-125,242-248`, `hub/src/runtime/dispatcher.rs:207-212`, `agent/src/spool.rs:66-95` | M | G |
| D38 | STATIC. Model attribution and accounting: `TriageReport` and usage rows carry no model (usage stores `provider.id()` only); a failed call records no usage; the LLM client has only an idle read timeout that restarts on each byte, and `max_seconds` is checked between turns, so there is no hard per-call deadline. Decision reference: RW-06 | `core/src/triage/report.rs:52-76`, `engine/src/triage/agent.rs:110-120,146-148`, `llm/src/http.rs:20-30` | M | G |

## 12. Documentation drift

| Doc | Says | Code |
|---|---|---|
| `ARCHITECTURE.md` section 9 (`:269`) | table `digests` | migrations create `digest_facts`; no `digests` (V) |
| ARCHITECTURE section 9 | "signals 30 days, transcripts 90 days", incidents kept | `incident_events` purged at 90 d; triage cache 90 d (`purge.rs:287-296`) |
| ARCHITECTURE section 4 | `checks` features `host`, `net` | third feature `sources` |
| ARCHITECTURE section 6 | port table; `Clock` implemented by engine | missing ports `SettingsRepo`, `UserRepo`, `SessionRepo`, `NotifyStateRepo`, `AdvisoryRepo`, `FactRepo`, `TriageCacheRepo`, `UsageRepo`, `InteractionSink`, `SignalSink`, `LogSource`; `SystemClock` is in the binary (`init.rs:135`) |
| ARCHITECTURE 5.3 | incident diagram without Dismiss | `Dismiss` implemented (`transition.rs:43,167`) |
| ARCHITECTURE section 8 (`:253`), `protocol.md:78` | `capabilities` meaningful | always empty, unused |
| `docs/book/src/reference/protocol.md:25` | version 1.0 | `PROTOCOL_VERSION` 1.1 (V); ADR 0018 says additions stay at 1.0 |
| ADR 0019 | only the writer writes actions | triage inserts and rotates tokens directly |
| ADR 0014 | tampered row cannot widen what runs | holds only for skills not forgeable on the agent; DB writer can forge approval + plan hash (E, INFERENCE) |
| ADR 0016 | suggestions (Proposed) | not implemented |
| ADR 0003 / INV-10 | "mutually authenticated" | TLS is one-way (hub pin); agent authenticates in an application-layer signature |
| `web/DESIGN.md:356` | Live indicator "is truthful" | covers incident/action push only; hosts and others are polled (B) |
| `web/DESIGN.md:341` vs `HostContext.tsx:177-186` | leaving a host page keeps selection | host detail forces selection and restores the user's choice on leave (B) |
| AGENTS.md / TESTING.md | `cargo xtask ci` mirrors CI | omits rustdoc, web `format:check`/`gen:api`, `arch-check --base` (AGENTS.md table) |

## Corrections to investigation reports

1. B s1.4 "REST surface is 50 operations": `web/src/api/openapi.json` has 50 paths and 58 operations (counted; matches INV and 58 `#[utoipa::path]` handlers). Used 58 operations over 50 paths.
2. A s3 "hub (18.5k lines)": same-method measurement gives 17137 for `domovoi-hub` (INV). Used 17137. Other crate LOC in INV re-measured and matched.
3. F s4 "/metrics unauthenticated?" was hedged; E states it as FACT (`api/system.rs:142-157`). Treated as a FACT per E; not re-read here beyond the `/healthz` handler.
4. Wording corrections after verification (G): `HumanApproval` is not compile-time sealed (constructors are `pub`; restriction is a text scan, section 8); the mutation-feedback lint is file-level and the leave guard exists only in the notes editor (section 9); D36 is a static code-reading finding, not a reproduced defect (section 11); the upgrade command checks a same-origin checksum, not an attestation (section 10.2).
5. Spot-checks that confirmed the reports without change: `Hello.capabilities` empty (`agent/src/link/handshake.rs:47`); triage queue reload only on lag (`triage/queue.rs:66-95`) and `TriageRequested` only on state change (`manager/decide.rs:125-148`); `verify()` sleeps in memory (`recheck.rs:65`); budget fail-open (`admission.rs:52`); `is_sensitive` omits `cli.*` (`patch.rs:107-117`); `/healthz` constant; `compatible()` rule (`version.rs:22-26`); no `expires_at` check in executor or `dispatch`; agent only logs `action_id`; cards.rs has no skill/args; `HumanApproval::new` call sites; `synchronous = Normal`; `digests` table claim in ARCHITECTURE.md:269; protocol.md says 1.0.
