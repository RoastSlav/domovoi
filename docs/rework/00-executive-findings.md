# 00 — Executive findings

Baseline: `overhaul-investigation` at `64117f0` (main + #112, #113). Planning output lives in
`docs/rework/`; the design pack it reviews is in `docs/design-pack/` and was not modified.

Files: `01` repository reality · `02` pack validation · `03` decisions (RW-01…24) · `04` target
architecture · `05` migration · `06` roadmap · `07` prompt index (draft) · `99` open decisions ·
`investigations/` raw evidence (A–G + inventory).

## What is sound

- The pack's direction: adaptive reasoning inside a deterministic control plane; available ≠
  authorized; knowledge, policy and experience kept separate; Operation separate from Incident;
  SQLite only, with no vector database, graph database, workflow engine or broker. The evidence
  supports all of it.
- The repository's safety core is real and worth keeping:
  - single-writer incident and action services;
  - the `HumanApproval` seal plus its gate tests;
  - plan hash checked by both hub and agent;
  - argv-only runner, closed read-tool set, channel-bound agent identity;
  - ~900 Rust tests, plus strong web guardrails.
- Evolve the existing code. A rewrite is not justified (RW-01).

## What needs correction in the pack

1. **Capability descriptors must be hub-owned** (RW-07). As written, the provider declares its own
   risk and scope. Today the hub already trusts an agent's `risk = read` label, and such a skill runs
   with no human step.
2. **Phases should not be the state machine** (RW-05). Use a coarse operation status plus typed,
   append-only steps. Phases become step labels. This is what makes "dynamic, yet resumable" possible.
3. **Experience is the journal, not a third log** (RW-15). The pack's `experience_events` would
   dual-write beside `incident_events` and `audit_log`.
4. **Knowledge model is too big** (RW-14). Use typed subject references and facts with provenance.
   Drop the generic `entities` table (it duplicates baselines) and numeric confidence.
5. **Policy precedence is underspecified** (RW-10). Proposed fix: deny wins, then the most specific
   rule decides; equal allows intersect; a fixed default table; `Destructive` cannot be
   policy-allowed in v1.
6. **The UI spec is graph-first and profile-first** (RW-21). It also conflicts with `DESIGN.md`
   (motion durations, the no-inline-style rule, the bundle budget). Proposed fix: ship a step
   timeline first and the graph after a spike; defer profiles.
7. **Offline emergency permits and L3 containment add risk with little v1 value** (RW-13).
   Recommend deferring both past v1.
8. **Gap analysis and lessons overstate current safeguards.**
   - Plan binding omits the script bytes, and the approve request carries no hash.
   - Verification and the triage queue are not restart-safe.
   - Doc 18 covers 110 of 113 PRs.

## What the pack misses entirely

- **Atomicity.** State, timeline and audit are written in separate statements (RW-03).
- **Outcome-unknown and idempotency.** The agent does not deduplicate, and a dropped link becomes
  `Failed` while the skill keeps running (RW-09).
- **Approval freshness.** Approved actions re-run after restart with no age or incident-open check
  (RW-08).
- **Agent-side proof of authorization.** `RunSkill` carries none. Autonomy built on the current wire
  format would make a compromised hub silent (RW-12).
- **Live-update replay.** SSE events have no ids and slow clients drop events (RW-18).
- **Protocol version rule.** A newer-minor agent is refused for good (RW-17).

## Important discoveries (current product defects; full table in `01` §11)

| Sev | Defect |
|---|---|
| H | Discord Approve/Deny buttons show no skill, host or args. The card text is model prose. |
| H | An admin session can set the LLM CLI provider's command (an arbitrary program on the hub) without re-entering the password. |
| H | Stale approvals run after restart. Separately, catch-up runs before agents reconnect, so valid approvals are rejected. This was found by reading code; it still needs a test. |
| H | Audit failures are swallowed. The loop guard is rebuilt from audit rows that may be missing. |
| H | Triage queue and verification are lost on restart. Admission budget fails open. There is no total prompt cap. |
| M | `/healthz` always returns ok; `/metrics` is unauthenticated; backups are unverified and there is no restore command. |

## Recommended direction

1. **M0 — fix the current product and release v0.1.** Every M0 fix is also a rework prerequisite.
2. **M1 — execution foundation:** idempotency, outcome-unknown, plan hash v2, single execution
   boundary.
3. **M2 — capability contracts:** hub-owned contracts and the registry.
4. **M3 — Operation aggregate.** Triage becomes an Investigate operation with identical behaviour.
   Live updates move to SSE v2.
5. **M4 / M5 / M6 in parallel:** dynamic investigation, knowledge, and the Operator.
6. **M7 — policy in shadow mode.**
7. **M8 — L2 autonomy,** behind a host-local ceiling and guardrails.

The UI track runs alongside, starting at M3. Details are in `06`.

## Decisions I need from you (top five; all 15 in `99`)

1. **O-1:** fix and release v0.1 first?
2. **O-2:** accept the core corrections (hub-owned contracts, operation model, journal as
   experience, unit of work)?
3. **O-3:** anchor autonomy with a host-local `unattended` list, or with signed policy bundles?
4. **O-4:** defer L3 containment and offline permits past v1?
5. **O-8:** step timeline first and graph second, in v1?

The detailed implementation prompts (08+) wait for these answers.
