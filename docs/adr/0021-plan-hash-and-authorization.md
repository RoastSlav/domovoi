# 0021. Plan hash and the authorization wrapper

- Status: Accepted
- Date: 2026-11-16

## Context

An approval must bind everything that decides what will run: the capability contract and version, the implementation the agent will execute, the target and the enrolled agent key, and the arguments. A hash that omits any of these leaves an approval standing after a re-enrolment or a replaced entry file.

The value that carries an authorization must have two cases, a human approval and a policy permit, without opening a construction path that a transport could use to approve a run. The compiler cannot enforce that on its own: a constructor called by the wrong layer, or a second implementation of a domain line, is invisible to type checking.

## Decision

- `PlanHash::compute` hashes exactly five inputs under the domain line `plan:v2`: the capability contract id and version, an `ImplHash` over the manifest hash and the entry script's SHA-256, a canonical target (host, enrolled key fingerprint, check and optional target), the canonical arguments and the execution id. The encoding is length-prefixed.
- Each other kind of plan has a domain line of its own so one approval can never stand in for another: `plan:package-v1` and `plan:package-job-v1` ([0029](0029-package-relay-boundary.md)), `plan:capability-v1` ([0037](0037-permit-issuer-and-unit-restart.md)) and `plan:containment-v1` ([0038](0038-containment-boundary.md)). Each literal is written in one file.
- `PlanHash::agent_check` (`plan:v1`: skill name, manifest hash, host and canonical arguments) is the hash the hub sends in `run_skill` and the agent recomputes from its own manifest. It is a local integrity check and never an approval binding.
- `KeyFingerprint` is the SHA-256 of the host's enrolled ed25519 public key, computed by the hub from its own record. It is part of every target and is never accepted from an agent, an incident or a client.
- `Authorization` is a wrapper with private internals, not a public enum. Its two cases are reachable only through `Authorization::human` and `Authorization::policy`; it has no `Default`, no `Deserialize` and no `From` or `Into`. A public variant would be a construction path the call-site scan cannot see.
- `PolicyPermit` has private fields and two constructors: `PolicyPermit::issue`, called only by the hub's permit issuer, and `PolicyPermit::restore`, called only by the store. Both take `PolicyPermitParts`.
- Enforcement is the line-based source scan of `cargo xtask arch-check` (`INV-1`), over every authorization constructor (`HumanApproval::new`, `HumanApproval::restore`, `PolicyPermit::issue`, `PolicyPermit::restore`, `Authorization::human`, `Authorization::policy`), over each plan domain line and over a `pub enum Authorization` anywhere but its own module. It is textual: a macro, a re-export under another name or generated code is not caught, and no comment claims a compile-time seal.
- There is no `clippy.toml` `disallowed-methods` entry for these constructors. A legitimate call site would need an `#[allow]`, which `arch-check` rejects outside `domovoi-runner`.

## Consequences

- A re-enrolment, a replaced entry executable, a changed contract, target, argument or execution identity invalidates an approval at dispatch, as a rejection rather than an error.
- A host without a live enrolled agent cannot produce a `plan:v2` hash at proposal time.
- The scan's ownership table names the modules that may build each value, so adding a second constructor call is a visible edit to `rules.rs`, which needs an ADR.

## Alternatives considered

- A public `Authorization` enum: simpler to match, but a transport could construct a variant directly and the scan could not see it.
- Serialising the authorization case as an enum on the wire or in the database: the wire needs only a discriminator.
- A clippy `disallowed-methods` entry: conflicts with the rule that forbids `#[allow]` outside the runner.
