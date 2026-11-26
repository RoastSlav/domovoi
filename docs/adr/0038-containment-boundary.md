# 0038. Containment boundary

- Status: Accepted
- Date: 2026-11-26

## Context

`firewall.block_source` drops inbound traffic from one external address or small prefix for a limited time, in a dedicated nftables table, through the root helper. A block that cuts the host's own management paths is worse than no block, so the helper enforces a kernel timeout, refuses overlap with its own exclusion set and applies a block in two steps with a confirmation that only the hub can send.

## Decision

1. **A closed call.** `ContainCall` is `Apply`, `Confirm`, `Rollback` (each with a `BlockSpec`) or `List`. It carries no key and no script; the helper renders every `nft` script from fixed templates. A phase always carries `expected_safety`; only a list is built without it.

2. **One capability, three executions, one approval.** Each phase is its own `run_capability` execution with its own execution id, all carrying the same plan hash and the same authorization, and all naming the block by the apply's execution id. The plan hash family `plan:containment-v1` excludes the phase and the execution id, so one approval covers exactly one block. The safety identity, or the reason it is unverified, is part of the hash: the approval binds what it showed. The literal is written only in `crates/domovoi-core/src/action/containment_plan.rs`.

3. **The hub confirms.** The agent dials out and a rule can sever its own control link, so a confirmation the agent sent itself would prove nothing. The hub's `confirm` reaching the agent proves the path both ways, and it must be sent within 20 seconds of the apply's result. If it is not, the hub rolls back while the session lives and otherwise lets the 30 second kernel timeout remove the block. An expired block is never re-applied.

4. **The helper decides.** The helper recomputes its exclusion set in both overlap directions and both families, enforces its maxima and the mutation limit, and compares `expected_safety`, for every caller. `rollback` is the one deliberate exception: it skips the identity comparison, the overlap check, the limits and the limiter, because it only removes a block.

5. **Verified and unverified paths.** A policy permit needs a verified, fresh, complete attestation and passed drills; the hub never issues a permit with an unverified assurance and the agent refuses one. A person may approve on the unverified path with their password, the source typed out and an explicit acknowledgement; the block records the reason and the paths that are not protected, and an audit row says so. A block step carries the destructive risk, so a person's approval needs the typed confirmation. `ExecutionRecord::authorize_by_permit` accepts a destructive step for this one contract only, and only when the permit names the verified safety identity it was issued against; every other destructive contract stays refused. The issuer re-reads the helper's identity and the plan, requires passed drills, and evaluates the rules with the containment input built from the verified report and the requested TTL and prefix lengths, so a ceiling's `max_ttl` and `prefix_len_*` floors apply. At dispatch the hub compares the permit's identity with the helper's current one and sends nothing on a difference (`safety_changed`).

6. **The leak self-test is a separate subcommand.** The long-lived helper never creates a namespace. A one-shot unit ordered before the helper socket runs the self-test when the kernel, `nft` or the layout changes. An inconclusive or missing result blocks containment writes.

7. **Boundary rules.** `ContainRelay` has one caller, `domovoi_engine::containment::phases`; the raw link names `start_contain` and `contain_result` appear once each in `runtime/caps.rs`. `AuthorizedContainment::of` and `execute_contain::execute_containment_step` run once, in the operation service's dispatch. A step is authorized only through `OperationCommands`. Containment logic stays out of the hub's `capability/` directory.

## What the checks cannot see

A passing leak self-test is weak evidence: it does not cover a real NIC, multi-queue receive, a conntrack rule, lower rates or the live set's contents. A flood from the blocked source can delay the confirmation past 30 seconds; the block then expires, which fails safe for management and is ineffective for containment. Containment writes follow the platform tiers of [0031](0031-helper-and-skill-runner-delivery.md): an untested platform needs the owner's per-host opt-in. The attestation that gates verified blocks is [0032](0032-helper-safety-attestation.md).

## Consequences

Containment never survives a reboot and is never re-applied after the helper's table is lost; the hub re-requests through a normal request. The driver keeps the last refresh of a host in memory only, so a policy run is refused until the owner refreshes after a restart.
