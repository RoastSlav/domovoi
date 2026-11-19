# 0024 The capability registry replaces the skill policy wrapper

- Status: accepted
- Date: 2026-11-19
- Supersedes the hub half of [0023](0023-capability-objects-one-checker-bounded-evidence.md):
  the `PolicyLink` wrapper is gone; the rest of 0023 stands.

## Context

[0023](0023-capability-objects-one-checker-bounded-evidence.md) put the hub's per-host skill
enablement in a wrapper around the agent link (`agent_server/skill_policy.rs`), so the engine
could not see a skill the owner had switched off. The wrapper answered every raw link operation
and narrowed `skills()`.

The capability registry supersedes it. Availability is now a hub-owned answer per
`(host, contract)`: the compiled contracts, the agent's catalogue and `hello.skills`, the host's
own status words, hub enablement, the raise-only skill floors and the agent's ceiling summary
are combined in one registry, and the engine asks that registry through the
`CapabilityRegistry` port. Enablement is enforced at the two boundaries the engine owns
(`collect_evidence` and `execute`), not by a link wrapper.

## Decision

- `agent_server/skill_policy.rs` and `agent_server/skill_policy_key.rs` are deleted. The
  disabled-skill setting keeps its key and value (`host.<name>.skills`,
  `{"disabled": [..]}`) and its reader and writer (`capability::Enablement`).
- `AgentServer::policy_link` is deleted. The server keeps one accessor for the raw link,
  `AgentServer::link`, reachable only inside the hub crate; the runtime composition root is its
  only caller outside `agent_server/`.
- The one place the hub turns that link into narrow capability objects stays
  `runtime/caps.rs`. It now also builds `LinkCapabilities`, the registry's raw-link view
  (`HostCapabilities`: availability, announced catalogue hash, catalogue pages, the 1.2 status
  words, announced skills), so the registry and its module never name the link (EXEC-4).
- The boundary cards for `run_tool`, `start_skill`, `skill_result` and `run_check` no longer
  have the policy wrapper site: the raw operations are named once each, in the hub link's own
  `AgentLink` implementation and in `runtime/caps.rs`. `PolicyLink` leaves the refused type
  names, and `skill_policy.rs` leaves the exact-file allowlist.
- Nothing else changes: EXEC-1 to EXEC-4 keep their meaning, their scanned crates and their
  default-deny behaviour.

## Consequences

- A disabled skill is refused by the read boundary and the execution boundary with the same
  sentence, taken from one constant, instead of by a link that hid it.
- The engine needs one more port in its dependency structs (`CapabilityRegistry`), and the
  testkit needs `FakeCapabilityRegistry` beside `FakeAgentLink`.
- A hub module that wants agent facts asks the capability layer, not the link.
