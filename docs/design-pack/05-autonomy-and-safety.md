# 05 — Autonomy and safety

## Autonomy model

Autonomy is a permission to perform a defined operation under defined conditions, not a global switch that makes the model trusted.

| Level | Meaning | Typical behavior |
|---|---|---|
| **L0 — Observe** | Read-only. | Detect, collect evidence, diagnose, report and monitor. |
| **L1 — Assisted** | The system can prepare plans, but a human authorizes changes. | Propose restart/configuration/security actions and wait for approval. |
| **L2 — Bounded autonomy** | Owner-authorized, low-risk and recoverable operations can run without per-action approval. | Restart an approved service, renew a certificate, clear a bounded queue, unload an unused model. |
| **L3 — Security autonomy** | Owner-authorized, narrowly scoped protective operations can run quickly. | Temporarily block a clearly identified external source or isolate a specific unhealthy/compromised workload. |

These are capability categories, not a single host-wide ranking. A host may allow L2 service recovery and L3 network containment while explicitly denying deletion and host reboot.

**v1 scope (owner decisions D-03, D-04).** L2 ships as a reviewed set of reversible operations. L3 also ships in v1, as milestone M9, under a general **Contain** effect class that is not limited to firewall blocking. Every unattended step is bounded by a **host-local ceiling** (below). Offline emergency permits and owner-signed policy bundles are after v1.

## Policy scope and precedence

**Agreed direction:** support rules scoped by host, capability/category and target, plus defaults. **Changed by owner decision D-05:** the first draft said "explicit deny always wins". The owner replaced that principle: a more specific rule overrides a broader one, even a broader Deny, so that "deny everything on this host, except restarting this one container" works as written.

**Precedence (owner decision D-05; formalisation in [RW-10](../rework/03-architecture-decisions.md)):**

1. A rule's scope is its host, capability and target dimensions. Rule A is **strictly more specific** than rule B when A's scope is a subset of B's on every dimension and a strict subset on at least one. This is a partial order; there is no numeric score.
2. The strictly more specific matching rule overrides the broader one, **including a broader Deny**.
3. At **equal specificity, Deny wins**.
4. Conflicting rules that are **incomparable** (neither is more specific) resolve by the recommended default **Deny > RequireApproval > Allow**. This tie-break is **open** for owner confirmation (see `09`).
5. No matching rule → approval required or denied according to the operation class; never implicitly allow.
6. Host ids only in v1; host tags come after v1.

**Hard limits are not rules.** They apply after rule resolution and cannot be overridden by any rule, however specific: the host-local ceiling, Destructive operations never being policy-allowed in v1 (owner decision D-06), the mandatory containment exclusions, and unknown or stale safety-critical facts never yielding Allow.

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
- Whether the operation remains allowed when the hub is offline (after v1; in v1 nothing runs unattended while the hub is unreachable).

## Illustrative per-host policy profile

This example captures the intended granularity; it is **not** a default policy or an instruction to enable these permissions automatically. Because a more specific rule beats a broader Deny, a "deny" written at host scope can be overridden by a narrower Allow; where the owner wants a denial to be final for a capability, it should be written at that capability's scope, or be a hard limit (Destructive operations always are).

| Scope | Automatically allowed | Approval required | Explicitly denied |
|---|---|---|---|
| **NAS** | Restart the Plex container/service; restart other specifically allowlisted application containers; temporary block of a clearly identified external source through an approved firewall provider. | Restart SSH; OS/package updates; host reboot; changes to storage layout. | Delete user files, media, backups or storage pools; destructive cleanup. |
| **AI host** | Restart Ollama; unload a model from memory when the target is known and the operation is safe. | Host reboot; GPU driver changes; model downloads or environment changes with broad impact. | Delete model files or user data. |

The exact capability names and constraints would be configured per environment. The example demonstrates that one machine can have high autonomy for selected operations while retaining explicit denials for data loss, and another can allow only a narrow subset of local AI operations.

## Unattended execution guardrails

Beyond the policy decision, an unattended (policy-authorized) step is bounded by three further controls.

- **Host-local ceiling (owner decision D-03, [RW-12](../rework/03-architecture-decisions.md)).** Owner-controlled configuration on the agent's host lists, per capability, the explicit targets, a maximum rate per hour, constrained arguments where the contract allows them, and for Contain capabilities a maximum lifetime and maximum number of active containments. No entry means nothing runs unattended. The hub cannot change or widen it, the UI shows it read-only, and hub policy may only narrow it. Human-approved and policy-authorized executions stay distinguishable. The agent applies its rate limit to both kinds of execution, which bounds the damage a lying hub could do.
- **Target binding (owner decision D-07, RW-23).** An unattended step may target only the operation's **pinned subjects** plus targets **explicitly named by the matching owner-authored rule**. A target outside that set can only be routed to human approval, flagged as outside the operation's scope. Pinned subjects change only by owner command.
- **Operational brakes.** Per-rule and global rate limits, a human-only global **pause autonomy** switch, a circuit breaker that suspends a rule after repeated failed or unknown results, and shadow mode for every new rule.

**What the ceiling does not do.** It is an enforcement limit, not a signature. A compromised hub could label a dispatch as human-approved and run anything the host's local risk allowlist permits; that is today's exposure, now rate-bounded. A hub permit is therefore not a cryptographic proof of owner approval. Owner-signed approvals and policy bundles, planned after v1, are the fix.

## Knowledge is not authority

User preferences, environment facts, recurring fixes and LLM confidence do not grant permission. A statement such as “restarting Plex usually fixes this” may influence planning; only a separate explicit policy rule can authorize that restart automatically.

Learning and permission changes are separate user actions. An experience suggestion may say “you have approved this three times; consider allowing it automatically”, but must not apply the rule.

## Separate authorization types

Keep the current human approval path distinct from autonomous authorization. The target introduces a typed policy permit that records the owner-defined rule, policy version, relevant conditions and execution scope (RW-11). Do not create a fake `HumanApproval` or allow model output to satisfy a human gate. Be precise about what enforces this: `HumanApproval` is a public type restricted by lint rules and a line-based scan in CI, not sealed by the compiler. Those controls stop careless code paths in review; they are not a runtime boundary against a compromised hub, which is why the plan hash, the agent's local checks and the audit chain exist.

Every action should carry a decision trace: requested operation, relevant policy rule/version, decision, conditions evaluated, actor/principal, plan hash, execution outcome and verification. The UI must make clear whether a human approved it or a policy authorized it. Past approvals and past successes are evidence for the reasoner and never authorization: they cannot satisfy a policy condition.

**Audit tamper evidence (owner decision D-14, RW-27).** Audit entries are hash-chained, computed inside the same transaction as the state change. The honest threat model: the chain detects accidental corruption, out-of-band edits and deletion in the middle of the chain; it does **not** stop an attacker who can rewrite the whole database and recompute the chain. As a cheap external anchor, the current head hash appears in the daily digest, so the owner holds off-host copies. Stronger anchoring is after v1.

The current implementation is human-approval-only for mutations. Treat that as a migration starting point, not as a permanent design limit. The target adds a separate owner-policy authorization path; never fabricate a human approval. Replace the current gate only when a typed policy decision, local enforcement, audit, migration and failure tests are ready together.

## Offline emergency authorization (after v1)

**Owner decision D-04:** offline emergency permits are **not part of v1**. In v1 an agent runs nothing unattended without a live hub permit inside its host ceiling. The design below is the direction for the later release; it is not open for v1.

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

- **Proposed first set (open for owner confirmation):** a temporary inbound block of one external address or small prefix using a kernel-enforced timeout (`firewall.block_source`); pausing one named container with a time limit (`container.pause`); stopping one named systemd unit with a time limit and a hard-coded exclusion of management units (`unit.stop_temporary`). Container network isolation, SSH session termination and account lock are later candidates because their lockout or recovery stories are weaker.
- **Triggering.** Automatic containment is eligible only when a deterministic detector emitted a typed signal whose structured fields name the target (for example a source address) and that target is the operation's pinned subject. **Free-form model output never triggers or targets automatic containment.** The model may investigate, explain and recommend; its recommendations go through human approval.
- **Safeguards.** Mandatory management-path exclusions (owner-confirmed facts; unknown means refuse), expiry on every containment, caps on simultaneous containments, a dead-man timer, verification that management access still works, outcome-unknown handling, and drills before any host is enabled.

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
- Unknown, stale, conflicting or unavailable safety-critical context means no autonomous execution.
