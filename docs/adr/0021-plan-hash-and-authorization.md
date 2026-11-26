# 0021. Plan hash v2 and the authorization wrapper

- Status: Accepted
- Date: 2026-11-16

## Context

The v1 plan hash binds the skill name, the manifest hash, the host and the canonical arguments. It does not bind the entry executable the agent will run, the capability contract and version, or the enrolled agent key, so re-enrolment and a replaced entry file leave an approval standing. The approval value was also only `HumanApproval`; a later milestone needs a second case, a policy permit, without opening a construction path that a transport could use to approve a run.

Two separate questions had to be answered together: which plan family binds an action, and which authorization cases may exist. Both need enforcement that the compiler cannot provide on its own: a constructor called by the wrong layer, or a second implementation of the v2 domain line, is invisible to type checking.

## Decision

- `PlanHash` keeps one type and gains a v2 domain line (`plan:v2`). The family is chosen by the presence of `plan_hash_v2` on the stored row: `NULL` means the legacy v1 binding. `ActionRequest::plan_hash_for_approval` is the only selector.
- v2 hashes exactly five inputs: the capability contract id and version, an `ImplHash` over the manifest hash and the required entry-script digest, a canonical target (host, enrolled key fingerprint, check and optional target), the canonical arguments and the execution id. The encoding is length-prefixed, and the domain line prevents a v1 and a v2 encoding from colliding.
- `KeyFingerprint` is the SHA-256 of the host's enrolled ed25519 public key, computed by the hub from its own record. It is always present in a v2 target and never accepted from an agent, an incident or a client.
- `Authorization` is a wrapper with private internals, not a public enum. The two cases are reachable only through `Authorization::human` and `Authorization::policy`; there is no `Default`, no `Deserialize` and no `From`/`Into`. A public variant would be a construction path the call-site scan cannot see, so the canonical sketch of an enum is deliberately not followed.
- `PolicyPermit` has private fields and one constructor, `PolicyPermit::issue`, a data factory that evaluates nothing. It takes its arguments in `PolicyPermitParts` so the factory stays a single-argument function and clippy's argument-count lint does not need an exception. No operational issuer exists.
- The enforcement is the existing line-based source scan, extended to every authorization constructor (`HumanApproval::new`, `HumanApproval::restore`, `PolicyPermit::issue`, `Authorization::human`, `Authorization::policy`), to the `plan:v2` literal and to a `pub enum Authorization` anywhere but its own module. It is textual: a macro, a re-export under another name or a generated call is not caught. No comment claims a compile-time seal.
- No `clippy.toml` `disallowed-methods` entry is added for these constructors. A legitimate call site would need `#[allow(clippy::disallowed_methods)]`, and the architecture scan rejects that attribute outside `domovoi-runner`; the change would break the build or force a weakening of an existing rule. The text scan already runs in CI.
- At M1 the writer keeps two separately named hashes: the approval-family hash (v2 for new actions) used by the approve request, the DTO and the dispatch guard; and the always-v1 `runskill_plan_hash` sent in the existing `RunSkill` request, where the agent still recomputes v1. Sending v2 there would make every run fail on the agent.

## Consequences

- A re-enrolment, a replaced entry executable, a changed contract, a changed target, a changed argument or a changed execution identity invalidates an approval at dispatch, as a rejection rather than an error.
- Legacy actions keep v1 for their whole life and are labelled `v1 binding` in the API and the UI; nothing migrates them.
- The policy case is refused by the writer with a named error. No permit columns exist until an issuer does.
- The scan's ownership table is now fixed before the policy layer exists: the policy prefixes name a module that M1 does not yet create.
- A host without a live enrolled agent cannot produce a v2 plan at proposal time and is rejected at dispatch.

## Alternatives considered

- A public `Authorization` enum: simpler to match, but a transport could construct a variant directly and the scan could not see it. Rejected.
- Serializing the authorization case as an enum in the wire or the database: the wire is frozen and the database only needs the discriminator. Rejected.
- Extending v1 in place: changes the meaning of every stored v1 hash and makes old approvals ambiguous. Rejected.
- A clippy `disallowed-methods` entry: conflicts with the existing attribute rule as described above. Rejected.
