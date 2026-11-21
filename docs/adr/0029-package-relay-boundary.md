# 0029 Package relay boundary

- Status: accepted
- Date: 2026-11-21
- Extends [0023](0023-capability-objects-one-checker-bounded-evidence.md) and [0028](0028-one-execution-path.md) with a second, separate execution boundary: the one that puts signed package files on a host and runs the package capabilities there.

## Context

Owners will install signed agent packages from the hub. That needs three things the link did not have: sending file bytes to one host (the `transfer_*` frames of wire 1.2), starting `agent.package.install` and `agent.package.uninstall` (`run_capability`), and reading the answer. Each of them can put code on a machine, so each needs the same treatment as a skill dispatch: one place in the code that does it, reachable only with a human approval, and counted by the checker.

The agent is the only authority over what it installs: it verifies the signature against its own trusted keys and refuses everything else. The hub never opens an archive and never decides which keys a host trusts.

## Decision

- **A closed `PackageCall`.** The only requests the hub can make are `Install { package, version, artifact_transfer, signature_transfer }`, `Uninstall { package, version }` and `List`. The type has no key field and no free argument map: the capability arguments are built from it using the `ARG_*` constants of `domovoi_core::package::keys` and nothing else, the contract is chosen by the variant, and the target is `package:<name>` or `packages:all`. The link always asks as a human (`authorization_kind: human`); there is no policy form of a package request.
- **A capability object, not the link.** `PackageRelay` (in `domovoi-core/src/ports/caps.rs`) has three methods: `relay_transfer`, `dispatch_package` and `package_result`. `PackageCaps` bundles it with the availability probe, the session observer and the journal status object, as `ActionCaps` does. The hub's `runtime/caps.rs` builds it from the one link. The raw link operations behind the three methods are `AgentLink::send_transfer`, `AgentLink::start_capability` and `AgentLink::capability_result`. They are raw operations of EXEC-2: each has one call site in `crates/domovoi-hub/src/runtime/caps.rs`. The three relay methods are not raw operations.
- **Frames wait for room instead of failing.** A 20 MiB archive is about 320 chunk frames, more than the session outbox holds at once. `Registry::dispatch_reserved` checks the expected session under the registry lock, clones the outbox sender, releases the lock, waits for a free slot raced against the deadline on the `Clock`, then re-checks the session and queues the frame in one step. It never uses `try_send` and never holds a lock across an await. The variant used for `End` registers the pending reply in the same step, so the reply cannot arrive before anyone is waiting for it. An `End` that gets no answer is `Indeterminate`, because the agent may have stored the bytes.
- **Jobs are not operation steps.** An operation is an immutable plan of steps over skills ([0025](0025-operation-aggregate.md)). An install job has per-host targets, a transfer stage before the dispatch, retries per host and its own recovery states. Fitting it into steps would give every step type a second meaning. The install service is a separate aggregate with its own writer, and its targets are settled by the same idea as an operation's (ask the agent's journal) without sharing the operation service's code.
- **Packages are not skills under 0028.** A package is a bundle that adds skills to a host; installing one never enables anything. The single execution path of 0028 is the path by which a skill runs. Installation is a different action on the host, with a different authority (the agent's trusted keys), so it has its own boundary and its own cards, and the cards of 0028 are unchanged. The existing `run_capability` card, which says no code builds the wire message by that name, is unchanged too.
- **A plan hash of its own.** Package plans hash under `plan:package-v1`, never `plan:v2`, so a package approval cannot be replayed as a skill approval or the other way round. The per-host hash covers the contract, the package name, version, digest and signing key fingerprint, and the host. The job hash is `plan:package-job-v1` over the kind and the sorted host hashes. The execution id is not an input, so a retry can compare the host's plan hash and still get a new execution.
- **The hub's key list is a convenience, not an authority.** `[[packages.owner_keys]]` lets the hub refuse a package it knows no host would accept, and name the signer. Nothing on the hub can add, replace or remove a key an agent trusts. The hub cannot write a request that carries a key, because `PackageCall` has no such field.

## What the checker enforces

`cargo xtask arch-check` counts `send_transfer`, `start_capability` and `capability_result` as raw link operations, each exactly once, in `runtime/caps.rs`. The cards for the engine side follow the 0028 pattern and are added with the engine's install service: `AuthorizedPackageTarget::of` once and `execute_package` once, both in `crates/domovoi-engine/src/packages/worker.rs`. A gate test asserts that `execute.rs` is the only engine file that names `PackageRelay`.

## What the checker cannot see

- Whether the value passed as `HumanApproval` really came from an authenticated person. `HumanApproval::new` is confined to the hub's `auth/` directory by an existing rule, and the install endpoints take no client-supplied approval, but that is two rules and a test, not one proof.
- Whether the `expected_session` a caller passes is the session it observed. The link checks it against the live session under the registry lock; a caller that passes a stale value gets `SessionChanged`, and a caller that passes a wrong but current value is not noticed.
- Whether a transfer's bytes are the bytes the plan hash named. The engine hashes while it sends and compares before `End`; the agent verifies again. Neither is visible to a scanner.
- Whether the capability's outputs say what was installed. The engine compares them with the job's package, version, digest and key, and settles a difference as an unknown outcome.

## Consequences

- One place in the hub decides to relay package bytes and one decides to start a package capability, each with a value that exists for one target.
- The link and the testkit fake gain three operations. The trait's default bodies answer "not supported", so a link that does not implement them cannot be misread as having done something.
- No wire change and no agent change. The checker gains three raw operations and no new rule id.

## Alternatives considered

- **Reuse `start_skill` and skills for packages.** Rejected: a skill runs under a manifest and the agent's runner; an install changes what the agent will run.
- **Retry when the queue is full.** Rejected: a loop that polls a full queue spends the deadline sleeping and hides ordering. Waiting for a free slot is the queue's own mechanism.
- **Let the hub carry the key to trust in the request.** Rejected: it would make the hub an authority over the agent's trust.
