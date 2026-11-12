# E. Security review of the current Rust workspace and the proposed autonomy model

Scope: read-only review of hub, agent, chat, engine, LLM, store and the design pack (`05-autonomy-and-safety.md`, `03-capability-model.md`, `01`, `09`). Paths are relative to `C:\Programming\domovoi\crates\` unless they start with `docs/`. Labels: FACT (read in code or docs), INFERENCE (follows from facts, not executed), RECOMMENDATION.

## 0. Verdict in five lines

- FACT: human-only approval is well built. One writer task, a guarded SQL update, a typed `HumanApproval` with no `Deserialize`, plan-hash recheck on both hub and agent, and gate tests.
- FACT: it is enforced by text scan and review, not by the type system. The stored approval is an unsigned database row.
- FACT: the weakest link is not the code path but what the human sees. Discord Approve buttons name no skill, host or argument, and the model writes all the surrounding prose.
- FACT: the agent cannot tell a human-approved dispatch from any other hub-originated `run_skill`. Autonomy therefore needs a new agent-verified authorization artifact, not a new `Principal`.
- RECOMMENDATION: do not ship L2/L3 until the artifact, the agent-side ceiling and the fail-closed offline rules in section 5 exist and are gate-tested.

## 1. Trust boundaries and authN/authZ

### 1.1 Boundaries (FACT)

| Boundary | Trusted side | Untrusted input crossing it |
| --- | --- | --- |
| Browser to hub web listener (`ARCHITECTURE.md:275-279`) | admin session | everything in requests; `Origin` header |
| Webhook sender to `/hooks/*` (`domovoi-hub/src/ingest/auth.rs:11-56`) | HMAC or bearer holder | body (becomes signals, incident titles, prompt text) |
| Agent to hub (`domovoi-hub/src/agent_server/handshake.rs:162-186`) | enrolled ed25519 key, TLS channel binding (RFC 9266), revocation check | tool output, signals, skill catalogue text |
| Hub to agent (`domovoi-agent/src/requests/skills.rs:62-125`) | agent's local catalogue and `allow_risk` | `run_skill` args and plan hash |
| Chat platform to hub (`domovoi-hub/src/auth/interactions/mod.rs:184-219`) | platform user id on the allowlist | button ids, thread text, slash args |
| Hub to LLM provider | redactor output only | model text (all of it untrusted) |
| Hub to its own SQLite file | nothing (see 1.5) | row contents are trusted on load |

### 1.2 Users, roles, sessions, CSRF (FACT)

- One admin account, no roles. `AuthState::setup` creates it once with a 24 h single-use token (`domovoi-hub/src/auth/mod.rs:183-247`). `chat` acts as `first_user()` (`auth/interactions/mod.rs:191`). There is no per-user authorization; "who" in the audit log is the single admin id plus, for chat, the platform user.
- Sessions: 32 random bytes in an `HttpOnly; SameSite=Strict; Secure-if-https` cookie, only the SHA-256 stored, 12 h idle and 7 d absolute (`auth/session.rs:9-65`). Argon2id passwords (`auth/password.rs`). Login throttle is per source IP, in memory, 5 failures a minute, lockout doubling to 15 min (`auth/limit.rs:9-62`). Not per account, resets on restart.
- CSRF: token is `SHA256("domovoi-csrf:v1\n" + session_id)` (`auth/session.rs:36-41`), compared in constant time (`auth/mod.rs:355-358`). The guard also checks `Origin` when present (`auth/csrf.rs:43-64`). Requests with no session cookie skip the token check and are then answered 401 by the `Authed` extractor (`api/mod.rs:252-266`). That is safe because every unsafe handler takes `Authed`; it relies on no handler forgetting it.
- Re-auth: `confirm_password` is demanded only for a closed list of "trust-widening" settings: any secret, `discord.allowed_user_ids`, `web.trusted_proxies`, `webhooks.*` (`domovoi-hub/src/settings/patch.rs:107-117`, `settings/service.rs:229-239`).
- Unauthenticated: `/healthz`, `/metrics` (`api/system.rs:142-157`), webhooks (own auth), SPA.
- Agent identity: single-use enrolment token (hashed, 15 min), ed25519 key, pinned hub certificate fingerprint, signed challenge bound to the TLS exporter, `revoked_at` checked each handshake (`handshake.rs:162-186`).
- Chat identity: Discord numeric user id from the gateway event (`domovoi-chat/src/discord/events.rs:131-141`), checked against `allowed_user_ids`. Discord signs the identity; the hub never sees raw button strings (`discord/ids.rs:1-8`). Anything the allowlisted account can do in Discord, it can do here.

### 1.3 Findings on this layer

- F1.1 FACT. A web admin session can set an LLM `cli.command` (absolute path) and `cli.args` (`domovoi-config/src/hub/llm.rs:66-80`). These are not in `is_sensitive` (`patch.rs:109-116`), so no password re-entry. The hub then runs that program through the runner (`domovoi-llm/src/cli.rs:1-140`; `test_llm` endpoint runs it on demand, `api/mod.rs:358`). INFERENCE: a stolen session cookie or an XSS gives arbitrary binary execution as the hub user without the password. It also allows removing `--tools ""` from the Claude preset (`cli.rs:94-96`), turning a prompt-injected log line into a tool-using coding agent on the hub box. RECOMMENDATION: treat `cli.*` as sensitive, and restrict `command` to a fixed allowlist of known binaries plus a fixed argument shape.
- F1.2 FACT. `/metrics` is open. INFERENCE: it leaks host and incident counts, spend and agent connection state. Low, but the design pack adds more labels.
- F1.3 FACT. Webhook HMAC covers the body only: no timestamp or nonce (`ingest/auth.rs:28-45`). Replay of a captured signed body re-opens an incident or injects text again. Low to medium.
- F1.4 FACT. Login throttle is per IP and in memory; with `trusted_proxies` unset behind a reverse proxy every user shares the proxy IP (`auth/proxy.rs:11-24`), so one attacker can lock the owner out and a distributed attacker is unthrottled. No MFA/passkey. Medium for a internet-exposed hub, and it matters more once the hub can mint autonomous permits.
- F1.5 FACT. A single role means "can approve" equals "can edit policy, rules, notes, LLM settings, skills on/off". The rework will add policy editing to the same role. RECOMMENDATION: separate step-up (password or passkey) for any policy mutation, and have the owner key (5.5) sign policy so session theft cannot widen autonomy.

### 1.4 Secrets (FACT)

- Config secrets are `SecretString` (`domovoi-config/src/secret.rs`); only 20 `expose_secret` call sites outside tests, all at point of use (grep result: llm headers, Discord client, webhook HMAC, crypto, heartbeat URL).
- UI-entered secrets: ChaCha20-Poly1305 with path-bound AAD, key in `<data_dir>/secret.key` mode-checked (`settings/crypto.rs:1-60`); write-only through the API (hub gate test scans API responses for leaks, `domovoi-hub/tests/gates.rs` around line 520-530).
- Redactor (`domovoi-engine/src/redact.rs:11-27`): registered secret values plus 9 regexes plus `key = value` for password/secret/token/api key. Gaps (INFERENCE from the regexes): JSON form `"password": "x"` does not match (`password\s*[=:]` cannot cross the closing quote); URLs with embedded credentials (`postgres://u:p@h`), cookies, `Authorization: Basic`, SSH key fragments without BEGIN/END, and any host secret that is not registered in Domovoi config all pass through. Everything read by `docker_logs`, `journal`, `docker_inspect` is host data, so INV-5 is best-effort for host secrets (the gate test `secrets_never_reach_the_model` covers registered values only). `strip_env` removes Docker `Config.Env` values (`domovoi-checks/src/containers/mod.rs:167`), which is a good targeted control.
- RECOMMENDATION: keep tools allowlisted per capability, add an explicit "may contain secrets" flag on capabilities so the policy can forbid sending their output to a non-local provider, and add JSON-key and URL-userinfo patterns.

### 1.5 Database trust (FACT, matters for rehydration)

`HumanApproval::restore` rebuilds an approval from three columns (`domovoi-store/src/repos/actions.rs:58`). `update_from` rewrites every column, including `args`, `plan_hash`, `approved_by` (`repos/actions.rs:173-202`). Nothing authenticates the row. ADR 0014 says a tampered row "cannot widen what runs" because the agent recomputes the plan hash. INFERENCE: that holds only for skills that are not on the agent; anyone who can write the SQLite file can set `status=approved`, a forged approver, any args valid for any local skill whose risk the agent allows, and a matching plan hash computed from the public manifest hash the hub already knows. The agent cannot distinguish that from a real approval (see 2.9). RECOMMENDATION: seal the approval fields (action id, plan hash, approver, channel, time) with an HMAC keyed from `secret.key` and verify in `restore`; fail to `rejected` on mismatch. The audit log has the same exposure: it is append-only by trigger only (`domovoi-store/migrations/0001_init.sql:145-153`), with no hash chain, so a file-level attacker can drop the trigger.

## 2. Every path by which an action can execute

State machine and writer: `domovoi-core/src/action/transition.rs:45-151`, `domovoi-engine/src/actions/writer.rs:232-435`, executor `executor.rs:219-290`. All writes go through one task and a SQL-level status guard (ADR 0019).

Only one path from `Proposed` to `Approved` exists in code (`ActionRequest::approve`, `transition.rs:65-85`); it takes a `HumanApproval` by value. Proposers are only the triage run and the follow-up flow (`triage/proposals.rs:128-137`, `triage/followup.rs`); `Principal::System("triage")` is the only system proposer in production code. Nothing runs automatically without an `Approved` row.

| # | Path | Who authorises | Re-validated at execution | Bound to | Notes |
| --- | --- | --- | --- | --- | --- |
| P1 | Web approve `POST /api/v1/actions/{id}/approve` (`api/actions.rs:104-124`) | admin session + CSRF; destructive also needs `confirm_text == skill name` (`auth/approval.rs:20`) | hub: skill present, risk equal, args valid (`executor.rs:325-341`), loop guard, plan hash (`transition.rs:104-118`); agent: catalogue, `allow_risk`, args, plan hash (`skills.rs:62-109`) | plan hash stored at proposal; the HTTP body names only the action id | the human sees the DTO (skill, risk, args) in the UI; the plan hash itself is not part of the request |
| P2 | Chat approve button (`auth/interactions/buttons.rs:134-163`) | allowlisted Discord user + single-use token (`approvals.rs:28-34`, token hash cleared on use `writer.rs:249`); never destructive (`buttons.rs:142`, `request.rs`/`transition.rs:80`) | same as P1 | same | see F2.1: the card does not show what is approved |
| P3 | Rehydrated approval after restart (`executor.rs:88-105`, `recover.rs:123-130`) | the earlier P1/P2 decision, read from the store | same as P1, run later | stored plan hash | no age limit (F2.2); restores a row that is not authenticated (1.5) |
| P4 | Event-bus lag catch-up (`executor.rs:62-70`) | same as P3 | same | same | up to 8 parallel runs |
| P5 | Retry | none exists. A failed action stays `Failed`; `retry_triage` only re-runs triage (`api/mod.rs:339`) and creates new `Proposed` actions that need a new approval (`proposals.rs:63-158`) | n/a | n/a | good: a retry cannot reuse an old approval |
| P6 | Cached triage report on a repeat incident (`triage/run.rs:80-93`, `cache.rs`) | none: proposals are re-validated and stored as `Proposed` | proposals pass `check` (`proposals.rs:37-53`) | new plan hash | a poisoned report can be re-served for 7 days (F3.4) |
| P7 | Scheduled or automatic remediation | none exists | n/a | n/a | scheduler only expires proposals (`writer.rs:376-425`), triage queues, digests |
| P8 | Suggestions (ADR 0016) | none | n/a | n/a | FACT: no `suggest` module in the engine; ADR status is Proposed. Its gate test (no suggestion path builds a `HumanApproval`) does not exist yet |
| P9 | Auto read-only execution without approval | the model | agent: skill risk must be `read`, args valid (`skills.rs:129-173`); hub same check (`triage/tools.rs:149-159`) | none | the only unattended execution of host-side code today. Its safety rests entirely on the manifest author labelling `risk = "read"` honestly |
| P10 | Verification re-check after success (`recheck.rs:56-100`) | system | read-only check; then `set_verdict(Fixed)` | n/a | model-independent; agent-reported result decides the incident verdict |

Findings:

- F2.1 FACT, high. The Discord card has `buttons: Approve/Deny` with an action id and token only (`domovoi-core/src/card.rs:32-48`, `domovoi-chat/src/discord/components.rs:34-47` labels are literally "Approve"/"Deny"). `build_card` (`domovoi-engine/src/cards.rs:111-157`) carries headline, bullets, `next_step` and `details`, all model text; it never lists the proposed skill, host or arguments. With up to three proposals the buttons are indistinguishable. The human authorises on the model's prose. Combined with prompt injection (section 3) this is the practical bypass of ADR 0007: the model cannot approve, but it can write the sentence the human acts on. RECOMMENDATION: render one block per proposal with skill, host, risk, every argument, expiry and a short plan-hash prefix; put the same text into the button label or an adjacent field; never use model prose as the only description.
- F2.2 FACT, medium. The approval window (`PROPOSAL_TTL` = 1 h, `triage/mod.rs:54`) bounds only the `Proposed` state. `dispatch` checks neither `expires_at` nor incident state (`transition.rs:104-118`, `executor.rs:219-290`). A hub that restarts days after an approval runs it from the catch-up. An incident that resolved in the meantime still gets its action (card buttons disappear when resolved, `components.rs:56-58`, but the row stays `Proposed` and approvable on the web). RECOMMENDATION: an `execute_by` deadline after approval, a precondition re-check (incident still open, evidence still fresh) at dispatch, and `Approved -> Expired` for stale ones.
- F2.3 FACT, medium. Approvals are not bound to a content hash the human saw. The action row is immutable after proposal (only transition methods mutate; fields are `pub(super)`, `request.rs:130-145`), so TOCTOU on args is closed in practice, and the manifest hash is in the plan hash. Two residual gaps: (a) `ManifestHash` covers the serialized manifest only (`domovoi-core/src/skill/manifest.rs:145-151`); the entry script's bytes are not hashed, so a script edited after approval runs under the old plan hash (mitigated by the permission checks in `domovoi-runner/src/catalog/checks.rs:141-167`: not group/world writable); (b) `revalidate` uses the hub's cached copy of the catalogue from `Hello` (`executor.rs:329`), so the hub-side check can be stale, but the agent's own check is authoritative (`skills.rs:98-109`). RECOMMENDATION: include the entry-file SHA-256 (and `run_as`, `hosts`) in the capability version, and make the web approve request carry the plan hash the UI displayed (reject on mismatch).
- F2.4 FACT, medium. The agent cannot verify that a human or any authority approved: `RunSkill` carries `action_id, skill, args, plan_hash, deadline` (`domovoi-proto/src/messages/work.rs:120-131`), and the plan hash is a function of public data the hub also computes. A compromised hub (or a DB forger, 1.5) can run any local `write` skill with valid args on any connected agent. INV-3 limits blast radius to the local catalogue and `allow_risk` (default read+write, `domovoi-config/src/agent.rs:269-271`) but is not an authorization check. This is the central design fact for section 5.
- F2.5 FACT, low. Audit failures do not stop execution: `announce` logs the error and continues (`actions/mod.rs:140-144`), and the state write precedes the audit write (`record_from`, `mod.rs:127-136`), so a crash between them leaves a transition with no audit line. The write-ahead order is right for `Dispatched`, but the audit trail is best-effort, not a transactional record.
- F2.6 FACT, low. Chat Deny does not check the token (`writer.rs:265-279`, `buttons.rs:165-189`): any allowlisted user can deny any action whose id they can read. Acceptable.
- F2.7 FACT, low. The loop guard (3 per skill per host per hour) is in memory and rebuilt from audit rows (`executor.rs:297-320`, `recover.rs`). It protects against loops, not against a malicious sequence of distinct skills.

## 3. Model output as authorization, and prompt-injection surfaces

### 3.1 Authorization by model output (FACT)

- The tool catalogue is the closed `ReadTool` enum (`domovoi-core/src/triage/tools.rs:7-47`), parsed through `ReadToolCall::from_json` with length and range limits (`triage/tool_call.rs`). Unknown names are errors the model can read (`engine/triage/tools.rs:61-72`). Gate tests pin this (`domovoi-engine/tests/gates.rs:32,56`; `domovoi-core/tests/gates.rs:14,48`).
- Nothing the model returns reaches `HumanApproval`. A report becomes `Proposed` actions only through `proposals::store` (`proposals.rs:63-158`): skill must be in the host catalogue, `applies_to` must match the incident's check, args must validate against the manifest. Free text in the report is never executed (gate `shell_command_in_report_is_never_executed`). Proposals are always created for `incident.host()` (`proposals.rs:128-130`), so injected text on host A cannot target host B.
- Follow-up chat parses a `PROPOSALS:` line out of answer text (`triage/followup.rs:34-45`). It is a text-marker parse of model output; the parsed objects go through the same `store`. INV-9 says "never parsed into commands"; this is parsed into proposals only. Acceptable, but keep the marker parser strict and covered by the injection gate.

### 3.2 Influence on targets and argv without validation

- F3.1 FACT, low to medium. Read-tool string arguments are validated by length only in the core (`tool_call.rs:241-258`, container/unit/device up to 128/64 chars). Charset checks exist for `journal` units (`UnitName`: no leading `-`, restricted alphabet, `domovoi-checks/src/commands.rs:107-148`) and `smartctl` devices (`/dev/[a-z0-9]+`, `commands.rs:199-225`). `docker_logs`, `docker_inspect` and `docker_events` pass the model-supplied container name straight to the Docker API client (`domovoi-agent/src/requests/tools.rs:86-96`, `domovoi-checks/src/containers/docker.rs:224-241`). INFERENCE: if the client interpolates the name into the URL path, `/`, `?` or `..` change the endpoint. Docker's router normally redirects non-clean paths, so I rate this low, but it is unverified. RECOMMENDATION: validate container names against `[A-Za-z0-9][A-Za-z0-9_.-]*` in core.
- F3.2 FACT. Write-skill args are bounded by the manifest's `ParamSpec` (string pattern and max_len, integer range, enum). The model chooses the values. Nothing ties an argument to the incident: a prompt-injected model can propose `restart_container name=<any container matching the pattern>`, and the human sees a card without arguments (F2.1). RECOMMENDATION: add `bind: incident.target` style constraints on parameters (value must equal, or be derived from, the incident target or an evidence field) so a proposal cannot wander.
- F3.3 FACT. Skill execution is argv-only with `DOMOVOI_PARAM_*` env, cleared environment, no shell (ARCHITECTURE §12; `domovoi-runner/src/skill_run.rs`). Good. The risk is a skill author who runs `sh -c "$1"` inside the script: not detectable by Domovoi. RECOMMENDATION: a manifest lint that flags scripts invoking a shell on a parameter, and require `pattern` on every string param (no unbounded strings) for eligibility of anything beyond read.

### 3.3 Injection surfaces (FACT unless marked)

Untrusted text that enters prompts, all after redaction (`context.rs:71`, `tools.rs:87-93`):

| Surface | Where it enters | Controls |
| --- | --- | --- |
| Container logs, journal, docker inspect/events, read-skill stdout | tool results (`triage/tools.rs:95-181`) | 8 KiB cap, redaction, "untrusted data" clause (`prompts/triage_system.md`, `followup.rs:30-38`) |
| Signal summaries and evidence from checks and webhooks | context bundle `signals()` (`context.rs:98-125`) | cap 4 KiB per part |
| Incident titles of other incidents | `similar()` uses `incidents.search(query, 20)` with no host filter (`context.rs:168-194`) | INFERENCE: text from a compromised or noisy host A can appear in host B's prompt. Filter by host |
| Host notes and the generated host-profile note | `notes()` and `profile_note()` (`context.rs:200-244`) | notes are admin-authored but importable from uploaded Markdown (`api/notes_import.rs`); a poisoned note is persistent injection |
| Skill catalogue text (description, param specs) | `catalogue()` (`context.rs:248-267`) | comes from the agent; a compromised agent can write prompt text for its own host's incidents only |
| Thread messages and web questions | follow-up flow | only allowlisted users; replayed history is marked untrusted (`followup.rs:30-32`) |
| Triage cache | 7-day reuse of a prior report (`run.rs:80-93`) | a poisoned report persists and is re-proposed after `check`; the cache key buckets numbers and drops timestamps (`cache.rs:22-48`) |
| Provider error bodies | `provider_failure` redacts and clips to 500 chars (`followup.rs:56-62`) | good, ADR-gated |

- F3.4 INFERENCE, medium. Combining F2.1, F3.2 and the cache: an attacker who can write a log line (for example via a container name or HTTP request logged by a service) can try to steer a proposal and the explanation a Discord user reads. The system prompt's instruction is the only barrier before human review. Deterministic controls to add: (1) show the human the structured plan; (2) pin proposal targets to incident-derived values; (3) mark the card "proposed after reading untrusted text from tools X" when the report used `docker_logs`/`journal`/read-skill output; (4) never let a cached report's proposals skip the structured display.
- The CLI provider is called with no tools by preset (`cli.rs:94-96`, `--tools ""`), fine, but see F1.1.

## 4. Audit trail completeness

FACT, what is recorded: every action transition writes an `audit_log` row with actor (`user:<id>` or `chat:<platform>:<user>` or `system:*`), plan hash, host, skill, risk, status; approvals add channel; proposals add args and expiry; runs add exit code, argv hash, output SHA-256, duration, truncation and a redacted 4 KiB excerpt (`domovoi-engine/src/actions/audit.rs:53-190`). Chat interactions that are accepted write `chat.<kind>` rows (`auth/interactions/mod.rs:156-167`, `210-216`). The table has update/delete/replace triggers (`0001_init.sql:145-153`) and the port exposes only `append` and `list` (gate `audit_log_has_no_mutation_api`).

Gaps:

- A1 FACT. Rejected or unauthorised attempts are not audited: a non-allowlisted chat user, a destructive-approve attempt from chat, a wrong token, a failed password confirmation, a failed login are only `tracing` lines (`mod.rs:185-190`, `buttons.rs:142-143`, `auth/mod.rs:274`). For a policy engine you need denials as first-class records.
- A2 FACT. Web approvals carry user id only; no client IP, user agent or session id. Chat approvals carry the platform user, which is good.
- A3 FACT. The record omits what the human was shown (rendered card or UI text, model report id, evidence hashes) and whether the report used untrusted tool output. RECOMMENDATION: store a `plan_view_hash` and the triage run id on proposal and approval.
- A4 FACT. No tamper evidence beyond the trigger: no hash chain, no external anchor. Settings changes are audited per ARCHITECTURE §14, but I did not verify field-level before/after values (not read).
- A5 FACT. Audit write failure is swallowed (F2.5), and the agent keeps no local execution record at all (only `tracing::info!` at `domovoi-agent/src/requests/skills.rs:110`). Offline autonomy (section 5) cannot be accounted for without one.
- A6 FACT. There is no separate record of "decision trace" (policy rule, version, conditions). The design pack asks for it; it does not exist.

## 5. Evaluation of the proposed autonomy and policy model

### 5.1 What the design pack gets right

Separate typed authorization from `HumanApproval`; explicit deny wins; knowledge is not authority; agent-side validation after hub authorization; destructive stays human-only; stale or ambiguous context means no autonomous run; offline permit cache is small, signed, expiring, revocable. These match F2.4 and ADR 0007's own text ("future autonomy may only act on reversible write skills", `docs/adr/0007-human-only-approvals.md:15`; `ARCHITECTURE.md:367`).

### 5.2 Gaps and contradictions in the proposal

- G1. Precedence list (`05-autonomy-and-safety.md`, "Policy scope and precedence") says "any explicit deny wins" then "most specific rule wins". Define it as: deny is absolute at any scope and cannot be overridden by a more specific allow; allows only combine by intersection of their limits; "specific override" is available only between two allows or two defaults. Otherwise "global deny + capability allow" is ambiguous. Also resolve "approval required or denied according to class" (step 6): fixed table: read allowed, write approval-required, destructive denied for autonomy.
- G2. L1..L3 are called categories but the examples mix a reversible action (restart) with a harmful-if-wrong one (firewall block, isolate workload) that can lock the owner out and is not covered by today's `Risk::{Read, Write, Destructive}` (`domovoi-core/src/skill/mod.rs`). Autonomy eligibility needs new manifest facts: `reversible`, `revert` command or TTL, declared effect scope, `management_path_safe`. Today `revert`/`confirm_window` are optional (`manifest.rs:140-142`).
- G3. "Whether the operation remains allowed when the hub is offline" is a policy attribute, but the cache itself is the risky object (5.6).
- G4. The doc treats the model as the planner for L2/L3. The strongest design is trigger by deterministic rule over signals (not model text), with the model optional for explanation. Otherwise autonomy converts F3.4 from "trick a human" into "trick a trigger".
- G5. Scope of enforcement is only described as "hub and agent". Section 5.3 fixes it.

### 5.3 Enforceable scope: where each rule lives

| Rule scope | Enforced by | Why |
| --- | --- | --- |
| Agent ceiling (`[policy]` in the agent file: allowed risk, allowed autonomous capabilities, forbidden targets, management ranges, max rate) | agent only, never pushed by the hub (today `config_update` carries schedules and the Docker switch only, ARCHITECTURE §8) | the one thing a compromised hub cannot widen; it generalises today's `allow_risk` |
| Capability contract (target schema, effect, revert/TTL, verify) | agent manifest + hash in the permit | bounds what a permit can mean |
| Host policy, category policy, global default | hub evaluates, agent re-evaluates the subset it can see (host, capability, target, window, rate, expiry) | hub for UX/simulation; agent for enforcement |
| Per-host narrowing in the UI | hub `PolicyLink` already hides disabled skills at dispatch and "only ever narrows" (`domovoi-hub/src/agent_server/skill_policy.rs:1-8`) | reuse as the model: hub policy narrows an agent ceiling, never widens |

So: global, host and category policy are authoritative for deny and for "ask vs allow" decisions, but the permit that reaches the agent must carry a self-contained, signed snapshot of the matched rule (id, version, scope, limits, expiry). The agent applies `ceiling ∩ permit`, and re-checks target, args, rate and window locally. Deny anywhere (agent ceiling, hub policy, permit exclusion) wins.

### 5.4 Coexisting with INV-1 and ADR 0007

Current wording has three load-bearing sentences: "Only a Human principal can approve" (`ARCHITECTURE.md:320`), "`HumanApproval::new` only in `auth/`" (ADR 0007, `rules.rs:169`), and "no auto-approval for destructive" (INV-6). Autonomy does not need to weaken any of them, if it is not an approval:

1. Keep `HumanApproval` and INV-1 verbatim. Rename the concept in prose: INV-1 governs approvals; add INV-13: "Unattended execution requires a `PolicyPermit` minted only in `domovoi-hub/src/policy/`, signed by the owner key, bound to one plan; no model, chat or MCP path can mint, edit or extend one."
2. Replace `ActionRequest.approved_by: Option<HumanApproval>` (`request.rs:139`) with an enum `Authorization { Human(HumanApproval), Policy(PolicyPermit) }`, and add `authorize_by_policy(permit, now)` next to `approve`. `PolicyPermit` has no `Deserialize` and no public constructor outside the policy module, mirrors ADR 0014's restore rule, and gets its own arch-check allowlist (`xtask/src/arch/rules.rs`, a third prefix list like `HUMAN_APPROVAL_*`).
3. Gate tests: the existing `no_approval_without_human` pins `approve`'s signature (`domovoi-core/tests/gates.rs:67-76`). Keep it. Add: `policy_permit_cannot_approve_destructive`, `policy_permit_requires_matching_plan_hash`, `no_model_path_constructs_a_permit`, `permit_has_no_deserialize`, `human_approval_not_constructible_from_policy_module`.
4. INV-6 becomes structural, not configurable: `authorize_by_policy` returns `DestructiveNeedsHuman` for `Risk::Destructive` irrespective of any policy row.
5. Update ADR 0007 consequences and `ARCHITECTURE.md` §5.6/§15/§19 in the same change; add a new ADR for the permit, as ADR 0007 itself requires ("any future surface must state how it preserves this invariant").
6. UI, audit and chat must say who authorised: `authorized_by = policy:<rule_id>@<version>` plus `owner:<user>` that created the rule. Never reuse `user:<id>` for permit-based runs.

### 5.5 Agent-side verification (required, and absent today)

The agent must receive and verify the authorization, which means a signature the hub cannot forge alone:

- RECOMMENDATION: the owner holds a signing key (WebAuthn/passkey or an ed25519 key in a browser/CLI), pinned by each agent at enrolment next to the hub certificate fingerprint. Policy rules are owner-signed bundles `{rule_id, version, host, capability+hash, target schema, limits, window, expiry}`. The hub stores and serves them but cannot create or widen them. The hub then issues per-run permits `{permit_id, plan_hash, rule_id@version, nonce, not_before, not_after}` signed by a hub key whose scope the agent restricts to "within a pinned owner-signed rule". A compromised hub can only run what the owner pre-authorised, within limits.
- For human-approved runs the same applies in weaker form: today F2.4 stands. At minimum add a hub-signed `Authorization` envelope with the approver kind to the `RunSkill` frame so the agent can enforce "autonomous requires a permit, human requires nothing extra yet" and the agent audit can say which.
- The agent records every execution in a local append-only, hash-chained log (action id, permit id, argv hash, exit, times), reported at reconnect with sequence numbers for gap detection.

### 5.6 Offline / emergency mode: risks of cached owner-approved policies

Risks and required mitigations:

| Risk | Mitigation |
| --- | --- |
| Agent clock skew or tampering extends a permit | permit validity uses a monotonic budget (max N executions and max T since last hub contact) plus wall-clock; if clock jumps backwards or the agent has no time sync, refuse (fail closed) |
| Replay and duplicate execution after restart or reconnect | permit nonce and per-rule execution counter persisted atomically before running; idempotent capability or revert check; the hub dedupes by `permit_id` on reconcile |
| Revocation lag | short expiry (minutes to hours), revocation list delivered at reconnect, owner-visible "outstanding offline permits" panel |
| Stolen or edited cache file | owner-signature verification on load; file mode checks like the key file (`settings/crypto.rs:46-52`) |
| Condition evaluation with hub-supplied evidence | offline permits may only use evidence the agent measures itself; stale or missing evidence means no run |
| Offline model use | none: offline permits are rule-triggered with no model (G4) |
| Self-lockout (SSH, VPN, management network) | agent-local hard exclusions for loopback, management ranges and listening-service ownership, not overridable by any permit; automatic expiry and rollback; cap on concurrent blocks |
| Escalation chain (cached permit used to create new permits) | permits cannot reference policy edits, skill install or credential changes; capability allowlist for offline is a strict subset of L3 |
| Audit gap while offline | local hash-chained log (5.5), reconciled and gap-checked later; refuse to run if the local log is full or unwritable |

### 5.7 Guardrails required before any autonomous execution

1. Typed `PolicyPermit` with the gate tests in 5.4; destructive excluded in code.
2. Owner-signed policy and agent-pinned owner key; agent ceiling file that the hub cannot change.
3. Eligibility test at policy-save time: a capability is autonomous-eligible only if it declares bounded parameters (no free strings), a `revert` or TTL, a `verify` step, `hosts`, `applies_to`, and the entry-file hash in its version.
4. Targets derived from deterministic evidence: the argument must equal, or be derived from, the incident target or signal field. The model may pick among eligible capabilities, not choose the target freely.
5. Rate and blast-radius limits per rule, host and global; automatic circuit breaker that disables a rule after N `VerifyFailed` or failures in a window; one-click global pause plus a file-based local pause on each agent.
6. Shadow mode first: evaluate the policy and record "would have run" decisions with the same evaluator (D-28) for a soak period before live.
7. Execution-time recheck of preconditions and incident freshness (see F2.2); `execute_by` on every permit.
8. Full decision trace in audit (rule, version, inputs hash, conditions, outcome) and denials audited (A1).
9. Persistent, user-visible notification of every autonomous run in chat/UI, with undo where `revert` exists; no silent execution.
10. Stored-row integrity: HMAC-seal approvals and permits (1.5) so DB write access cannot mint authority.
11. Policy changes require step-up auth and are never reachable from chat, MCP or a model tool; extend `no_tool_or_chat_command_reaches_settings` (`domovoi-hub/tests/gates.rs:535`) to the policy module.
12. Treat all model-influenced text as untrusted in policy conditions: no condition may read model output.

## 6. Ranked risks for the rework

1. HIGH, F2.1: approval UI/chat shows model prose, not the structured plan. Fix first, independent of autonomy.
2. HIGH (conditional on autonomy), F2.4/5.5: the agent cannot verify authorization. Adding autonomy on the current wire format widens a compromised hub from "any local write skill" to "any local write skill, silently".
3. HIGH, F1.1: admin session can configure arbitrary program execution through the LLM CLI provider without re-authentication. Policy-editing UI will inherit the same weak step-up.
4. HIGH (design), G4/F3.4: model-selected autonomous actions turn prompt injection from "persuade a human" into "trigger a rule". Use deterministic triggers and pinned targets.
5. MEDIUM, 1.5: approvals and the audit log are unauthenticated rows; DB write equals forged authority; ADR 0014's claim is overstated.
6. MEDIUM, F2.2: approvals have no execution deadline or freshness recheck; restart catch-up can run stale approvals.
7. MEDIUM, F2.3: plan hash omits the entry script; web approval is not bound to the hash the user saw.
8. MEDIUM, 5.6: offline permit cache brings clock, replay, revocation and lockout risks that need a hard-coded agent ceiling and a local hash-chained log; neither exists.
9. MEDIUM, 1.4: redactor misses JSON-quoted secrets, URL credentials and host secrets not in config, and logs/journal flow to a cloud provider.
10. MEDIUM, A1/A5/A6: denials, UI-shown content, decision traces and agent-local execution are not recorded.
11. LOW to MEDIUM, F1.4/F1.3: per-IP in-memory login throttle behind a proxy, no MFA, webhook replay; both matter more once the hub can issue permits.
12. LOW, F3.1/F3.3/`context.rs:168`: container-name charset not validated, cross-host incident titles in prompts, skill scripts that shell out internally.
13. LOW, F1.2/F2.5/F2.6/F2.7: open `/metrics`, swallowed audit errors, token-less deny, in-memory loop guard.

Positive findings to preserve: single-writer action service with SQL status guard (ADR 0019); one `HumanApproval` construction site per direction enforced by `xtask` (`rules.rs:168-175`, scan at `xtask/src/arch/scan.rs:63-72`, testkit dev-only per Cargo.toml); agent local catalogue and `allow_risk`; per-host skill narrowing; redaction before prompts and on provider errors; argv-only execution; enrolment with channel-bound signatures; constant-time token and CSRF compares.

Caveat on enforcement strength (FACT): INV-1's code-level control is a line-based text scan for `HumanApproval::new(`/`restore(` outside `auth/`, the store and tests (`scan.rs:75-100`); a renamed import or macro would evade it. A review rule or a compile-time seal (a zero-sized token type only the hub's `auth` module can construct and `new` requires) would be stronger; apply the same to `PolicyPermit` from day one.

> Note added after review: the "compile-time seal (a zero-sized token type only the hub's `auth` module can construct)" suggestion above is not feasible across crates — `HumanApproval` must live in `domovoi-core` because the engine uses it and cannot depend on the hub. RW-11 in `docs/rework/03` records the feasible replacement (path-resolved clippy `disallowed-methods`, an arch rule on the `expect` attribute, and the existing scan).
