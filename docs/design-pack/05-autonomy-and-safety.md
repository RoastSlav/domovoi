# 05 — Autonomy and safety

## Autonomy model

Autonomy is a permission to perform a defined operation under defined conditions, not a global switch that makes the model trusted.

| Level | Meaning | Typical behavior |
|---|---|---|
| **L0 — Observe** | Read-only. | Detect, collect evidence, diagnose, report and monitor. |
| **L1 — Assisted** | The system can prepare plans, but a human authorizes changes. | Propose restart/configuration/security actions and wait for approval. |
| **L2 — Bounded autonomy** | Owner-authorized, low-risk and recoverable operations can run without per-action approval. | Restart an approved service, renew a certificate, clear a bounded queue, unload an unused model. |
| **L3 — Security autonomy** | Owner-authorized, narrowly scoped protective operations can run quickly. | Temporarily block a clearly identified external source. Isolating a workload is DEFERRED. |

These are capability categories, not a single host-wide ranking. A host may allow L2 service recovery and L3 network containment while explicitly denying deletion and host reboot.

**v1 scope (owner decisions D-03, D-04).** L2 ships as a reviewed set of reversible operations. L3 also ships in v1, as milestone M9, under a general **Contain** effect class whose first and only v1 capability is `firewall.block_source` (owner decision D-17). Every unattended step is bounded by a **host-local ceiling** (below). Offline emergency permits and owner-signed policy bundles are DEFERRED past v1 (`docs/rework/06` "Deferred and after-v1").

## Policy scope and precedence

**Agreed direction:** support rules scoped by host, capability/category and target, plus defaults. **Changed by owner decision D-05:** the first draft said "explicit deny always wins". The owner replaced that principle: a more specific rule overrides a broader one, even a broader Deny, so that "deny everything on this host, except restarting this one container" works as written.

**Precedence (owner decision D-05; formalisation in [RW-10](../rework/03-architecture-decisions.md)):**

1. A rule's scope is its host, capability and target dimensions. Rule A is **strictly more specific** than rule B when A's scope is a subset of B's on every dimension and a strict subset on at least one. This is a partial order; there is no numeric score.
2. The strictly more specific matching rule overrides the broader one, **including a broader Deny**.
3. At **equal specificity, Deny wins**.
4. Conflicting rules that are **incomparable** (neither is more specific) resolve by **Deny > RequireApproval > Allow** (owner decision D-19). Publishing a policy version warns about both kinds of conflict, equal-scope rules with different outcomes as well as incomparable ones, shows how each resolves, and requires the owner to acknowledge them (RW-10, `docs/rework/04 §7.6`).
5. No matching rule → approval required or denied according to the operation class; never implicitly allow.
6. Host ids only in v1; host tags are DEFERRED (`docs/rework/06` "Deferred and after-v1").
7. **Locked Deny (owner decision D-26).** The owner may mark a Deny rule as locked (off by default). A locked Deny is a hard prohibition: no narrower rule overrides it, it is shown distinctly from ordinary rules in the editor, simulator, explanation and trace (with what ordinary resolution would have decided), and changing it needs step-up authentication.
8. **A policy Deny is never offered for approval (owner decision D-23).** The owner must change the policy; there is no approval bypass. A `RequireApproval` outcome, by contrast, is offered to the owner.

**Hard limits are not rules.** They apply after rule resolution and cannot be overridden by any rule, however specific: the host-local ceiling, Destructive operations never being policy-allowed in v1 (owner decision D-06), the mandatory containment exclusions, unknown or stale safety-critical facts never yielding Allow, and a host whose agent still runs as root never yielding an unattended Allow (owner decision D-29). A locked Deny (below) behaves the same way for the one prohibition the owner wrote it for.

The evaluator is pure and deterministic, produces an explainable decision record naming the winning rule, and is the same function the simulator calls.

Policy should constrain more than a risk label:

- Host and target scope (including allowed entities/paths/IP ranges).
- Operation and capability version.
- Preconditions and acceptable evidence freshness.
- Maximum effect, rate, frequency and concurrency.
- TTL/automatic expiry for temporary changes.
- Reversibility and rollback requirements.
- Trusted management networks, VPN paths and critical services.
- Allowed time windows or maintenance state where relevant.
- Whether the operation remains allowed when the hub is offline (offline operation is DEFERRED past v1; in v1 nothing runs unattended while the hub is unreachable).

## Illustrative per-host policy profile

This example captures the intended granularity; it is **not** a default policy or an instruction to enable these permissions automatically. Because a more specific rule beats a broader Deny, a "deny" written at host scope can be overridden by a narrower Allow; where the owner wants a denial to be final for a capability, it should be written at that capability's scope, or as a locked Deny (owner decision D-26), or be a hard limit (Destructive operations always are).

| Scope | Automatically allowed | Approval required | Explicitly denied |
|---|---|---|---|
| **NAS** | Restart the Plex container/service; restart other specifically allowlisted application containers; temporary block of a clearly identified external source through an approved firewall provider. | Restart SSH; OS/package updates; host reboot; changes to storage layout. | Delete user files, media, backups or storage pools; destructive cleanup. |
| **AI host** | Restart Ollama; unload a model from memory when the target is known and the operation is safe. | Host reboot; GPU driver changes; model downloads or environment changes with broad impact. | Delete model files or user data. |

The exact capability names and constraints would be configured per environment. The example demonstrates that one machine can have high autonomy for selected operations while retaining explicit denials for data loss, and another can allow only a narrow subset of local AI operations.

## Unattended execution guardrails

Beyond the policy decision, an unattended (policy-authorized) step is bounded by three further controls.

- **Host-local ceiling (owner decision D-03, [RW-12](../rework/03-architecture-decisions.md)).** Owner-controlled configuration on the agent's host lists, per capability, the explicit targets, a maximum rate per hour, constrained arguments where the contract allows them, and for Contain capabilities a maximum lifetime, a maximum number of active containments and mandatory management-path exclusions (owner decision D-18). No entry means no policy-authorized execution. The hub cannot change or widen it, the UI shows it read-only, and hub policy may only narrow it. Human-approved and policy-authorized executions stay distinguishable. A separate agent-side **mutation rate limiter** applies to all mutating executions, human- and policy-labelled (owner decision D-27), which bounds the damage a lying hub could do. It survives a reboot: entries are aged by the boot's monotonic clock and entries from before a reboot are kept for a full window, so a reboot or a wall-clock change never opens an unrestricted window (RW-38). A request it refuses is recorded as `Refused` with the reason (for example the rate limit), and the approval card shows that reason.
- **Target binding (owner decision D-07, RW-23).** An unattended step may target only the operation's **pinned subjects** plus targets **explicitly named by the matching owner-authored rule**. A target outside that set can only be routed to human approval, flagged as outside the operation's scope. Pinned subjects change only by owner command.
- **Operational brakes.** Per-rule and global rate limits, a human-only global **pause autonomy** switch, a circuit breaker that suspends a rule after repeated failed or unknown results, and shadow mode for every new rule.

**What the ceiling does not do.** It is an enforcement limit, not a signature. `authorization_kind` is a label the hub sets, not proof. A compromised hub can label a request `human` and so bypass the ceiling; this is an **accepted v1 limitation** (owner decisions D-03, D-18, D-27). It is still limited by everything the agent checks for every mutating request: its local capability catalogue and manifest, the plan hash, the arguments, `allow_risk` and the mutation rate limiter. A hub permit is therefore not a cryptographic proof of owner approval. Owner-signed approvals and policy bundles are the fix and are DEFERRED past v1 (`docs/rework/06` "Deferred and after-v1").

## Knowledge is not authority

User preferences, environment facts, recurring fixes and LLM confidence do not grant permission. A statement such as “restarting Plex usually fixes this” may influence planning; only a separate explicit policy rule can authorize that restart automatically.

Learning and permission changes are separate user actions. An experience suggestion may say “you have approved this three times; consider allowing it automatically”, but must not apply the rule.

## Separate authorization types

Keep the current human approval path distinct from autonomous authorization. The target introduces a typed policy permit that records the owner-defined rule, policy version, relevant conditions and execution scope (RW-11). Do not create a fake `HumanApproval` or allow model output to satisfy a human gate. Be precise about what enforces this: `HumanApproval` is a public type restricted by lint rules and a line-based scan in CI, not sealed by the compiler. Those controls stop careless code paths in review; they are not a runtime boundary against a compromised hub, which is why the plan hash, the agent's local checks and the audit chain exist.

Every action should carry a decision trace: requested operation, relevant policy rule/version, decision, conditions evaluated, actor/principal, plan hash, execution outcome and verification. The UI must make clear whether a human approved it or a policy authorized it. Past approvals and past successes are evidence for the reasoner and never authorization: they cannot satisfy a policy condition.

**Audit tamper evidence (owner decision D-14, RW-27).** Audit entries are hash-chained, computed inside the same transaction as the state change. The honest threat model: the chain detects accidental corruption, out-of-band edits and deletion in the middle of the chain; it does **not** stop an attacker who can rewrite the whole database and recompute the chain. As a cheap external anchor, the current head hash appears in the daily digest, so the owner holds off-host copies. Stronger anchoring is DEFERRED (`docs/rework/06` "Deferred and after-v1"). History that predates the chain stays unchained and is anchored once by a digest; verification and the audit page report that legacy part and the chained part separately.

Approval binding is staged (RW-11). The current action flow (F4) binds an approval to the existing plan hash v1: the manifest, the host and the arguments, not the entry-script bytes or the target's identity. M1 introduces plan hash v2 (script bytes, resolved target identity, execution id), and actions created before that keep v1 binding until they finish, labelled as such. M3 moves approval onto Operation steps. Systemd unit targets are identified by canonical id plus a hash of the fragment and drop-ins; generated and transient units are not valid mutation targets, and referenced files such as `EnvironmentFile=` and binaries are not bound, which the approval card states (RW-34).

The current implementation is human-approval-only for mutations. Treat that as a migration starting point, not as a permanent design limit. The target adds a separate owner-policy authorization path; never fabricate a human approval. Replace the current gate only when a typed policy decision, local enforcement, audit, migration and failure tests are ready together.

## Offline emergency authorization (DEFERRED past v1)

**Owner decision D-04:** offline emergency permits are **not part of v1**. In v1 an agent runs nothing unattended without a live hub permit inside its host ceiling. The design below is the direction for the later release; it is not open for v1. It is DEFERRED because it needs signed bundles first and carries clock, replay, revocation and lockout risks (`docs/rework/06` "Deferred and after-v1").

For security or protective operations that must work during hub disconnection, the agent may hold a small, signed, time-limited emergency policy cache. It must be:

- Explicitly created and authorized by the owner while connected.
- Narrow in host, capability, target and effect.
- Versioned, expiry-bound and revocable on reconnect.
- Unable to grant new capabilities or broaden existing scope.
- Audited locally in a tamper-evident spool and reconciled with the hub later.
- Safe under restart, clock uncertainty and duplicate execution.

When the cached permit is stale, expired or ambiguous, fail closed. This mechanism is not general offline autonomous reasoning with unrestricted powers.

## Containment in v1 (owner decision D-04, RW-13)

Containment (L3) ships in v1 as milestone M9, after bounded L2 autonomy is complete. It is a general **Contain** class, so the safeguards below apply to any contain capability, not just firewall rules.

- **First and only v1 capability (owner decisions D-17, D-22):** `firewall.block_source`, a temporary inbound block of one external address or small prefix. The block carries a kernel-enforced nftables timeout, so it expires even if the agent, the hub or the helper is down. Applying it is a two-step confirmation: the first element lives 30 s, and only a hub confirmation that management reachability still works extends it to the full TTL, by one atomic delete-and-add in a single nftables transaction (never by assuming that adding an existing element refreshes it); an unconfirmed block expires by itself. Only the root-owned helper's `nft` group touches nftables (an owner-enabled group, not on in a fresh install), and containment is not production-ready until the nftables research item R-2 is complete (`docs/rework/04 §9.1`).
- **DEFERRED:** `container.pause` and `unit.stop_temporary`. Their only expiry today would be an agent-side timer, which is not reliable automatic expiry (a paused container stays paused if the agent is down). They may be reconsidered in v1.x once expiry is independent of the agent and recovery is proven (the checklist in `docs/rework/04 §9.3`); the v1 helper has no pause, unpause or unit-stop operation, so reconsidering needs a helper ADR adding a typed operation with its own independent expiry, and the helper never offers generic `systemd-run`, `run_as` or command execution. Container network isolation, SSH session termination and account lock are likewise deferred for weaker recovery or lockout stories (`docs/rework/06` "Deferred and after-v1").
- **Triggering.** Automatic containment is eligible only when a deterministic detector emitted a typed signal whose structured fields name the target (for example a source address) and that target is the operation's pinned subject. **Free-form model output never triggers or targets automatic containment.** The model may investigate, explain and recommend; its recommendations go through human approval.
- **Management paths (owner decision D-28).** Unattended containment on a host requires owner-confirmed, current management paths, and every confirmed path must be covered by the effective exclusion set. That set has one authoritative source, the root-owned `[safety]` section of `/etc/domovoi/helper.toml`; the helper enforces it on every applicable operation and the agent reports its hash and a summary as returned by the helper (`safety.read`), never from its own configuration. Dynamic elements (the hub address from root-owned agent config, the kernel-observed link peer, loopback, link-local, interface addresses) are established by the helper, never supplied by the agent, and an element the helper cannot establish means containment is refused. Unknown, stale, conflicting or uncovered paths mean it is refused, a mismatch or any network or exclusion change blocks it until the owner re-confirms, and the helper refuses an overlapping target regardless of the hub (RW-37). This affects neither monitoring nor other human-approved work. The UI shows the confirmed paths beside the helper's effective exclusion set, where each came from, how fresh it is and which containment is blocked.
- **Safeguards.** Expiry independent of the main agent on every containment, a watchdog for connectivity effects (the 30 s two-step above), caps on simultaneous containments, verification that management access still works, outcome-unknown handling, and drills before any host is enabled unattended (owner decision D-22). The hub cannot empty the mandatory exclusions.

## Security containment example: suspected SSH brute force

A safe flow is evidence-first and protects the management path:

1. Correlate authentication failures and identify source addresses, destinations, timing and whether any login succeeded.
2. Determine whether sources are external or internal; compare against explicitly trusted admin/VPN ranges and active management sessions.
3. Identify where SSH is exposed and which provider controls the relevant firewall (host firewall, router or cloud rule).
4. Choose the narrowest owner-authorized containment, preferably a temporary block of a specific external source with a TTL.
5. Enforce exclusions for trusted management addresses, VPN endpoints, loopback and critical infrastructure; cap the number and duration of simultaneous blocks.
6. Verify the block is active and that trusted management access still works.
7. Monitor for recurrence or displacement to other sources; report the outcome and expiry.

Do not reflexively stop SSH or apply a broad firewall policy: that can lock the owner out and may not contain the real source. If the enforcement point or trusted path cannot be established, ask the owner or escalate.

## Hard safety boundaries

- Destructive data operations (delete files, pools, backups, databases or user data) are **never policy-allowed in v1** (owner decision D-06): the evaluator enforces this as a hard limit that no rule can override. A human can still approve one, with typed confirmation.
- Policy/configuration changes, host reboot, updates, credential changes and broad network changes require explicit handling; they are not ordinary L2 actions.
- The model cannot modify its own policy, grant itself capabilities, approve its plan or disable audit.
- A capability must declare target, effect, prerequisites, rollback/expiry and verification. If it cannot be bounded, it is not eligible for autonomous execution.
- Agent-side validation is mandatory even after hub authorization.
- The agent gets no general Docker socket write access, which would be root on the host. The agent runs as an unprivileged user (owner decision D-29); Docker write operations and other privileged host operations go through one narrow, root-owned first-party helper with a root-owned allowlist and its own ADR, installed only by the owner-controlled OS path and never by a package or skill. Docker write capabilities are unavailable for unattended use until it exists (owner decisions D-24, D-29, RW-39). A fresh install enables only the read groups and `safety.read`; the write groups (`docker.write`, `systemd.write`, `nft`) need explicit owner configuration of the group and its allowlist plus their milestone prerequisites (M8, M9). `docker.write` is restart only: there is no pause or unpause.
- The helper allowlist bounds which privileged operations and targets exist; it is not execution authorization. The helper enforces groups, canonical targets, protected resources, management-path exclusions, containment TTL and active limits, argument validation, timeouts, output caps and a helper-side mutation limit; it does not verify approval or policy. Approval, `allow_risk`, the ceiling and the rate limiter are agent-side checks. Residual risk: a compromised control agent can call the helper directly within the helper's limits; owner-signed approvals are DEFERRED (RW-39, RW-12).
- Skills and packages run through the skill runner as `domovoi-skill`, never as root and never as the control agent `domovoi-agent`. That identity has no access to the helper socket, the skill-runner socket, agent state or key, or `/etc/domovoi`, and package files are read-only to it; the helper's peer-uid check is only one layer, because skills never run as the permitted uid (RW-39, RW-26). A skill whose `run_as` is not `domovoi-skill` is an open owner question (O-30; recommended `Unavailable(run_as_unsupported)`, not decided). Platforms without the skill runner (R-9) run without skills. A root install is labelled and gets no unattended execution.
- Unknown, stale, conflicting or unavailable safety-critical context means no autonomous execution.
