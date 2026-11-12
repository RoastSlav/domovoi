# D. Hub/agent protocol, execution and the proposed capability model

Read-only investigation. Labels: FACT (read in code or docs, cited), INFERENCE (follows from code, not run), RECOMMENDATION. Paths are relative to `C:\Programming\domovoi`. Nothing was executed; no file other than this one was written.

Sources read: ADR 0003, 0004, 0007, 0018; `ARCHITECTURE.md` §4, §7, §8, §12, §15; `crates/domovoi-proto`, `domovoi-agent`, `domovoi-runner`, `domovoi-checks` (framework, commands), `domovoi-hub/src/agent_server`, `domovoi-engine/src/actions`, `domovoi-core` (skill, action, ports); `docs/design-pack/03-capability-model.md` and `11-capability-catalogue.md`.

---

## 1. Transport, framing, auth, versioning, liveness

### Transport and framing
- FACT: WebSocket over TLS 1.3, agent dials out; hub certificate is self-signed and pinned by SHA-256 (`docs/adr/0003-hub-agent-split.md:13`; `crates/domovoi-agent/src/link/tls.rs:176` TLS13 only; `:179` `with_no_client_auth`).
- FACT: JSON text frames, envelope `{v, id(UUIDv7), kind, reply_to?, payload}`; `MAX_FRAME_BYTES = 1 MiB` (`crates/domovoi-proto/src/envelope.rs:11`); binary frames are an error (`agent/src/link/ws.rs`, `hub/src/agent_server/ws.rs`). Hub drops a connection above 100 frames/s (`hub/src/agent_server/ws.rs:16`).
- FACT: 23 message kinds, declared once in a macro (`crates/domovoi-proto/src/messages/mod.rs:78-125`). Unknown `kind` decodes to `Message::Unknown`, answered with `unsupported`; unknown payload fields are ignored (`envelope.rs:76-93`).
- FACT: `ReadToolCall` inside `run_tool` is parsed strictly (unknown tool or field fails the whole frame): `messages/work.rs:301-322`, doc in `docs/book/src/reference/protocol.md:21`.

### Authentication
- FACT: not mTLS in the TLS sense. TLS authenticates the hub only (pin). The agent authenticates at the application layer: `hello` -> `challenge` (32 random bytes) -> ed25519 signature over `"domovoi-agent-auth-v1\0" || nonce || agent_id || RFC 9266 channel binding`, verified with `verify_strict` against the enrolled key (`proto/src/auth.rs`; `hub/src/agent_server/handshake.rs:150-190`). ADR 0003 and INV-10 call this "mutually authenticated"; that is accurate in effect, not in TLS terms.
- FACT: enrolment token is single-use, 32 random bytes, stored hashed, 15 minutes (`ADR 0003:14`; `handshake.rs:84-106` consumes by SHA-256, 1 s delay on a bad token). A new enrolment revokes the host's previous agent (`handshake.rs:112-116`). Revoked agents get `reject revoked` (`:162`).
- FACT: single-box mode runs the same handshake over an in-memory transport with session hint `"local"` (`agent_server/mod.rs:52,187`; `agent/src/local.rs`).
- FACT: there is no SSH or agentless execution path anywhere in the workspace. The `ssh` hits are the SSH-login *check* (`domovoi-checks/src/security/ssh.rs`). ADR 0003 rejects agentless SSH explicitly.

### Versioning and compatibility (R14)
- FACT: `PROTOCOL_VERSION = 1.1` (`proto/src/version.rs:17`). Hub accepts agent iff same major and `agent.minor <= hub.minor <= agent.minor + 1` (`:22-26`). A newer agent than the hub is refused. `incompatible` is a *final* error on the agent (stops retrying: `agent/src/error.rs:93`, `link/mod.rs:163`).
- FACT: second, independent gate by release: `MIN_AGENT_VERSION = MIN_HUB_VERSION = 0.0.5` (`version.rs:49-60`); the hub applies it only after the signature verified (`handshake.rs:192-215`); the agent checks `welcome.hub_version` (`link/handshake.rs`). The `agent_too_old` reason is only sent to releases that know it (`handshake.rs:140-148`).
- FACT: the agent never learns the hub's protocol minor (`Welcome` carries only `hub_version`, `proto/src/messages/handshake.rs:332-339`).
- FACT (doc drift): `docs/book/src/reference/protocol.md:25` says "current version is 1.0"; code and `tests/protocol/version.rs:25` say 1.1. ADR 0018 says additions "stay at 1.0" (optional fields), so the practice is: additive optional fields, no minor bump.
- FACT: snapshot tests pin every message (`crates/domovoi-proto/tests/protocol/snapshots*`).

### Heartbeat, reconnect, buffering
- FACT: agent pings every `welcome.config.heartbeat_seconds` (0 means 10 s), declares the link dead after 3 unanswered pings (`agent/src/link/mod.rs:39,230-238`, `handshake.rs:99`). The hub never pings; it answers pongs and records `last_seen` on any frame (`hub/src/agent_server/session.rs:60,110`). The hub's presence loop raises `agent.down` / `host.down` after three silent heartbeats (`presence.rs:1-8`).
- INFERENCE: a half-open TCP connection stays `Presence::Connected` on the hub until TCP/TLS fails or the agent reconnects (`registry.rs:106-110` replaces the old session). During that window `wait_for_agent` (`engine/src/actions/executor.rs:344`) says connected and a dispatch goes into a dead socket; it ends as a timeout after skill timeout + 30 s.
- FACT: reconnect backoff 1 s doubling to 60 s, +-20% jitter, reset after 60 s stable (`link/backoff.rs:6-12`). Handshake step timeout 30 s on the agent, 10 s on the hub.
- FACT: offline buffering exists for *signals only*: JSONL spool, `batch_seq` rising across restarts, replayed on reconnect and dropped on `signals_ack`; caps 10 000 batches / 8 MiB, evicting info-severity firing signals first, resolved signals last (`agent/src/spool.rs:1-10,26-28,63`). Hub acks only after `sink.submit` succeeds (`session.rs:90-100`). Results of requests are not buffered (see 2).
- FACT: hub-pushed settings: `welcome.config` and `config_update` carry heartbeat, per-check overrides and the Docker tri-state, nothing else (`messages/handshake.rs:70-80`, `hub/.../overrides.rs:50-65`).

---

## 2. What agents report, what they can execute, how an action travels

### Reported
- FACT: `hello` carries: protocol, agent_version, `HostFacts` (hostname, os, kernel, arch, cpus, mem, boot id), `capabilities: Vec<String>`, `skills: Vec<SkillSummary>`, `checks: Vec<CheckInfo>`, `docker: Option<DockerReport>` (`proto/.../handshake.rs:164-174`). The hub keeps the hello snapshot in memory per live session (`registry.rs` `Live`) and answers `skills(host)` from it (`:263`).
- FACT: there is no continuous inventory. The hub builds a host profile by calling the fixed read tools (`hub/src/profile/collect.rs:1-6`). Check results flow as `signals` batches; on-demand re-check is `run_check` (used for verification).
- FACT: the catalogue is loaded once at agent start (`agent/src/runtime.rs:138`, `Arc<SkillCatalog>` immutable); `SkillCatalog::reload` exists (`runner/src/catalog/mod.rs`) but nothing re-announces. A new or edited skill needs an agent restart and a new hello.

### What can run on an agent
1. Built-in checks (compiled in, `domovoi-checks`).
2. Read tools: closed `ReadToolCall` set (11 tools, `core/src/triage/tools.rs:8`), implemented from `Commands` argv templates with typed slots, absolute program candidates, no `-` prefix, 15 s timeout, 2 MiB cap (`checks/src/commands.rs:1-36`) plus the Docker API.
3. Skills: host-owner-written programs in `skills/<name>/skill.toml`, risk `read|write|destructive` (`core/src/skill/manifest.rs:31,109`). Hub cannot add or edit them (INV-3).

### Action path (write skill)
`Proposed` (from `ActionProposal{skill,args}` validated against the *advertised* `SkillSummary`: `engine/src/triage/proposals.rs:34-52`) -> human approval -> executor (`engine/src/actions/executor.rs`):
1. reload action, still `Approved` (`:219-232`); in-memory claim so event path and catch-up cannot both run it (`:107-125`).
2. `revalidate` against the live advertised skill (still present, same risk, args valid) (`:325-341`).
3. loop guard: 3 dispatches per skill per host per hour, rebuilt from audit after a restart (`:297`, `actions/mod.rs:50`).
4. `PlanHash::compute(skill, manifest_hash, host, args)` re-derived and persisted as `Dispatched` via the single writer (`:248`; `core/src/action/approval.rs:19`).
5. `wait_for_agent` up to 5 min (`actions/mod.rs:51`), then `link.run_skill` with deadline = skill timeout + 30 s (`:260-272`).
6. wire: `run_skill{action_id, skill, args: Map<String,String>, plan_hash, deadline}` (`proto/.../work.rs:120`). Hub requests are correlated by envelope id; at most 256 pending per agent (`registry.rs:19,160`).
7. agent re-validation, fixed order: skill in local catalogue; risk in `[policy] allow_risk` (default read+write; destructive is opt-in: `domovoi-config/src/agent.rs:68,269`); args vs local manifest; recompute plan hash; then run (`agent/src/requests/skills.rs:1-12,38,98`). Gate tests: `crates/domovoi-agent/tests/gates.rs`.
8. runner: `build_spec` = `[entry, one argv element per declared param]` plus `DOMOVOI_PARAM_*`, cwd = skill dir, manifest timeout and output cap (`runner/src/skill_run.rs:24`); `SandboxExecutor` refuses relative paths and shell-named programs, clears the environment (no `LD_*`), own process group killed on timeout (TERM then KILL after 2 s), optional `run_as` uid switch, redaction (`runner/src/exec/mod.rs:20-57`). Explicitly no namespaces/seccomp/cgroups (`:51`).
9. reply `skill_result{ok{exit_code,stdout,stderr,truncated,duration_ms,argv_hash}|err{code}}`; executor records `Succeeded`/`Failed` and, on success, runs the verify re-check (`executor.rs:272-289`).

- FACT, argv-only enforcement is layered: runner is the only crate with `Command::new` (`runner/src/exec/spawn.rs:41` scoped allow; ADR 0004:12; arch-check). The boundary is argv; what the *skill script does inside* is trusted code (a skill can contain any logic; only the entry file name is checked against shell names, `exec/program.rs`).
- FACT, concurrency and limits: agent runs at most 4 requests at once, queue 32, then `busy` reply (`requests/mod.rs:39,161`; `link/mod.rs:41`). Every request is cut off at its `deadline` (`requests/mod.rs:238,254`) which also drops the child (`kill_on_drop`).

### Timeouts, retries, idempotency, results
- FACT: no retries anywhere on the action path. No idempotency key in the protocol: `action_id` is logged by the agent but not used for dedup (`skills.rs:~98`, `rg idempotent` finds only the `Redactor` contract). The only duplicate protection is hub-side: persisted `Approved -> Dispatched` transition plus the in-memory claim.
- FACT, connection dropped mid-action: `Registry::end` drops the session's pending senders, so `request` returns `LinkError::Transport("the agent disconnected before answering")` (`agent_server/link.rs:34-46`); executor maps any `Err` to `Failed(err.to_string())` and does *not* verify (`executor.rs:286-288`). Deadline expiry becomes `LinkError::Timeout`, also `Failed`.
- FACT: the agent keeps running the skill after the link drops (the request task is independent of the link, `requests/mod.rs:314-319`); its reply is queued in the link outbox, goes out on the next session with the old `reply_to`, and the hub discards it ("a result arrived after its request gave up", `hub/.../session.rs:117-146`; `Registry::deliver` also filters by session, `registry.rs:193`).
- FACT: hub restart: actions left `Dispatched` are failed with "the hub restarted while this ran; check the host before running it again" (`engine/src/actions/recover.rs:27,204`).
- FACT: `ActionStatus` has no unknown/indeterminate outcome (`core/src/action/request.rs:15-42`): `Failed{reason}` conflates "did not run", "ran and exited non-zero", and "ran, outcome lost".
- INFERENCE: for a write skill like "restart container" this is tolerable (`Failed` text tells the human to check). For anything non-idempotent or self-disrupting (reboot, firewall change that cuts the agent link, package upgrade) the current contract is unsafe: the result is lost and the product claims failure.

---

## 3. Existing notion of "capabilities" or features

- FACT: `Hello.capabilities: Vec<String>` exists ("optional features, such as `docker` or `journal`", `proto/.../handshake.rs:162-164`) but the agent always sends an empty vector (`agent/src/link/handshake.rs:47`) and the hub never reads it (no consumer in `crates/**`; only a snapshot sample uses it). ARCHITECTURE §8 table still lists it as meaningful (`ARCHITECTURE.md:253`).
- FACT: the real, working feature mechanisms are separate and ad hoc:
  - skills: agent-advertised `SkillSummary` (name, description, risk, params, applies_to, manifest hash, timeout, verify, README <= 8 KiB: `runner/src/catalog/checks.rs:17`); hub-side per-host *disable* in `settings` key `host.<name>.skills`, enforced by `PolicyLink` which "only ever narrows what the agent itself allows" (`agent_server/skill_policy.rs:1-5,144`);
  - checks: `CheckInfo` + hub `CheckOverride{enabled, interval, thresholds}` pushed via `config_update`;
  - Docker: `DockerReport` up, tri-state down, with the owner-file-wins rule (ADR 0018:24-34; hub "cannot turn a false into true, and cannot turn a true off").
- FACT: `ReadTool` is a closed Rust enum pinned by gate test `model_sees_only_read_tools` (`engine/tests/gates.rs:32`). `RunReadSkill` is the one open door, decided by *advertised* risk on the hub (`engine/src/triage/tools.rs:154`) and by manifest risk on the agent (`agent/src/requests/skills.rs` `run_read_skill`).
- FACT: ARCHITECTURE §19 already earmarks "Operator mode" as an "agent-side capability behind its own manifest and plan approval" (`ARCHITECTURE.md:368`).

---

## 4. Evaluation of `03-capability-model.md`

### What is right
- Separating contract (capability) from provider and from skill, and wrapping skills first, matches the code: skills already have typed params, risk, verify, manifest hash (`03:7-13,69-75`).
- "Available does not mean authorized", "descriptor never grants authority", re-validate at the enforcement point (`03:53,38,49-50`) are the same stance as INV-3/INV-4.
- Not modelling observations as capabilities (`03:67`) is correct and keeps checks/signals separate.

### Lifecycle independence (discover / validate / enable / configure / authorize / execute / disable / audit)

| Stage | Today (FACT) | Gap against the proposal |
| --- | --- | --- |
| Discover | hello snapshot only; no refresh while connected; catalogue read at start | Needs a catalogue message, a change notification, and size control (hello is capped at 1 MiB with up to 8 KiB README per skill) |
| Validate | agent validates manifest on load (`runner/src/catalog/checks.rs`); hub trusts `SkillSummary` as decoded, never re-runs `SkillManifest::validate` on it | Hub must validate every offered descriptor against a hub-owned definition |
| Enable | three unrelated mechanisms (skill disable list, check overrides, Docker tri-state) | One enablement model with a single owner-ceiling rule |
| Configure | only check thresholds; skills have none | Per-capability config is unspecified in 03 |
| Authorize | risk class -> approval type; no scope, rate, TTL, or target policy beyond manifest regexes | `required_authority` is named in 03 but no policy model exists yet (belongs to doc 05) |
| Execute | `run_skill`/`run_tool`; no outcome-unknown, no idempotency | See 5 |
| Disable | hub narrow-only per host per skill; agent `allow_risk` coarse | Needs per-capability agent ceiling |
| Audit | `audit_log` rows per action transition (`engine/src/actions/audit.rs:11-31`); read-tool calls are not audit rows (INFERENCE: they live in triage transcripts) | Reads of sensitive capabilities need audit too |

Verdict: skills/checks/Docker can each be managed independently today, but only because there are three bespoke implementations. The proposal does not yet define the single state machine (offered -> known -> enabled -> authorized -> dispatchable) or who owns each transition.

### Can agents advertise capabilities that bypass hub policy? Yes, as 03 is written
- FACT: today the hub's gating inputs come from the agent. Approval type and the "no-approval read path" key off `SkillSummary.risk` as advertised (`engine/src/triage/tools.rs:154`, `triage/chat.rs:166`, `cards.rs:86`, `actions/recheck.rs:48`). A skill whose manifest says `read` is callable by the model without any human step; there is no plan hash for read skills.
- FACT: the descriptor in 03 puts `effect`, `risk`, `reversible`, `constraints`, `scope`, `verification`, `preconditions`, `timeout` on the provider-registered descriptor (`03:21-39`), and step 1 says providers register it. If those fields stay agent-authored, a lying, buggy or downgraded provider can mark a mutating operation `effect=read` and widen `scope`.
- INFERENCE: for a host-local agent the trust root is already the host owner, so the practical attacker is a compromised or misconfigured *offer*, a tampered skill folder, or prompt injection through agent-authored text, not a stranger. The risk rises sharply for "providers" that act beyond their host (router, hypervisor, cloud): an agent could claim authority over a scope it was never granted.
- FACT: agent-authored free text (`description`, params, README) is printed into the model prompt (`engine/src/triage/context.rs:248-262`). 03 adds `examples`, widening this injection surface.
- FACT: `PlanHash` binds skill name, manifest hash, host and canonical args (`approval.rs:16-27`) but not a resolved target identity: "restart container plex" binds the name, not the container id.

### Must the agent enforce a local allow-list?
- RECOMMENDATION: yes, and widen it. INV-3 (`ARCHITECTURE.md:322`) is the strongest property in the current design and ADR 0018 shows the pattern (owner's file wins; hub can narrow, never widen). Extend `[policy] allow_risk` to `[policy] allow_capabilities` (ids or families) with per-capability scope ceilings (allowed container names, units, interfaces, CIDRs), default-deny for any *new* mutating family, and agent-side enforcement of rate limit, TTL and management-path exclusion. The agent must not trust the hub's `constraints`; it applies the stricter of hub constraint and local ceiling.
- FACT: the agent can already do this cheaply: the request handler runs a fixed ladder before the runner (`requests/skills.rs`), so more rungs fit.

### Version skew between old agents and new capability definitions
- FACT: old agent + new hub: unknown request kinds are answered `unsupported` and the session survives (`link/mod.rs:299-303`). The sharp edge is *known kind, unknown content*:
  - `run_tool` with a tool name an old agent lacks fails the whole frame decode (`work.rs:301-322`), which `link::frame::recv` maps to `LinkError::Protocol` and ends the session (`frame.rs:32`; `link/mod.rs:221` `frame?`); it is non-final, so the agent reconnects and the hub's pending request fails. INFERENCE: adding a new `ReadTool` variant therefore flaps old agents; it is not a graceful "unsupported".
  - the reverse: a newer agent's `hello` with a value an old hub cannot parse (for example a new `Risk` variant, `core/.../manifest.rs:31`, has no `#[serde(other)]`) fails `decode` inside the hub's first-frame read (`hub/.../handshake.rs:55`, `conn.rs:68-82`) -> `ConnError::Protocol`, connection closed without a `reject`. The agent logs "hub closed the connection" and retries forever with 60 s backoff. Same hazard for the proposal's new `effect` classes (contain, verify).
  - the 1 MiB frame cap means a rich catalogue in `hello` can exceed it; the whole hello is then rejected before the handshake.
- FACT: version window: an additive minor bump to 1.2 would make hub 1.2 refuse every 1.0 agent as `incompatible`, which is final on the agent (it stops). Compatibility policy and capability evolution must not share one counter.
- FACT: `Result` codes already use `#[serde(other)] Unknown` (`work.rs:110,207`; `handshake.rs:246`): the pattern exists, just not for descriptors.

### Catalogue (11) entries that look unsafe or unrealistic
Judged against INV-2/3/5/6, ADR 0003 ("hub never executes"), ADR 0018 (Docker socket is root), and the missing outcome-unknown/idempotency machinery.

Unsafe unless a prerequisite lands first:
- `11 §1` Agent upgrade/restart and hub upgrade/restart: pushes executable code to a host (contradicts INV-3 "the hub cannot push scripts") and hub self-upgrade contradicts "the hub never executes commands" (ADR 0003:15). Restart of the agent mid-action destroys the very channel the result travels on. Keep out-of-band (package manager, installer) or require signed artifacts pinned on the host.
- `§1` Host reboot/shutdown, `§7` update apply/reboot coordination, `§11` power cycle, IPMI, smart PDU, UPS-driven shutdown: guaranteed "connection dropped mid-action". Needs outcome-unknown state and boot-id based reconciliation first (hello already carries `boot_id`: `HostFacts`), plus a dependency model that knows what powers the hub.
- `§3` Pull/redeploy/rollback images, container stop/pause/network-disconnect, `§6` isolate container: all need *write* access to the Docker socket, which ADR 0018:10 says is root on the host; its read-only rationale and `"auto"` default no longer hold. Needs its own ADR, a separate off-by-default switch not covered by `auto`.
- `§5/§6` Firewall block of an IP, restrict WAN SSH, remove port forward, segmentation, host quarantine: lockout is realistic because the agent dials *out* to the hub; a rule can sever the control link or the owner's session. Only acceptable with agent-side enforced exclusions (hub address, gateway, RFC 1918 and owner ranges, current SSH peers), native expiry (nft set timeout), dead-man revert, and outcome-unknown handling.
- `§6` Rotate exposed API key/certificate/credential, disable account/revoke token: secrets handling contradicts INV-5 unless a secret store and redaction design exists; undefined today.
- `§4` File deletion, volume destruction, formatting, backup pruning, pool repair/resilver, restore entire host: even "deny-default" invites a generic typed-path capability; there is no path-scope model. Recommend removing these from the catalogue (documented manual runbooks only).
- `§17` deploy, rollback deployment, rerun CI, create PR/issue; `§19` smart locks, alarms, smart plugs; `§16` resize/start/stop cloud instances; `§20` status-page updates: outside an overseer's trust envelope or require third-party credentials and public side effects; treat as non-goals until Q0.6 (third-party trust) is answered.
- `§21` retry/requeue message, `§8` failover/promotion: replay duplicates side effects; needs real idempotency keys, which do not exist.

Unrealistic as "capabilities with a typed contract" in the current architecture:
- `§12/§16` router, hypervisor, NAS, cloud, Home Assistant providers: the protocol is one agent per host and the hub rejects signals for another host (`session.rs:86`); `HostKind::External` hosts have no agent. These need hub-side providers holding third-party credentials, which is a different security design (ADR 0003 says the agent owns checks/skills/read tools). Do not describe them as extensions of the agent registry.
- `§15` "run parallel independent reads, serialize conflicting mutations", `§22` "policy simulation": need hub-side concurrency keys per capability; no such field in 03's descriptor.

Realistic and mostly already present: host/Docker/SMART/journal/ports reads; backup status via read skills; reachability probes; `§22` emergency stop (hub-side `PolicyLink` wrapper plus an agent-side kill file), per-host self-check.

### Other gaps in 03
- No `idempotency`, `outcome_unknown` policy, `concurrency_key`, or `lease/TTL` in the descriptor, although `11 §23` items 7 and 9 demand them.
- Two registries (hub and agent, `03:45`) with no statement of which is authoritative for authorisation. It must be the hub for policy and the agent for availability and ceilings.
- Opening the model to "discover" capabilities collides with ADR 0007/INV-1 ("closed `ReadTool` enum", gate `model_sees_only_read_tools`). The model surface must remain closed even if the capability set is open (see 5).
- `manifest_hash or equivalent` is not enough: bind capability id, contract version, provider implementation hash, resolved target identity and an expiry.

---

## 5. Recommendation: where capabilities are defined, and the protocol changes

### Decision: hub-owned typed contracts, agent-reported implementations (hybrid), skills as a wrapped tier
1. RECOMMENDATION, contract = hub side, typed, in `domovoi-core` (pure data, R2 compliant). A `CapabilityId` (`family.verb`, e.g. `container.restart`, `host.overview`) maps to a compiled-in `CapabilityDef { contract versions, input schema (typed struct like `ReadToolCall`), effect, risk floor, reversibility, verification, default timeout/output limit, idempotency class, outcome-unknown policy, concurrency key, default policy }`. Snapshot-tested like the wire format (R14). The hub, not the agent, owns `effect`, `risk`, `reversible`, `constraints`, `required_authority`.
2. RECOMMENDATION, offer = agent side, only facts about implementation: `{id, contract_versions, impl: native|skill, impl_hash, status: available|unavailable{reason}|locked_by_owner, discovered targets (bounded), local ceilings (scope allow-list, rate, TTL)}`. An offer for an id the hub does not define is ignored (or shown as "unknown", never dispatchable).
3. RECOMMENDATION, skills keep working as a tier: each skill is synthesised as `skill.<name>` with agent-declared params, but the hub applies floors: advertised `read` is only honoured if the operator acknowledged that manifest hash on the hub (changing the hash revokes it, same idea as today's plan-hash binding but extended to reads); `write`/`destructive` as today. This is a behaviour change from today's "advertised read is trusted"; call it out in the ADR.
4. RECOMMENDATION, native providers live in `domovoi-agent`/`domovoi-checks` as `Commands`-style argv templates with typed slots executed through the `ProcessExecutor` port; no new crate unless an ADR approves one (R1). Third-party/hub-side providers (router, hypervisor, cloud) are a separate ADR (credentials as `SecretString`, a port in core, a new adapter crate, runs in the hub, no `ProcessExecutor` involvement).
5. RECOMMENDATION, the model keeps a closed surface: meta-tools `list_capabilities`, `describe_capability`, `run_read_capability{id,args}` (only ids whose hub-owned effect is read) and propose-only `ActionProposal{capability,args}`. Extend, never weaken, `model_sees_only_read_tools`. Approval, plan hash, `HumanApproval::new` call-site restriction and destructive typed confirmation are unchanged (INV-1, INV-4, INV-6).
6. RECOMMENDATION, plan hash v2 = `plan:v2` over capability id, contract version, impl hash, host, canonical args, resolved target identity, and expiry. Keep v1 for legacy `skill.*` until migration (migrations are append-only, R13).
7. RECOMMENDATION, authority layering, narrow-only at each step: agent owner ceiling (file the hub cannot write) >= hub policy (per-host, per-capability, scope, rate, TTL, maintenance windows) >= per-action approval. Hub may narrow an agent, never widen it (ADR 0018 rule), and an agent-side `locked` state is reported but not overridable.

### Protocol changes (additive, in this order)
| # | Change | Kind | Compat |
| --- | --- | --- | --- |
| 1 | Define semantics of `hello.capabilities`: protocol *feature flags* (`catalog/1`, `run_capability/1`, `action_status/1`); echo accepted set in `welcome.features`. Stop overloading the word "capability" in the wire (the catalogue goes in a new field). | optional fields | old peers ignore; no minor bump needed |
| 2 | `hello.catalog_hash` + `hello.catalog_seq`; full descriptors travel in a paged `get_catalog`/`catalog` request (same chunking as `get_logs`, `work.rs:35-65`), so hello stays small and under 1 MiB | new kinds | old agents answer `unsupported`; hub falls back to legacy view |
| 3 | `catalog_changed{seq,hash}` agent -> hub when the local catalogue or availability changes; hub re-pulls | new kind | optional; reconnect still works |
| 4 | `run_capability{action_id, idempotency_key, capability, contract, args: Map<String,Value>, plan_hash, deadline}` / `capability_result{ok|err}`; new `err` codes with `#[serde(other)] Unknown`; payload decoded loosely, args validated semantically (a bad arg yields `invalid_args`, never kills the session, unlike `run_tool`) | new kinds | old agents: `unsupported`, hub marks capability not dispatchable on that host |
| 5 | Outcome resolution: agent keeps a small journal `{action_id, idempotency_key, started, finished?, result_hash}` (atomic file in `state_dir`, like `fs_atomic`); `get_action_status{action_id}` / `action_status`; agent re-sends the last unacknowledged results on reconnect with an `action_result_ack` | new kinds | feature-gated (flag 1) |
| 6 | `ActionStatus::OutcomeUnknown` (core + store text column, new migration) entered when the link drops or times out after dispatch; resolved by (5) on reconnect, otherwise surfaces as "verify manually" and blocks automatic retry; never auto-redispatched | core + store | existing `Failed` rows unchanged; hub restart recovery (`recover.rs:204`) moves to the new state |
| 7 | Tolerant decode for descriptors: parse `offers` as `Vec<Value>` and decode per item; unknown enum values become `Unknown` and mean "not executable"; a bad item is reported, not fatal. Add the same `#[serde(other)]` to `Risk` before introducing new classes. | proto + core | prevents the hello poison-pill loop in 4 |
| 8 | Agent never learns the hub's protocol minor today; add `welcome.protocol` so the agent can pick behaviours and print the right upgrade message | optional field | ignored by old agents |
| 9 | Fix the stranded-agent hazard: hard cuts use `MIN_AGENT_VERSION` (release), soft evolution uses feature flags; do not bump `minor` for optional additions (current practice, ADR 0018:32). Update `protocol.md` (says 1.0, code is 1.1). | policy | no wire change |

### Compatibility plan
1. Phase 0 (no behaviour change): hub-side "legacy capability view" synthesised from today's `ReadTool` set, `skills`, `checks`, `docker` report so the UI/engine can speak capability ids before any wire change. Old agents are fully supported forever through this adapter.
2. Phase 1: tolerant decode (row 7) and feature flags (row 1) ship first, as a release that changes nothing visible; wait one release window before using them.
3. Phase 2: catalogue pull, `run_capability`, journal and `action_status`; new agents advertise flags; hub dispatches `run_capability` only to hosts that advertised it, otherwise the legacy `run_skill`/`run_tool`.
4. Phase 3: native mutating capabilities (container restart first, because it mirrors the existing skill and uses the existing verify re-check); each behind agent `allow_capabilities` default-deny.
5. Hub policy lives in the `settings` table keyed like `host.<name>.skills` (`skill_policy.rs:27`), with an audit event per change (as ADR 0018's `settings.host_docker`).
6. New ADRs needed (stop-and-propose rule in AGENTS.md): (a) capability contracts and authority layering; (b) outcome-unknown and agent action journal; (c) hub-side providers and credential custody; (d) write access to container engines.

---

## 6. Open questions
1. Who may define capabilities in practice: only first-party code (hub release needed for each) or operator-installed packs? If packs, what is the signature/trust root (ties to `09-open-questions` Q0.6)? Until answered, skills are the extension point.
2. Is a read skill allowed to be trusted on the agent's word alone (today) or must the hub operator acknowledge the manifest hash? This changes default UX for existing installs.
3. Should the agent journal outcomes durably and for how long, and is it acceptable for `OutcomeUnknown` to block the loop guard slot (3/hour) or to count against it?
4. Self-disrupting capabilities (reboot, agent restart, firewall): do we want a two-phase protocol (`prepare` / `commit` with dead-man revert) or exclude them? 03 and 11 do not say.
5. Hub-side providers: does the hub get to hold third-party credentials (router, cloud)? This reverses ADR 0003's split for those providers and needs a secret-custody design (INV-5).
6. Target identity binding: is container id / unit invocation id / interface index part of the plan, and who resolves it (agent at dispatch, with a mismatch -> reject)?
7. Is a half-open connection detected fast enough for actions? Should the hub ping and close idle sessions (today only the agent pings, `session.rs`, `link/mod.rs:230`)?
8. Catalogue churn: re-announce on skill folder change (needs a watcher or SIGHUP; today a restart) and what the audit trail records when availability flips.
9. `hello.capabilities` is documented in `ARCHITECTURE.md:253` and `protocol.md:78` but unused; repurpose as feature flags (recommended) or deprecate? Either needs a snapshot update.
10. Per-capability audit of *reads* of sensitive data (users/groups, process lists, file integrity in 11 §1/§6): required, and where is it stored given retention rules (`ARCHITECTURE.md` §9)?
