# 46 — Containment: `firewall.block_source`, the helper's nft write group, verified and unverified paths, drills

| | |
|---|---|
| Branch | `containment` |
| Milestone | M9 |
| Depends on (must be merged) | 45 (`policy-permit-autonomy`: `PermitIssuer`, the host ceiling enforced by the agent for policy runs, the persisted mutation limiter (the helper-side `restarts` file and `[limits] mutations_per_hour`), `POLICY_DISPATCHABLE`, `unit.restart` and the unit-specific dispatch pattern `UnitRelay` / `execute_unit_restart` / agent `requests/unit.rs` that this prompt follows with its own siblings; the names are those of the 45 brief, and merged code wins), 47 (`three-identities-helper-and-skill-runner`: 47a helper skeleton and `helper.toml`, 47c attestation, hub verifier, high-water mark, `ManagementCoverage` implementation, 47d typed read variants and `HelperRequest::group()`; the names are those of the 47a, 47c and 47d briefs, and merged code wins), 34 (`ManagementCoverage` hook, merged), 25 and 24 (registry and contracts, merged), 42 (package relay pattern and ADR 0029, merged), 26, 27, 29 (operations and the use-case layer, merged). |
| Can run in parallel with | nothing (it is the last prompt of the helper and autonomy line; one branch at a time). |
| Produces (shared contracts) | `domovoi_core::containment::*` (pure: canonical network target, exclusion algorithm, nft script templates, error-token classifier, journal record types, gate and drill state types, output keys); contract `firewall.block_source` (v1, `Contain`, helper path) and `firewall.list_blocks` (v1, `Read`, helper path); `TargetKind::NetworkSource`, `TargetQuery::NetworkSource` and the `ipnet:` identity builder; `domovoi_core::action::containment_plan::ContainmentPlan` (plan hash family `plan:containment-v1`); the helper's `nft` operation group (`firewall.apply`, `firewall.confirm`, `firewall.rollback`, `firewall.list_blocks`) and the subcommands `domovoi helper nft-selftest`, `domovoi helper containment-drill`; the agent's containment backend; `domovoi_engine::containment::{ContainmentDriver, ...}`; hub table `containment_blocks`; the `/api/v1/hosts/{name}/containment*` endpoints; the one-shot self-test unit and the namespace CI job; ADR for the containment boundary |
| Consumes | 45's permit issuer, agent ceiling check, persisted limiter and `POLICY_DISPATCHABLE` (this prompt appends `firewall.block_source` to it and extends 45's gate `policy_dispatchable_list_is_closed`); 47a's helper request and response types, state directory, lock and start checks; 47c's `SafetyIdentity`, `ExpectedSafety` constructors, hub gate (`latest outcome`, high-water mark) and `ManagementCoverage` implementation; 34's `ManagementCoverage` and `CoverageVerdict`; 25's `CapabilityRegistry`/`TargetResolver`; 39's `ContainmentInput` and hard limits; 42's relay and boundary pattern; the wire 1.2 `run_capability`, `refused`, `capability_result`, `action_status` (frozen, **not changed here**) |
| Launch gate | 45 and 47 merged; **R-2 and R-10 accepted by the owner** (recorded in `99` and the ADRs; a research prompt never accepts them); the leak self-test (section 6.8) returns `open` for hosts on at least one platform where the owner has run the installer. This is **not** a support claim: D-39 and D-57 leave the Supported tier empty, so "passing on a supported platform" cannot hold literally. The first platform that passes is Untested for containment writes and the owner opts in per host. |
| Suggested model | the strongest available (security-sensitive, root helper, kernel behaviour; every decision is made below, follow the contracts exactly, and stop on any difference that changes a decision) |


## Hub use-case layer (read first)

Prompt 29 changed how hub code is organised; 42 repeated the rules and they still hold.

- Every command is a method on a service in `crate::services` taking `&Caller`. HTTP handlers only parse, call and map. Plain reads may stay in `api/`.
- Do not add an engine handle (`ActionHandle`, `OperationHandle` or `ContainmentHandle`) to `ApiDeps` or `ChatDeps`; give it to `Services`. Arch rules `MOD-1` to `MOD-4` reject the other form.
- A new top-level hub module must be listed in `ZONES` or `UNZONED_MODULES` in `xtask/src/arch/rules.rs`.
- `crates/domovoi-hub/tests/gates.rs:125-145` forbids the strings `HumanApproval`, `PolicyPermit`, `Authorization`, `expected_safety`, `EffectClass::Contain` and `Contain {` in non-test code under `crates/domovoi-hub/src/capability/`. **Do not put containment logic there**: the contract lives in `domovoi-core`, the driver in `domovoi-engine`.

If `crates/domovoi-hub/src/services/` does not exist, or 45 or 47 is not merged, stop and report.

## 1. Objective

Make `firewall.block_source` a working, bounded, journaled containment capability: a timed inbound block of one external address or small prefix, enforced by the root helper in nftables with a kernel timeout, applied in two steps with a hub confirmation, protected from cutting the host's own management paths, and verified by drills. Concretely:

1. **The contract** `firewall.block_source` (class `Contain`, helper path) with phases `apply`, `confirm`, `rollback`, and the read contract `firewall.list_blocks`, in `domovoi-core`, with a network target kind and a canonical `ipnet:` identity.
2. **The helper's `nft` write group**: an `nft` CLI executor built only from fixed script templates (argv and stdin only, no shell), the table layout decided in `RW-13` (one `prerouting` chain at priority -310, plain hash sets for single hosts, interval sets for prefixes only where the leak self-test passed), a create-if-absent bootstrap, explicit non-zero timeouts, the two-step apply with a 30 s kernel timeout, an idempotent rollback batch, the expired-versus-rolled-back classification by table check, JSON membership read and a write-ahead journal, exclusions computed in both overlap directions and both families, conntrack deletion only when flowtables exist, table-loss detection, `max_elements` and `max_active` accounting, the bridge-family gap report, the verification probe and the error-token table.
3. **The leak self-test** as a separate helper subcommand, run by the installer and by a one-shot unit ordered before the helper socket when the kernel, `nft` or the layout changes. The long-lived helper never creates a namespace.
4. **The agent backend** that turns `run_capability` for the contract into helper requests, and the **hub driver** that runs the two steps, handles the verified and unverified authorization paths, and recovers from crashes.
5. **Drills**: a named drill suite (namespace tests in CI and an on-host `containment-drill` subcommand) that must pass before unattended containment is dispatched on a host.
6. **Hub and UI surfaces** this prompt owns and nothing else: a containment read endpoint, the unverified-human approval variant, a phase timeline, and the attention item for `containment_lost`.

It does **not** change the wire, build the helper skeleton, build attestation, issue permits, or implement any other helper group.

## 2. Architectural context

**Authority (RW-13, RW-37, RW-39, D-17, D-22, D-28).** The helper is the only authority over what is blocked: it recomputes its exclusion set, refuses overlap, enforces maxima and the mutation limit, and compares `expected_safety`, for every caller. The agent and the hub are untrusted by the helper beyond that. A compromised agent can call the helper directly within the helper's limits (residual risk accepted in `04` §8.4).

**One capability, three phases, one approval.** The two-step apply of `04` §9.1 needs a hub confirmation after a round trip on the live session: that round trip is the watchdog (`If the hub never confirms, the block disappears by itself`). Wire 1.2 gets no new frame and the agent journal deduplicates by `execution_id` (`crates/domovoi-agent/src/requests/capability.rs:255-278` returns the earlier result for a repeated id). So each phase is its **own `run_capability` execution with its own execution id and its own journal entry**, all carrying the **same plan hash** and **the same authorization**, and all naming the block by the execution id of the apply (`block`). The plan hash is a new family (`plan:containment-v1`, section 6.2) that excludes the phase and the execution id, so one approval covers exactly one block: apply, its confirmation, and its rollback. The agent and the helper never confirm on their own: only the hub, after a successful round trip, sends `confirm`.

**Why the hub confirms and the agent does not.** The agent dials out and a rule can sever its own control link. A confirmation that the agent could send itself would prove nothing about the hub being reachable. The hub's `confirm` request reaching the agent proves the path both ways.

**Safety, in two layers.** The hub gates on the verified attestation (47c): `ExpectedSafety::Verified{..}` is built from the stored verified identity at dispatch. The helper compares it with its freshly computed identity and refuses `safety_changed`. The unverified human path (RW-37) sends `ExpectedSafety::Unverified{reason}` and skips only that comparison; everything else still runs. The hub never issues a `PolicyPermit` with `unverified`; the agent refuses `unverified` with a policy authorization.

**What this prompt owns, and what it does not.** 47 owns the helper skeleton, `helper.toml` parsing, key custody, the signed report, the pin, the verifier and `ManagementCoverage`. 45 owns the permit, the ceiling, the persisted mutation limiter, `POLICY_DISPATCHABLE` and the unit-specific dispatch pattern (`UnitRelay`, `execute_unit_restart`, agent `requests/unit.rs`); there is **no generic helper-capability dispatch**, so 46 builds `ContainRelay`, `execute_containment` and an agent `requests/containment.rs` arm as siblings of 45's, copying its check order. 46 owns everything specific to the nft write group and the containment capability, and the human approval variant for unverified containment.

## 3. Repository context

Verified in this checkout (branch `research-synthesis`, which carries the merged 42). Re-open each item before editing and report differences. Migrations end at `0019_install_jobs.sql`; after 47c (expected `0020`) and 45 (expected `0021`) the next free number is expected to be `0022`; list the directory and take the next free number, never assume it.

**Contracts and targets.**

- `crates/domovoi-core/src/capability/effect.rs:13-22` `EffectClass { Read, Mutate, Contain, Destructive }`; `:74-83` `TargetKind { None, Host, Container, Unit }` has **no network variant**; `:107-119` `ExecutionPath { Hub, Agent, SkillRunner, Helper }` already has `Helper`.
- `crates/domovoi-core/src/capability/contract.rs:185-306` `CapabilityContract::validate`: `:200-205` requires `target_arg.is_some()` exactly when the kind is `Container` or `Unit`; `:227-233` a mutating contract needs reversibility and verification; `:55-56` `constrainable_args` keys are keys of the input schema.
- `crates/domovoi-core/src/capability/ids.rs:237` `TargetIdentity` already accepts the `ipnet` scheme (rest 1 to 300 printable ASCII); `crates/domovoi-core/src/capability/target.rs:80-` `TargetQuery { Host, Container, Unit }`. `docs/rework/prompts/25-capability-registry.md` section 6.3.4 says explicitly that the network target kind, the CIDR parser and the resolver branch **arrive with this prompt**.
- `crates/domovoi-core/src/capability/package.rs:75-80` `known_contracts()` chains the built-ins and the package contracts; `ContractId::from_static` is `pub(crate)` (`ids.rs:111`), so the contract is defined in core beside `package.rs`.
- `crates/domovoi-hub/src/capability/registry.rs:265-269` a `ExecutionPath::Helper` contract is only `Unavailable{HelperMissing}` when the helper is `Absent`; it does not look at the helper's offered `ops` (`crates/domovoi-proto/src/messages/handshake.rs:248-264` `HelperStatus::{Absent, Ok{ops}, Error{reason}}`; `crates/domovoi-core/src/ports/capability.rs:72-80` `HelperState::{Absent, Ready, Failed}` drops the `ops`). Containment must additionally require the nft operations in `ops` (section 6.10).
- `crates/domovoi-hub/tests/hub/capability_ceiling.rs:9` is a **test fixture** that already writes a ceiling entry `firewall.block_source` with `args {"min_prefix_len_v4": 24}`. No contract by that id exists in code. See "Differences" 4.

**Wire.** `crates/domovoi-proto/src/messages/actions.rs:17-37` `RunCapability` (`args: BTreeMap<String,String>`, `target: TargetKey`, `plan_hash`, `authorization_kind`, `expires_at`, `expected_safety: Option<ExpectedSafety>` with `#[serde(default)]`, comment says "Required for `contain`" but nothing enforces it); `:51-76` `ExpectedSafety { Verified{fingerprint: String, boot_id: String, generation, config_hash: Sha256Bytes, route_state_hash: Sha256Bytes}, Unverified{reason: String} }` with no `deny_unknown_fields`; `:105-119` `CapabilityFailureCode`; `:141-162` `RefusalCode` includes `PolicyRequiresVerifiedSafety`; `Refused.code` is `Open<RefusalCode>`, so an unknown token can still travel. `PROTOCOL_VERSION` is 1.2 (`crates/domovoi-proto/src/version.rs:17`). 47c makes `ExpectedSafety` strict (D-47): it moves to `domovoi_core::attest::expected`, is re-exported from `domovoi_proto::messages`, refuses extra fields and unknown tags, and its reason is the closed enum `UnverifiedReason`; this prompt uses that form and never edits `crates/domovoi-proto/`.
- `crates/domovoi-hub/src/agent_server/link_package.rs:250` builds `RunCapability` with `expected_safety: None` for packages; the package call is the closed `PackageCall` enum (42). A containment call is its own closed type (section 6.5), never an extension of `PackageCall`.

**Agent.** `crates/domovoi-agent/src/requests/capability.rs:166-299` `run_capability` serves only `agent.package.install|uninstall|list`; `:179-190` every other contract is `refused{contract_unknown}`; the order of checks (journal fence, contract, version, owner switch, human-only, expiry, argument parse before the journal, boot snapshot, `journal.accept`, `started`, work, `finished`) is the pattern to copy. `crates/domovoi-agent/src/requests/catalog.rs:41-` builds the advertised entries. There is no helper client in the agent crate (`grep -il helper crates/domovoi-agent/src` finds only journal/link/packages mentions); 47a adds it.

**Processes.** `crates/domovoi-core/src/ports/process.rs:8-27` `ProcessSpec { program (absolute), args, env (complete), cwd, stdin: Option<String>, timeout, max_output_bytes, run_as }`; `:74-77` `ProcessExecutor::run`; only `domovoi-runner` implements it. The testkit fake is `crates/domovoi-testkit/src/process.rs:19-` `FakeProcessExecutor` (`expect`, `expect_result`, `calls`).

**Policy and coverage.** `crates/domovoi-engine/src/policy/request.rs:129-` `ContainmentInput { attested, uncovered_paths, stale_confirmation, target_overlaps_verified_set, helper_incomplete }`; `crates/domovoi-engine/src/policy/limits.rs:60-110` applies the ten hard limits in order; `:142-186` `ceiling_covers` reads `request.requested_ttl` against `entry.max_ttl` and, for each `contract.constrainable_args` whose key the entry bounds, parses `request.args.get(&arg.key)` as `i64` (a missing arg is **not covered**). `crates/domovoi-core/src/ports/knowledge.rs:114-122` `ManagementCoverage { is_available(), verdict(host, cidr) }`; `crates/domovoi-engine/src/knowledge/coverage.rs` `UnavailableCoverage` is the only production implementation until 47. `crates/domovoi-core/src/action/authorization.rs:102-184` `PolicyPermit` already has `safety_identity: Option<String>` (opaque).

**Boundary rules.** `xtask/src/arch/rules.rs:203-215` `RAW_OPS` (already holds `send_transfer`, `start_capability`, `capability_result` from 42); `:341-343` the EXEC-2 card says `run_capability` has no caller. `xtask/src/arch/scan.rs:233-238` allows the `plan:v2` line only under `crates/domovoi-core/src/action/`. `crates/domovoi-engine/src/operations/execute.rs:38-135` `AuthorizedStep` and `execute_step` are **skill-only** (they take `SkillSummary`, `ImplHash`, the v1 wire hash).

**Not in the repository (verified).** No helper crate, no helper client, no nft code, no `containment` module, no `firewall.*` contract, no typed signal carrying an attacker's source address (`crates/domovoi-checks/src/security/ssh.rs:26-120` puts the IP of an **accepted** login in evidence text only), no drill or dry-run for containment (`crates/domovoi-agent/tests/agent/dry_run.rs` is `check run`).

**Differences between the planning text and the repository (resolved here; report if the code disagrees).**

1. **Plan hash.** `PlanHash::compute_v2` (`crates/domovoi-core/src/action/plan.rs:164-183`) needs a `PlanTarget` with a `CheckKey` and an `ImplHash` from a manifest. A block has neither. Resolution: a separate family `ContainmentPlan` (6.2), the way 42 did `PackagePlan`.
2. **Ceiling argument names.** `04` §8.3 writes `args = { min_prefix_len_v4 = 24, min_prefix_len_v6 = 64 }`. `ceiling_covers` requires the bound's key to be the **same string** as a constrainable argument key present in the request args, and a missing arg fails. So the contract declares `prefix_len_v4` and `prefix_len_v6` (`IntAtLeast`), the request always carries both, and the ceiling keys become `prefix_len_v4` and `prefix_len_v6`. The `04` §8.3 example and the fixture at `capability_ceiling.rs:9` use the old name (the fixture is only JSON and keeps passing); list the doc edit under 15.
3. **Refusal spellings.** RW-37 says the agent refuses an unverified policy run with `unverified_requires_human`; the wire already has `RefusalCode::PolicyRequiresVerifiedSafety` (`actions.rs:154`). Use the existing variant. `malformed_safety` has no `RefusalCode`: it travels as an `Open` unknown code (`Open::new("malformed_safety", None)`), or as an undecodable message once 47c makes `ExpectedSafety` strict.
4. **Token names, one per meaning.** `exclusion_overlap` (03 §5.8, ADR 0032) wins over `overlaps_exclusion` (04 §5.6 of the synthesis); `invalid_target` wins over `bad_target` and `bad_target` is not used; `ttl_above_maximum` wins over the test name `ttl_above_max_ttl_is_refused` (the test is renamed). `absent_unexplained` and `table_missing` are internal classifications; the token that travels for a lost block is `containment_lost`.
5. **Defaults.** `max_active` defaults to **0** (D-50, `04` §8.3 fresh install, `04b` §2), not the 50 in synthesis 04 R-5.11.2; the 50 in `04` §8.3 is an *example agent ceiling*.
6. **"Drill mode."** No document describes a dry-run or drill mode. The "drills" of the index are the six drills of `04` §9.1 plus the four named tests of `06` M9 (`block_expires_with_agent_stopped`, `unconfirmed_block_expires_in_30s`, `excluded_address_refused`, `model_cannot_trigger_containment`); this prompt specifies them as a named suite (section 6.9).
7. **Self-test duration.** Synthesis 04 5.14 says "about 30 s". That figure came from T's in-process libnftables replacer at about 5 000 replacements per second. The helper's call path is one `nft -f -` process per batch, far slower, so the minimum replacement count is reached later or not at all. "Minimum not reached" is inconclusive and means `blocked_untested` (fail closed).
8. **Who creates a block.** No detector emits a typed source address. The entry point in v1 is an owner-pinned `ipnet:` subject on an operation (and the owner's explicit step approval); the SSH brute-force detector is a non-goal.

## 4. Exact scope

The helper lives in `crates/domovoi-helper` (47a, `docs/rework/prompts/47a-helper-accounts-and-skeleton.md` section 6.7). Below, `<helper>` means that crate root; `SafetyIdentity` is defined in 47c (`domovoi_core::attest`, `47c-helper-attestation.md`). Non-test files at most 500 lines; split rather than exceed.

Files to **add**:

| File | Purpose |
|---|---|
| `crates/domovoi-core/src/containment/{mod,net,exclusion,templates,tokens,journal,gate,keys}.rs` | pure types and functions (6.1, 6.3, 6.4, 6.6) |
| `crates/domovoi-core/src/capability/containment.rs` | the two contracts (6.1) |
| `crates/domovoi-core/src/action/containment_plan.rs` | `ContainmentPlan` (6.2) |
| `<helper>/src/nft/{mod,exec,ensure,apply,confirm,rollback,classify,journal_file,flows,verify,limits,list}.rs` | the nft write group (6.3, 6.4) |
| `<helper>/src/nft/selftest/{mod,scenario,sender,verdict}.rs` and `<helper>/src/nft/drill.rs` | leak self-test and drill subcommands (6.8, 6.9) |
| `crates/domovoi-agent/src/containment/{mod,args,backend}.rs` | the agent backend (6.7) |
| `crates/domovoi-engine/src/containment/{mod,driver,phases,input,reconcile,audit}.rs` | hub driver (6.5, 6.10) |
| `crates/domovoi-store/migrations/00NN_containment_blocks.sql`, `crates/domovoi-store/src/repos/containment.rs`, `crates/domovoi-store/src/containment/{mod,apply}.rs`, `crates/domovoi-core/src/ports/repos/containment.rs` | `containment_blocks` (section 8) |
| `crates/domovoi-hub/src/api/{containment,dto_containment}.rs`, `crates/domovoi-hub/src/services/containment.rs` | endpoints (6.11) |
| `web/src/features/containment/*` | approval variant, timeline, attention item (6.12) |
| `packaging/systemd/domovoi-helper-selftest.service` (path per 47a's installer layout) | the one-shot unit (6.8) |
| `docs/adr/0038-containment-boundary.md` (expected number; next free at implementation time) | required because `rules.rs` changes |
| tests: section 12 |

Files to **change** (minimal edits): `crates/domovoi-core/src/{lib.rs,capability/{effect,contract,mod,package,target,ids}.rs,action/mod.rs}` and every exhaustive `match` on `TargetKind`/`TargetQuery` the compiler lists (found: `capability/{builtin,skill,tests*}.rs`, `crates/domovoi-engine/src/{plan/resolve.rs,policy_shadow/inputs.rs,targets/mod.rs}`, `crates/domovoi-hub/src/capability/registry.rs`, `crates/domovoi-testkit/src/target.rs`); `crates/domovoi-agent/src/requests/{capability,catalog,mod}.rs` and the agent's ceiling check (45's code); 45's hub dispatch (extend, do not fork); `crates/domovoi-engine/src/policy` input builder only through a new function in `engine/src/containment/input.rs`; `crates/domovoi-hub/src/{agent_server/link_package.rs or a sibling,runtime/caps.rs,runtime/mod.rs}`; `crates/domovoi-store/src/{lib,uow/writes}.rs`, `ports/uow/*` (new `AggregateKind`, `JournalKind`, `StateWrite` variants beside the others), the in-memory twin and crash summary; `crates/domovoi-config` (helper `[nft]` section, 47a's `helper.toml` schema) ; `xtask/src/arch/rules.rs`; `crates/domovoi-testkit/src/{process,link/*,repos/*,lib}.rs`; `.github/workflows/ci.yml`; `TESTING.md`; docs (section 15).

## 5. Explicit non-goals

- **No wire change.** Nothing under `crates/domovoi-proto/`; no new frame, no field. Tokens travel as `"<token>: <detail>"` inside `Failed.message` and `Refused.detail`; no enum grows.
- **The helper skeleton, accounts, sockets, start checks, `helper.toml` parsing, key custody and the signed report are 47.** This prompt adds one operation group and two subcommands to what 47a built.
- **Attestation, pinning, the verifier, the high-water mark and the latest-outcome gate are 47c.** This prompt reads the gate.
- **Permit issuance, the unattended ceiling logic, the mutation limiter and `unit.restart` are 45.** This prompt reuses the limiter.
- **No Docker helper group, no `container.pause`, no `unit.stop_temporary`.**
- **No bridge-family table** (synthesis 04 D8: a coverage gap is reported). **No netlink binding** (`nft` CLI only). **No reboot persistence** (containment does not survive a reboot; permits are invalid by `boot_id`). **No automatic re-apply after table loss** (the hub re-requests through a normal request). **No removal of orphan elements** (report only). **No `ct` rule anywhere in the chain.** **No `destroy`, no `flush ruleset`, no foreign table.**
- **No detector.** No change to `crates/domovoi-checks`; no typed source-address signal. A detector that names an attacker address is a follow-up.
- **No model involvement.** The LLM tool catalogue stays closed (`MetaTool`, `ReadTool`); there is no tool, chat command or button that creates a block outside the operation step approval or a permit. `model_cannot_trigger_containment` is a named test.
- **No hub-initiated background polling** of `firewall.list_blocks` (a read through `run_capability` needs a human `authorization_kind`; it is owner-initiated only).
- **No support statement** in docs or UI (D-57). "Validated on" notes only.
- Do not weaken any gate test or arch rule; do not touch merged migrations.

## 6. Required interfaces and contracts

Marked **fixed** (must match) or **convention** (follow repository style). Names taken from 45 and 47 are as defined in those briefs (`README.md`, "Cross-brief names"); merged code wins and each difference is reported.

### 6.1 Contracts, target kind and arguments (`domovoi-core`, **fixed**)

`TargetKind::NetworkSource` (serde `network_source`) is added. `CapabilityContract::validate` (`contract.rs:200`) treats it like `Container` and `Unit`: `target_arg` must be `Some`. `TargetQuery::NetworkSource { host: HostId, source: Net }` and `TargetQuery::kind/host` follow. Resolution is pure (no read): `TargetService` (`engine/src/targets/mod.rs`) takes the enrolled key as for a host and returns `ResolvedTarget { identity: canonical_network_identity(host, &key, &net), name, kind: NetworkSource, host_key }` where

```rust
/// `ipnet:{canonical}@host:{host_id}/{fingerprint}`; the only place the format is written.
pub fn canonical_network_identity(host: HostId, key: &KeyFingerprint, net: &Net) -> TargetIdentity;
```

The wire target (`RunCapability.target`, a `TargetKey`, `signal/keys.rs:86`) is `ipnet:{canonical}` (at most 128 characters, so `TargetKey::parse` accepts it).

Contract `firewall.block_source`, version 1 (built by a function `containment_contracts()` in `capability/containment.rs`, appended to `known_contracts()`):

| Field | Value |
|---|---|
| title, description | "Block source", "Drops inbound traffic from one external address or small prefix for a limited time, in a dedicated firewall table, through the root helper." |
| effect, category | `Contain`, `firewall.contain` |
| target | `NetworkSource`, `target_arg = Some("source")` |
| reversibility, idempotency | `Reversible`, `Idempotent` (a repeated `apply` of the same block is `already_present`; `confirm` and `rollback` are idempotent) |
| timeout | 60 s (the helper's own limit is `[limits] timeout`, default 30 s; the hub waits the contract timeout + 30 s as in 42) |
| output limit | 8192 |
| verification | `Postcondition`: "the element is in the block set with time left, the `contain` chain matches its template, and the hub can still reach the agent" |
| `constrainable_args` | `prefix_len_v4` (`IntAtLeast`, "shortest prefix allowed for an IPv4 source"), `prefix_len_v6` (`IntAtLeast`, "shortest prefix allowed for an IPv6 source") |
| `safety_facts` | `[]` (management-path coverage is the containment input of the evaluator, not a fact) |
| `execution` | `Helper`, `source: BuiltIn` |

Input schema: an object, `additionalProperties: false`, all of these required strings: `phase` (`apply`, `confirm`, `rollback`), `source` (canonical network text, `net.rs`), `ttl_s` (decimal integer, the TTL the block has after `confirm`), `block` (UUID of the **apply execution**), `prefix_len_v4`, `prefix_len_v6` (decimal integers). The args map always holds exactly these six keys (constants in `containment::keys::ARG_*`). Rules, checked by the hub builder, the agent and the helper alike:

- `source` is the output of `Net::canonical()`; anything else is `invalid_target`.
- `prefix_len_v4` is the target's own prefix length when it is IPv4, else `32`; `prefix_len_v6` is the target's length when IPv6, else `128`. So every ceiling floor of the *other* family is satisfied and `ceiling_covers` (`limits.rs:174-183`) never fails on a missing key.
- `ttl_s` is identical in all three phases of one block; `block` equals the execution id for `apply` and the apply's execution id for the other two.
- the requested TTL for the ceiling check (`EvaluateRequest.requested_ttl`) is `ttl_s` seconds.

Contract `firewall.list_blocks`, version 1: effect `Read`, category `firewall.observe`, target `Host`, `target_arg None`, no args (schema as `package.rs:18-37` with no properties), timeout 30 s, output limit 262 144, `Helper` path. Authorization kind on the wire is `human` (an owner-initiated refresh, as 42's list; there is **no background caller**).

The pure module `containment::keys` holds every output key (all strings): `phase`, `block`, `source`, `set`, `applied_at`, `step1_ttl_s`, `remaining_s`, `table_state` (`ok|created|recreated`), `layout` (`hosts|hosts_and_prefixes`), `generation`, `existing_flows` (`cut|n/a|may_persist`), `bridge_gap` (`true|false`), `lost_blocks`, `chain_ok`, `arrivals`, `gate`, `drills`, `blocks` (JSON, list only), `active`, `orphans`.

### 6.2 Plan hash (`domovoi_core::action::containment_plan`, **fixed** encoding)

```rust
pub struct ContainmentPlan { /* contract id+version, source, ttl, host, host key, assurance */ }
impl ContainmentPlan {
    pub fn new(net: &Net, ttl: SignedDuration, host: HostId, host_key: &KeyFingerprint,
               assurance: &PlanAssurance) -> Self;
    pub fn hash(&self) -> PlanHash;
}
/// What the approval binds about safety: the verified identity, or the unverified reason.
pub enum PlanAssurance { Verified(SafetyIdentity), Unverified(UnverifiableReason) }
```

```
"plan:containment-v1\ncontract:{len}:firewall.block_source@1\nsource:{len}:{canonical net}\nttl:{secs}\n
 target:{len}:{host_target_text(host,key)}\nassurance:{len}:{verified:<fingerprint>/<boot_id>/<generation>/<config_hash>/<route_state_hash> | unverified:<reason>}\n"
```

Length-prefixed like 22's v2; the phase and the execution id are **not** inputs, so `apply`, `confirm` and `rollback` of one block carry one plan hash. The line `plan:containment-v1` may be written only under `crates/domovoi-core/src/action/` (extend the scan at `xtask/src/arch/scan.rs:233` the way 42 did for `plan:package-v1`). The safety identity **is** in the hash (the approval binds what it showed). `SafetyIdentity` is 47c's (`domovoi_core::attest`); `UnverifiableReason` is 34's, extended by 47c with `PathUncovered` and `ConfirmationStale`.

### 6.3 Pure core: `domovoi_core::containment` (**fixed** semantics, **convention** shapes)

```rust
/// A canonical network: one family, `lo..=hi` as u128, the prefix length.
pub struct Net { /* family, lo, hi, prefix */ }
pub enum Family { V4, V6 }
impl Net {
    /// Strict: dotted IPv4 (no leading zeros) or IPv6 text, optional `/len`, no hostnames,
    /// host bits zero. Refuses with `NetError`: `Invalid`, `MappedForm` (`::ffff:0:0/96` and
    /// `::/96` ranges), `HostBits`, `PrefixLength`.
    pub fn parse(text: &str) -> Result<Self, NetError>;
    pub fn canonical(&self) -> String;          // dotted IPv4, RFC 5952 lowercase IPv6, `/len` always written
    pub fn family(&self) -> Family;
    pub fn prefix_len(&self) -> u8;
    pub fn is_single_host(&self) -> bool;       // /32 or /128
    pub fn overlaps(&self, other: &Net) -> bool;   // `lo <= other.hi && other.lo <= hi`; one test, both directions
}
/// The fixed ranges of synthesis 04 R-5.6: 127.0.0.0/8, ::1/128, 169.254.0.0/16, fe80::/10,
/// 0.0.0.0/8, 224.0.0.0/4, 255.255.255.255/32, ::/128, ff00::/8.
pub fn fixed_exclusions() -> Vec<Net>;
pub struct ExclusionInputs { pub management_cidrs: Vec<Net>, pub hub_addrs: Vec<Net>, pub link_peers: Vec<Net>,
    pub local_addrs: Vec<Net>, pub gateways: Vec<Net> }
/// `Err(ExclusionRefusal::Incomplete)` when an input could not be established (the caller
/// passes `None` for it); the helper then refuses all containment.
pub fn effective_exclusions(inputs: Option<&ExclusionInputs>) -> Result<Vec<Net>, ExclusionRefusal>;
pub fn check_target(net: &Net, exclusions: &[Net], policy: &TargetPolicy) -> Result<(), TargetRefusal>;
pub struct TargetPolicy { pub min_prefix_v4: u8, pub min_prefix_v6: u8 }   // from the helper config; never /0
/// Disjoint ranges in ascending order, merged, for rendering an interval set.
pub fn coalesce(nets: &[Net]) -> Vec<(u128, u128, Family)>;
```

`check_target` refuses, in this order: `prefix_too_wide` (below the policy minimum, never `/0`), `exclusion_overlap` (any overlap in either direction, so a `/32` inside a management `/24` and a `/16` containing it are both refused), for each family separately (an IPv4 target is never compared with an IPv6 exclusion; a mapped member of an exclusion input is canonicalised to IPv4 before comparing). The hub uses the same two functions to compute `ContainmentInput.target_overlaps_verified_set` against the **verified report's lines** (6.10), so hub and helper cannot drift.

### 6.4 The nft script templates (`containment::templates`, **fixed**; the only text ever sent to `nft`)

Rendering takes validated values only: the table name (`[a-z][a-z0-9_]{0,31}`, default `domovoi`), set names from a closed enum, `Net::canonical()` strings, integers rendered as `{n}s` with `n >= 1`, and `max_elements` (1 to 10 000). Nothing else is interpolated. The renderer returns a `String`; the executor sends it on stdin with `nft -f -`. Every template is an `insta` snapshot.

Bootstrap, sent **once**, only when the JSON read says the table is absent (hosts layout; `N` = `max_elements`):

```
create table inet domovoi
add set inet domovoi blocked4 { type ipv4_addr; flags timeout; timeout 30s; size N; }
add set inet domovoi blocked6 { type ipv6_addr; flags timeout; timeout 30s; size N; }
add set inet domovoi excl4 { type ipv4_addr; flags interval; }
add set inet domovoi excl6 { type ipv6_addr; flags interval; }
add chain inet domovoi contain { type filter hook prerouting priority -310; policy accept; }
add rule inet domovoi contain counter
add rule inet domovoi contain ip saddr @excl4 accept
add rule inet domovoi contain ip6 saddr @excl6 accept
add rule inet domovoi contain ip saddr @blocked4 counter drop
add rule inet domovoi contain ip6 saddr @blocked6 counter drop
```

Layout `hosts_and_prefixes` (only where the self-test passed the interval layout) inserts, after the two `excl` sets, `add set inet domovoi blockednet4 { type ipv4_addr; flags interval,timeout; timeout 30s; size N; }` and the `blockednet6` twin, and appends `add rule inet domovoi contain ip saddr @blockednet4 counter drop` and the `ip6` twin after the two `blocked` rules. **Never** an `add set`, `add chain` or `add rule` over a live table (an `add set` silently rewrites `size` and the default timeout; an `add rule` duplicates). **Never** a `ct` rule; **never** a `comment` before a verdict.

Step 1, `apply` (per family; `S` = `blocked4|blocked6` for `/32` and `/128`, `blockednet4|blockednet6` for prefixes; `X` = `Net::canonical()` with the `/len` written for prefixes and **omitted** for a single host):

```
create element inet domovoi S { X timeout 30s }
```

Step 2, `confirm` (`T` = `ttl_s`, `30 < T <= max_ttl`), one batch, nothing else in it:

```
delete element inet domovoi S { X }
add    element inet domovoi S { X timeout Ts }
```

`rollback` (also the owner's early release), primary, one batch, idempotent for present, absent and expired elements:

```
add    element inet domovoi S { X timeout 5s }
delete element inet domovoi S { X }
```

and, only after the membership read says X is present, the fallback `delete element inet domovoi S { X }` alone. Exclusion rewrite, one batch per family, members coalesced to disjoint ranges so an interval set never sees an overlap, the `add element` line omitted when empty:

```
flush set inet domovoi excl4
add element inet domovoi excl4 { 192.0.2.0-192.0.2.255, 198.51.100.7-198.51.100.7 }
flush set inet domovoi excl6
add element inet domovoi excl6 { 2001:db8::-2001:db8::ffff }
```

Reads (argv, no stdin): `nft -j list table inet domovoi`; `nft -j list set inet domovoi S`; `nft -j list flowtables`. Conntrack (only when flowtables exist, section 6.4.2): `conntrack -D -s X`. The formatter emits the TTL only as `<n>s`, `n >= 1`; `timeout 0s` and an element without a timeout are unrepresentable. Program paths are absolute (`ProcessSpec.program`, `process.rs:12`), resolved at helper start from `/usr/sbin`, `/usr/bin`, `/sbin`, `/bin`; the environment is exactly `{"LC_ALL": "C"}` (stable stderr); the timeout is `[nft] call_timeout` (1 to 30 s, default 5 s, never 0); output caps 8 MiB for list reads and 64 KiB otherwise.

#### 6.4.1 Ensure (create-if-absent, drift, layout, table loss)

`ensure_table()` runs before **every** request (not only at start): `nft -j list table inet domovoi`. Absent (userspace `Error: No such file or directory`, exit 1): send the bootstrap batch once; if it fails with `File exists` on `create table` another actor won the race: re-read and compare (`table_exists_race`). Present: compare, from the JSON and by number (never from listing text, which prints priority -310 as `raw - 10` on nft 1.0.9): set names, types, flags, `size`, default timeout, the chain's hook, priority and policy, and the rules in order. Equal: send nothing. Different: refuse new containment with `table_drift` and report; no automatic repair of drift. **Layout mismatch** (the table lacks, or has, the interval sets compared with the layout the self-test result allows): if every `blocked*` set is empty and the journal has no live entry, delete **our own** table and re-bootstrap (journal `table_recreated{layout_changed}`); otherwise `table_drift` until the owner runs `domovoi helper nft-recreate` locally after releasing the blocks. A change of `max_elements` or the default timeout is the same maintenance operation, never an `add set`. After a bootstrap or whenever the rendered exclusion text differs from the JSON-read `excl*` members, rewrite the exclusion sets (template above).

**Table loss.** If the table is absent while the journal holds a live (applied or confirmed, not expired by its deadline) block: write journal `lost` for each, re-create only the empty structure, and report `containment_lost` with `lost_blocks`. A request that concerns a lost block (`confirm`, `rollback`) answers `containment_lost`; any other request proceeds as a normal request with a fresh safety check and carries `lost_blocks` in its outputs. The helper never re-applies a lost block.

#### 6.4.2 Existing flows, bridge gap

Our chain has no `ct` accept, so non-offloaded established flows are cut by the drop. Flowtable hosts: `nft -j list flowtables` (all tables) returns `flowtable` objects; if any exists, after `apply` run `conntrack -D -s X` for a single host and for prefixes of at least `/24` (IPv4) or `/120` (IPv6) (**[validate]** the prefix form; wider prefixes are not deleted). Report `existing_flows`: `cut` (flowtables exist and the deletion exited 0), `n/a` (no flowtable), `may_persist` (flowtables exist and the deletion failed, is unsupported, or was skipped as too wide). The UI never says "contained" for `may_persist`. Bridge gap: `bridge_gap=true` when an interface with a `device` link in `/sys/class/net/<if>/` also has a `master` whose target has a `bridge` directory (read through `std::fs` from a configurable `/sys` root so tests use a fixture tree); reported on every `confirm`. Hardware offload is not covered and is stated in the docs.

### 6.5 Hub driver and the closed dispatch type (`domovoi_engine::containment`, **fixed**)

```rust
/// What may be asked of an agent for containment. A closed type: no key, no raw script.
pub enum ContainCall { Apply(BlockSpec), Confirm(BlockSpec), Rollback(BlockSpec), List }
pub struct BlockSpec { pub block: ExecutionId, pub net: Net, pub ttl: SignedDuration }
pub struct ContainDispatch { pub execution: ExecutionId, pub call: ContainCall, pub plan_hash: PlanHash,
    pub authorization: AuthorizationKind, pub expires_at: Timestamp, pub expected_safety: ExpectedSafety }
```

`ContainDispatch::expected_safety` is mandatory in the type (every phase of `Contain` carries it; `List` carries `None` and is built by a different constructor). The hub link gets a method on the same pattern as 42's package methods (`start_contain`/`contain_result`, default bodies returning `LinkError::Transport`, implemented by `WsAgentLink`, `PolicyLink` and the testkit fake), built on the existing `start_capability` plumbing, and a capability object `ContainRelay` next to `PackageRelay` (core `ports/caps.rs`). `RAW_OPS` and the EXEC-2 cards get the new raw names counted exactly once in `skill_policy.rs` and once in `runtime/caps.rs` (copy 42's cards); the **only** caller of `ContainRelay` is `execute_containment` in `crates/domovoi-engine/src/containment/phases.rs`, which takes an `AuthorizedContainment` built from a stored human `Authorization` or a stored policy permit by the same boundary pattern 45 uses for `unit.restart` (`execute_unit_restart`, `UnitRelay`). **If 45 is not merged or its `execute_unit_restart` pair is missing, stop and report**: 45 extends the skill-only `execute_step` (`operations/execute.rs:38-135`) for `unit.restart` only; 46 adds `execute_containment` beside it, a sibling following the same pattern and not a second framework.

Driver state machine (stored in `containment_blocks`, section 8; every change is one `Transaction` with an audit row, guarded by the previous phase and version):

```
Queued ──apply──▶ Applying ──ok──▶ Applied{applied_at} ──confirm──▶ Confirming ──ok──▶ Confirmed{deadline}
                      │                │                                  │
                      │ refused/failed │ confirm window over / any failure │ expired | rolled_back | containment_lost
                      ▼                ▼                                  ▼
                  NotRun/Failed     RollingBack ──ok──▶ RolledBack     Expired | Lost | OutcomeUnknown
```

Procedure (fixed order):

1. **Gate.** For a policy authorization: refuse unless `drills_state == passed` for the host (`drills_not_passed`) and the verified identity is current (47c's gate; else `attestation_stale`/`key_mismatch` etc.). For a human authorization with a verified gate: `ExpectedSafety::Verified` from the stored identity. For the unverified human path (6.10): `ExpectedSafety::Unverified{reason}` and only with the stored typed confirmation. Never fabricate a fingerprint for `unverified`.
2. **Apply.** Dispatch `apply` (own execution id `E1`; `block = E1`). Committed `Applying` **before** the request is sent (as 42 and 23 do). On `Succeeded`: commit `Applied{applied_at = hub clock at result}`.
3. **Confirm, within the window.** After `Applied`, the hub performs its own reachability check, which is the `confirm` request itself reaching the live session. It must be **sent within 20 s of the apply result** (hub clock; the kernel's 30 s runs from the helper's `create`, so the budget leaves margin for the round trip). `confirm` has its own execution id `E2`, `block = E1`, the same plan hash and args except `phase`. Committed `Confirming` before sending. On `Succeeded`: `Confirmed{deadline = now + ttl}`.
4. **Failure paths.** `confirm` not sent in time, link error before delivery, `Failed{..}`, or `Refused`: do **not** retry blindly. Send `rollback` (execution id `E3`, `block = E1`) if the session is alive; if it is not, leave the block to expire on its own (30 s) and settle `Expired`. A `confirm` answered `expired` (the element was gone) settles `Expired`; **no automatic re-apply**: a new block needs a new request, a fresh safety check and a new authorization. `rolled_back` and `containment_lost` settle those states. `OutcomeUnknown` (timeout, dropped session) stays pending and is reconciled (6.5.1).
5. **Early release.** The owner's "Release block" sends `rollback` with a human authorization under the same plan hash rule (new execution id, `block = E1`).

#### 6.5.1 Reconciliation

On each authenticated `hello` of a host (the composite reconciler 42 introduced, `runtime/mod.rs`; add the containment service to it), for every block of that host in `Applying`, `Applied`, `Confirming` or `RollingBack`: ask `JournalStatus::status_batch(host, [E1/E2/E3], expected_epoch)` and map as 21 and 42 do (`Finished{ok}` -> the phase's success, `Finished{failed}` -> parse the `"<token>: .."` message, `Running` stays, `NotFound` -> that phase `NotRun`, `OutcomeUnknown`/epoch change -> `OutcomeUnknown`). An `Applied` block whose `applied_at + 30 s` has passed without a confirm is `Expired` (the kernel removed it). The hub never retries a phase after dispatch. Whether an element is really present is settled only by an owner-initiated `list_blocks` (6.1).

### 6.6 Journal records and gate state (`containment::journal`, `containment::gate`, **fixed**)

The helper's write-ahead journal is a file `containment.journal` under `/var/lib/domovoi-helper` (the only writable path of the helper unit; ADR 0031 decision 5), one JSON line per record, each written and `fsync`ed **before** the nft call it describes, under the same single `state.lock` 47c uses for the key, state and mutation limiter (**no second lock**): `intent{op, block, source, set, ttl_s, at}`, `applied`, `noop`, `failed{token}`, `confirmed{deadline}`, `rollback_intent`, `rolled_back`, `lost`, `table_recreated{reason}`, `orphan{element}`. A torn final line is ignored and reported; a corrupt non-final line makes the journal `unreadable` and refuses all containment (`journal_corrupt`; no automatic reset). Compaction: records of blocks finished more than 24 h ago are dropped on start. Active-block bookkeeping reads this journal; **authoritative membership is the JSON read of the sets**.

```rust
pub enum Gate { Open { interval_path: bool }, BlockedLeak, BlockedUntested(UntestedReason) }
pub enum UntestedReason { NeverRun, Inconclusive, KeyChanged, PlatformUntested }
pub struct SelftestResult { pub key: SelftestKey, pub hosts: Verdict, pub prefixes: Option<Verdict>, pub ran_at: Timestamp, pub replacements: u64, pub control_leak_ratio: f64 }
pub struct SelftestKey { pub kernel_release: String, pub nft_version: String, pub layout: Layout }
pub enum Verdict { Pass, Fail { leaked: u64 }, Inconclusive(String) }
pub enum DrillsState { NotRun, Passed { at: Timestamp }, Failed { at: Timestamp } }
```

Mapping (6.8): hosts `Pass` -> `Open{interval_path:false}`; hosts and prefixes both `Pass` -> `Open{interval_path:true}`; hosts `Fail` -> `BlockedLeak` (everything); prefixes `Fail` -> `Open{interval_path:false}`; any `Inconclusive` or a key that differs from the current kernel, `nft` version or layout, or no result -> `BlockedUntested`. A Failure to run is never a pass. On an Untested platform the 47 tier switch (per-host owner opt-in, audited) must also be on, else `BlockedUntested(PlatformUntested)`; read it from 47a: `[platform] allow_write_groups_on_untested` in `helper.toml`, surfaced as `TierDecision::helper_write` (`Allow` / `NeedsOptIn` / `Refuse`) and the `write_opt_in` field of the `Groups` answer.

### 6.7 Agent backend and the helper operations (**fixed** behaviour)

**Agent** (`crates/domovoi-agent/src/containment/`), reached through a `firewall.*` arm in `requests/capability.rs` beside 45's `unit.*` routing in `requests/unit.rs` (copy the check order of `capability.rs:166-299` and of 45's 11 ordered steps). For `firewall.block_source` v1 and `firewall.list_blocks` v1 only, in this order:

1. journal fence (`journal_corrupt`); contract and version known; the owner's switch (`[helper]` offers the `nft` group in `HelperStatus::Ok{ops}`, else `contract_unknown`);
2. `authorization_kind = policy`: the ceiling check of 45 (entry for the exact contract, target, `prefix_len_*` floors, `max_per_hour`), and a policy run with `expected_safety = Unverified` is `refused{PolicyRequiresVerifiedSafety}`;
3. `expected_safety` missing for `firewall.block_source` (any phase) is `refused{malformed_safety}` (an unknown `Open` code); a malformed value is whatever 47c's strict type makes undecodable;
4. expiry (`expires_at <= now` -> `authorization_expired`);
5. argument parse **before** the journal: exactly the six keys; `source` re-canonicalised and equal; `prefix_len_*` recomputed and equal; `ttl_s` an integer; `block` a UUID; any mismatch is `Error{bad_request}` and leaves no journal record;
6. `journal.accept` (deduplication, the mutation limiter counted for **`apply` only**: `confirm` and `rollback` are phases of an accepted block and never consume the per-hour budget; `max_per_hour` likewise counts `apply`), `started`, the helper call, `finished`.

The agent does not interpret the helper's answer beyond mapping it: a helper refusal or failure becomes `Failed{code: refused, message: "<token>: <detail>"}` (`helper_unavailable` when the helper cannot be reached), redacted by the existing redactor. `confirm` and `rollback` require, in the **agent journal**, a finished `apply` under the same `block`; otherwise `unknown_block`.

**Helper operations** (group `nft`; request and response framing are 47a's; names `firewall.apply|confirm|rollback|list_blocks`). The helper treats every request as untrusted. Common prelude under the single `state.lock`: start checks (47a), `ensure_table()` (6.4.1), gate (6.6; not `Open` -> `containment_blocked`, the detail names the gate state, or `prefix_unsupported_platform` for a prefix target when `interval_path` is false), the exclusion recomputation from root-owned state (`effective_exclusions`; an input that cannot be established -> `exclusions_incomplete`; no management CIDR configured -> `no_management_cidrs`), then, **in this order**:

`apply`: `expected_safety` present and well formed else `malformed_safety`; for `Verified`, advance the generation and compare the full identity (fingerprint of its own key, `boot_id`, generation, both hashes): unequal `safety_changed`, uncomputable `safety_unknown`; `Unverified` skips only this comparison. Then canonicalise `source` (`invalid_target`, `mapped_form`), `check_target` (`prefix_too_wide`, `exclusion_overlap`), protected units are not relevant to this operation, `ttl_s` (`ttl_too_short` when `<= 30`, `ttl_above_maximum` when above the smaller of the helper's `[safety.containment] max_ttl` and the agent's stricter limit; **refused, never clamped**), `max_active` (the JSON element count of `blocked4`, `blocked6` **and** `blockednet4`, `blockednet6` when the interval path is on, **foreign elements included**, plus one, against the smaller of `[safety.containment] max_active` and the agent limit; `max_active = 0` refuses everything: `max_active_reached`), the persisted mutation limiter (`mutation_limit`), then journal `intent`, template `apply`, `verify` (6.7.1), journal `applied`, conntrack (6.4.2), answer. `create` failing `File exists` is `already_present` (journal `noop`, no change, no TTL change on any kernel).

`confirm`: the same prelude and checks as `apply` (identity again, exclusions again, TTL limits again) because the safety set may have changed inside the 30 s: a refusal here lets the element expire, which is fail-safe. Block id must exist in the journal in `applied` with equal `source` and `ttl_s` (`unknown_block`, `block_mismatch`). Journal `intent`, replace template, read back and check `0 < remaining <= ttl_s`, journal `confirmed{deadline}`. On exit 1 run `classify_absent` (6.7.2). Never retry the batch blindly.

`rollback`: **never refused for a safety reason.** It still runs the prelude's start checks, `ensure_table()`, the lock and the journal, and canonicalises `source`, but it skips the identity comparison, the overlap check, the limits and the limiter, because it only removes a block. Journal `rollback_intent` **before** the batch, primary template, then the failure table: exit 0 `rolled_back`; `Too many open files in system` (set full, X absent) -> membership read: absent is success, present -> `delete` alone; `interval overlaps with an existing one` -> membership read; no exact X -> `absent_covered` (success, names the covering element, **never delete it**); kernel-stage `No such file or directory` -> `absent` after the table check; bare `Error: No such file or directory` -> `table_missing` (not absent).

`list_blocks`: runs `ensure_table()`, reads the sets as JSON, reconciles with the journal and returns `table_state`, the `gate` (with kernel release, `nft` version, layout), `drills`, the blocks (source, set, remaining seconds, journal phase), `active`, `orphans` (members with no journal entry: reported, still bounded by the set's default timeout, counted toward `max_active`), `flowtables`, `bridge_gap`.

Nothing here reads the model, the hub's database or any file but the helper's own config, state, journal, `/sys` and the kernel.

#### 6.7.1 Verification probe

After `apply` and `confirm`, in order: (a) the element is present with remaining time in `(0, T]` from the JSON read; (b) chain `contain` exists with hook `prerouting`, priority -310 and the template's rules in order (a deleted, reordered or re-hooked chain is caught); (c) the arrival counter of rule 1 is non-decreasing across two reads (a counter that never moves on a busy host means the chain is not traversed; the drop counter delta is reported as evidence, not required); failure of (a) or (b) is `postcondition_failed` and the helper runs the rollback batch before answering; (c) alone is reported (`chain_ok=false`) and does not fail the apply. The hub's round trip is the fourth check and is the `confirm` itself. The helper cannot send a packet from the blocked source; packet effectiveness is the self-test's and the drills' job.

#### 6.7.2 Expired versus rolled back (`classify_absent`)

Error text never decides. After any exit 1 of `confirm` or `rollback`: no table -> `table_missing` (wire `containment_lost`); JSON membership has exact X -> `present_unexpectedly` (re-read once); a covering element -> `covered_by_other`; the journal has a `rollback_intent` or `rolled_back` after the apply -> `rolled_back`; `now >= applied_at + 30 s - tolerance` (tolerance 2 s, `Clock` port) -> `expired`; else `absent_unexplained` (wire `containment_lost`, detail `element_missing`). All mutations hold the lock, so racing external actors are the only source of `absent_unexplained`.

#### 6.7.3 Token table (`containment::tokens`, **fixed**; one token per meaning)

Refusals before any nft call: `malformed_safety`, `safety_changed`, `safety_unknown`, `containment_blocked`, `prefix_unsupported_platform`, `invalid_target`, `mapped_form`, `prefix_too_wide`, `exclusion_overlap`, `exclusions_incomplete`, `no_management_cidrs`, `ttl_too_short`, `ttl_above_maximum`, `max_active_reached`, `mutation_limit`, `unknown_block`, `block_mismatch`, `table_drift`, `journal_corrupt`, `drills_not_passed` (hub only). Results: `already_present`, `rolled_back`, `absent_covered`, `expired`, `covered_by_other`, `containment_lost`, `postcondition_failed`. Operational: `set_full`, `resource_exhausted`, `nft_timeout`, `permission_denied`, `internal_template_error`, `unsupported_kernel`, `table_exists_race`, `helper_unavailable`.

Rejected aliases (do not introduce): `overlaps_exclusion`, `bad_target`, `ttl_above_max_ttl`, `table_missing` and `absent_unexplained` as wire tokens, `unverified_requires_human` (the wire has `PolicyRequiresVerifiedSafety`).

nft stderr mapping, matched on (operation, exit status, stage prefix, text), then the table check and the membership read; text alone never decides `expired`; the mapping must not depend on which line a full interval set's second error cites:

| Seen text | Token |
|---|---|
| `Could not process rule: Too many open files in system` | `set_full` |
| `Could not process rule: No such file or directory` on `delete element` | `element_absent`, then 6.7.2 |
| `element does not exist` (interval, nft 1.0.6 and later) | `element_absent`, then 6.7.2 |
| `interval not found in set` (nft 0.9.8 and 1.0.2) | `element_absent` |
| bare `Error: No such file or directory`, no `Could not process rule` | `table_missing` or `set_missing`: check the table; never absent |
| `interval overlaps with an existing one` | `covered_by_other` (rollback) / `table_drift` (apply) |
| `Could not process rule: File exists` on `create element` | `already_present` |
| `Could not process rule: File exists` on `create table` | `table_exists_race` |
| `Could not process rule: Invalid argument` (timeout into a set without the flag) | `table_drift` |
| `You must add 'flags interval' ...` | `table_drift` |
| `value too large`, `wrong time format`, `syntax error` | `internal_template_error` (a bug) |
| `Prefix length N is invalid`, `Could not resolve hostname` | `invalid_target` (rejected before nft; a hostname never reaches nft) |
| `Could not process rule: Cannot allocate memory` | `resource_exhausted` |
| `Chain of type "filter" is not supported, perhaps kernel support is missing?` | `table_drift` if the table exists on a re-read, else `unsupported_kernel` |
| `"destroy" command is not supported` / syntax error on `destroy` | `internal_template_error` (never used) |
| `Operation not permitted` and netlink errors | `permission_denied` (not observed; verify) |
| process timeout | `nft_timeout` |

### 6.8 Leak self-test (separate subcommand, **fixed**)

`domovoi helper nft-selftest [--if-changed] [--interval-only]` runs as root, never inside the long-lived helper. Key: (kernel release, `nft --version` string, layout). `--if-changed` exits 0 at once when the stored result's key equals the current key and the result is not `Inconclusive` older than 7 days. It writes `/var/lib/domovoi-helper/nft-selftest.json` (0600, atomic write, under the helper lock) and returns exit 0 for any completed run (the verdict is in the file), non-zero only for "could not run", which is **not a pass**.

Method (T's `p27`): a throwaway namespace (`ip netns add domovoi-st-<pid>`, always deleted, even on error; deletion tested), its own table `inet domovoi_selftest` (never the production table, never the default namespace, never a physical NIC or bridge), two UDP senders over a veth pair at the highest rate the machine sustains (target at least 450 000 packets per second in total; the achieved rate is recorded), a counter before the drop and one after it, the drop rule on the set under test, and a replacer looping the **exact** `confirm` template through the **same** executor code path the helper uses (`nft -f -`). A split `delete`/`add` control run proves the test can see a window (T's controls leaked 39 to 41%). The sender is the same binary re-invoked as `domovoi helper nft-selftest-sender` through `ip netns exec` (absolute program path, via `ProcessExecutor`; no `unsafe`, no raw sockets). Scenarios per set type the layout would use: a lone element, one neighbour above, one neighbour below, ten neighbours on each side; hosts in the plain layout, prefixes in the interval layout. At least 2 runs of 4 s per scenario **and** at least 20 000 replacements per scenario: each run continues until the replacement minimum **or** a 120 s cap; the cap hit first is `Inconclusive("replacements_not_reached")`.

Verdicts: `Pass` = 0 leaked packets in every scenario, the control leaked more than 1%, the replacement minimum reached. `Fail` = any leaked packet. `Inconclusive` = the control did not leak, the minimum was not reached, the namespace or the rate could not be created. The mapping to the gate is 6.6. A pass is weak evidence (it does not cover a real NIC, multi-queue receive, a conntrack rule, IPv6, lower rates, or the live set's contents): the UI and docs say "validated on this platform at start", never "never absent". Runtime is minutes, not the 30 s the synthesis assumed (Differences 7); `TimeoutStartSec` of the unit is 20 min.

**Units.** A one-shot unit `domovoi-helper-selftest.service` (`Type=oneshot`, root, `Before=domovoi-helper.socket`, `After=nftables.service`, `RemainAfterExit=yes`, `ExecStart=/usr/local/bin/domovoi helper nft-selftest --if-changed`, rewritten to the install prefix by the installer like 47a's units, so `UNIT-9` and `ID-3` pass) runs at every boot and after the installer; it holds the only directive set that may create a namespace (`CAP_NET_ADMIN` and `CAP_SYS_ADMIN` bounding set, **[validate] on the real unit and record it as the privilege statement**, no `RestrictNamespaces`). The long-lived helper units keep `RestrictNamespaces=yes` and never create a namespace. `After=nftables.service` goes on `domovoi-helper.socket` and on the helper's instance template. `ReloadPropagatedFrom=nftables.service` on `Accept=yes` instances is **recorded by a test, not adopted** (`reload_propagation_on_accept_instances_is_recorded`); runtime table-loss detection (6.4.1) is the guard. `UNIT-3` (47a, amended by 47d) asserts the directives of `domovoi-helper@.service` only; this unit gets its own clause and failing-input test in `xtask/src/arch/units.rs`. The installer (47a's `install.sh`) invokes `nft-selftest` after creating the accounts; its failure leaves the nft group refused (`BlockedUntested`) and never blocks the rest of the install.

### 6.9 Drills (a named suite, **fixed**)

There is no drill or dry-run mode in any document (Differences 6). The suite is: the six drills of `04` §9.1 as namespace tests (`drill_expiry_on_time_with_agent_stopped`, `drill_excluded_address_refused`, `drill_hub_unreachable_during_confirmation_window_block_gone_within_30s`, `drill_agent_restart_mid_ttl`, `drill_duplicate_dispatch_is_idempotent`, `drill_max_active_reached_is_refused`), the four named tests of `06` M9 (`block_expires_with_agent_stopped`, `unconfirmed_block_expires_in_30s`, `excluded_address_refused`, `model_cannot_trigger_containment`), and the leak self-test. They run in CI (section 12) and on a host as `domovoi helper containment-drill` (root, throwaway namespace, the same machinery as 6.8, minutes): it writes `/var/lib/domovoi-helper/drills.json` (`DrillsState` plus per-drill results and the self-test key) and `list_blocks` reports `drills`. A result with a different key (kernel, `nft`, layout) is `NotRun`. The hub refuses to dispatch a **policy-authorized** `firewall.block_source` unless `drills == Passed` (`drills_not_passed`, 6.5); human-approved containment does not need it. **Open item to report:** `04` §9.2 says drills must pass before a rule leaves shadow mode; the policy publication gate that would enforce it belongs to 40 and 45 and is not changed here.

### 6.10 Hub: approval paths, containment input, registry (**fixed**)

**Containment input for the evaluator.** `engine/src/containment/input.rs` builds `ContainmentInput` (39) from 47c's gate and 47's `ManagementCoverage`: `attested` = a verified, fresh (under 300 s from nonce issue) identity under the latest-outcome rule; `uncovered_paths` = discovered paths whose verdict is not `Covered`; `stale_confirmation`; `helper_incomplete` = the latest signed report was `complete=false`; `target_overlaps_verified_set` = `Net::overlaps` (6.3) against the **verified report's lines** (`cidr`, `hub_addr`, `link_peer`, `local_addr`, fixed ranges), never an agent-supplied value. Nothing is fetched during evaluation (39).

**Verified path** (unattended or human): requires a pinned key, a verified complete report, current confirmations. The full identity is in the plan (6.2), the permit's `safety_identity` (45/47c), and `ExpectedSafety::Verified` at every phase. **Unverified human path** (human only): the step's approval requires session, CSRF, **password** (step-up, `confirm_password` as 42), and a **typed confirmation equal to the canonical source** (`203.0.113.0/24`); the approval card (6.12) states that coverage cannot be verified, gives the reason token and lists every discovered path that is uncovered or unverified as "Not protected"; the hub persists `safety_assurance = unverified{reason, unprotected_paths}` on the block row and audits `containment.unverified_approved`; the dispatched `expected_safety` is `Unverified{reason}`. The hub never issues a `PolicyPermit` with `unverified`. The approval is made in `crates/domovoi-hub/src/auth/` only (`HumanApproval::new` rule); add `web_containment_authorization` beside 42's `web_package_authorization`. The authorization window is `config`-driven (reuse the step approval window) and must be at least 90 s; the driver refuses to start a block whose remaining window is shorter (`approval_window_too_short`).

**Registry.** `firewall.block_source` and `firewall.list_blocks` are `Available` on a host only when the helper is `Ok{ops}` and `ops` contains the four nft operation names. Extend `HelperState`/`index_state` (`registry.rs:265`) with the offered operations (a small addition: `HelperState::Ready` plus an `ops` set in the seen words); a missing operation is `NotInstalled`. The availability code stays free of the strings the gate test forbids (`gates.rs:125-145`): put the op-name check in a table of names, not in text mentioning `Contain`.

**Audit** (same transaction as the state change; actor `User` for owner actions, `System("containment")` for driver steps): `containment.requested`, `containment.applying`, `containment.applied`, `containment.confirming`, `containment.confirmed`, `containment.rolled_back`, `containment.expired`, `containment.lost`, `containment.not_run`, `containment.outcome_unknown`, `containment.refused` (detail `token`), `containment.unverified_approved`, `containment.released`, `containment.listed`. Subject `host:<id>`; details hold block id, canonical source, ttl, plan hash, safety assurance kind, execution ids; never a password, a key or a signature value.

### 6.11 HTTP API (`/api/v1`, **fixed** paths; **convention** DTO names)

All need `Authed`. Tag `containment`. Errors are `ErrorBody` with codes `containment_blocked`, `helper_unavailable`, `drills_not_passed`, `approval_window_too_short`.

| Method and path | Success | Notes |
|---|---|---|
| `GET /hosts/{name}/containment` | `ContainmentStatusDto { available, gate: {state, reason?, kernel?, nft?, layout?, ran_at?}, drills: {state, at?}, platform_tier, blocks: [BlockDto], observed_at?, lost_blocks?, bridge_gap?, existing_flows? }` | from `containment_blocks` and the last refresh; display only; works when unavailable |
| `POST /hosts/{name}/containment/refresh` | `ContainmentStatusDto` | login + CSRF; sends `firewall.list_blocks` with a human authorization; not-connected mapped as `hosts::logs` |
| `POST /operations/{id}/steps/{step}/containment/release` | `BlockDto` | step-up (password and typed source); sends `rollback` |

The step approval route of 27/29 is **extended, not restructured**: for a containment step the approve request additionally carries `unverified_acknowledged: bool` when the preview says `unverified`; a missing acknowledgement is 409 `unverified_not_acknowledged`. The step detail DTO gains `containment: { phase, source, ttl_s, assurance: verified|unverified{reason, unprotected_paths}, existing_flows?, bridge_gap?, expires_at? }`. The DTOs never contain a key, a path on the hub's disk or a journal line.

### 6.12 Web (`web/src/features/containment/`, **fixed** scope; read `web/DESIGN.md` first)

Only: (a) the **unverified-human variant** of the approval card, filling the optional slot 30 left (30's `docs/rework/prompts/30-operations-ui-timeline.md` lists it as owned by 46, 47 and 49; 49 section 6.9 leaves the helper and key screens to 47, so this prompt builds the containment variant and nothing of the helper key views): the warning sentence, the reason, the "Not protected" list, the typed-source field and the password field, all through the kit and `useActionFeedback`; (b) a **phase timeline** on the step detail (`apply`, `confirm`, then `confirmed until`, `expired`, `rolled back`, `lost`) with `existing_flows` and the bridge gap in plain words; (c) the **attention item** for `containment_lost` and `gate` states `blocked_leak`/`blocked_untested`, through 51's attention extension point (read how 51 registers items; if it is not merged, stop and report); (d) a read-only **containment status** block on the host's Capability Center page only if 47 replaced the helper placeholder, else skip. Statuses, labels, tones, hints and icons come only from `web/src/design/tone.ts`; words never say "contained" for `may_persist`, never "never absent", never "supported". Every element carries a visibility category (`Visible`), P0 for the unverified card, the phase timeline's current state and the lost/blocked attention item. Prefer one PR; if the web part exceeds the 250-line file limits or the bundle budget, keep it minimal rather than splitting.

## 7. Ordered implementation steps

1. **Inspect and verify; report differences.** Open every file in section 3. Confirm 45 and 47 are merged and read their real names (`POLICY_DISPATCHABLE`, `execute_unit_restart`, `PermitIssuer`, helper request types, `helper.toml` schema, `SafetyIdentity`, gate, `ManagementCoverage` implementation, the helper config's per-host write opt-in). Re-run `ls docs/rework/prompts` and `ls docs/adr` and the migrations directory. Write every difference down; **stop** if one changes a decision (notably: 45's `unit.restart` dispatch is missing, 47c is not merged, `ExpectedSafety` is not strict).
2. **Pure core** (6.1 to 6.4, 6.6): `containment/*`, the contracts, `TargetKind::NetworkSource`, `canonical_network_identity`, `ContainmentPlan`; fix every exhaustive match; unit tests and `insta` snapshots of every template.
3. **Helper nft group** (6.4, 6.7): executor over `ProcessExecutor` with `FakeProcessExecutor` tests, ensure, apply, confirm, rollback, classify, journal file, flows, verify, list. PURE tests first, then the namespace tests (step 6).
4. **Self-test and drills** (6.8, 6.9): subcommands, units, result files, gate mapping.
5. **Agent backend** (6.7) behind 45's dispatch.
6. **Namespace CI harness** (section 12): the `containment-ns` job and the separate load job; run the namespace suite in WSL as the repository rules say and record where each ran.
7. **Hub**: migration and store (section 8), driver and reconcile (6.5), input builder and approval (6.10), link method and relay, arch rules and ADR, API (6.11); regenerate OpenAPI.
8. **Web** (6.12) and the smoke test additions.
9. **Docs** (section 15), then the checks and the completion report.

## 8. Database and persistence

**Migration** `crates/domovoi-store/migrations/00NN_containment_blocks.sql` (`NN` = next free number after listing the directory on your branch; describe it by contents). STRICT table, integer millisecond timestamps, enums validated in Rust (never SQL `CHECK`); the only checks are hex length, positive values and non-empty text.

```sql
-- One block of one source on one host: the hub's record of the three phases.
CREATE TABLE containment_blocks (
    block_id           TEXT PRIMARY KEY,                 -- the apply execution id
    host_id            TEXT NOT NULL,
    operation_id       TEXT NOT NULL,
    step_id            TEXT NOT NULL,
    source             TEXT NOT NULL CHECK (length(source) > 0),   -- canonical net
    ttl_s              INTEGER NOT NULL CHECK (ttl_s > 30),
    plan_hash          TEXT NOT NULL CHECK (length(plan_hash) = 64 AND plan_hash NOT GLOB '*[^0-9a-f]*'),
    phase              TEXT NOT NULL,
    authorization_kind TEXT NOT NULL,
    assurance          TEXT NOT NULL,                    -- JSON: verified{identity} | unverified{reason, unprotected_paths}
    confirm_execution  TEXT UNIQUE,
    rollback_execution TEXT UNIQUE,
    applied_at         INTEGER,
    deadline           INTEGER,
    existing_flows     TEXT,
    bridge_gap         INTEGER,
    failure_token      TEXT,
    failure_message    TEXT,
    created_at         INTEGER NOT NULL,
    updated_at         INTEGER NOT NULL,
    version            INTEGER NOT NULL DEFAULT 1
) STRICT;
CREATE INDEX containment_blocks_host ON containment_blocks (host_id, phase);
```

`AggregateKind::ContainmentBlock`, `JournalKind::Containment` (`"containment.phase"`, payload `{block, host, from, to, token?}`), `StateWrite::Containment(ContainmentWrite { Create, Phase { block, expected: PhaseGuard{phase, version}, .. } })`; a guard mismatch is `CommitError::Conflict` and the caller re-reads. Add the arms to `crates/domovoi-store/src/uow/writes.rs`, the in-memory twin and the crash summary (the compiler lists them). Terminal rows older than the retention window of 57 (verify) are purged in the existing nightly job; audit rows are never pruned. **N-1 upgrade test** beside the others: existing rows survive, the table exists and is empty.

**Helper files** (not the hub database): `/var/lib/domovoi-helper/{containment.journal, nft-selftest.json, drills.json}`, 0600, written atomically (temp file, `fsync`, rename, directory `fsync`) under the single `state.lock`, created by the helper only after 47a's start checks pass. Containment never writes anywhere else and never reads `/etc/domovoi` beyond `helper.toml`.

## 9. Protocol and compatibility

- **Wire 1.2 is unchanged**: `run_capability` with contract `firewall.block_source`, version 1, six args, `expected_safety` set, one plan hash for three executions; replies are the existing `capability_result`, `refused`, `error` and `action_status_answers`. Tokens are strings inside existing messages. No golden frame changes.
- **Older agents** (no 45 or 46) never advertise the contracts: availability is `NotInstalled`/`Unavailable` and a step settles `not_run` before any frame. An agent whose helper lacks the nft operations is the same.
- **Older hubs** ignore the table. A hub without 46 never dispatches the contract.
- **Config**: helper `[nft] table`, `max_elements`, `call_timeout` and the `[safety.containment]` keys of `04` §8.3 are **47a's config schema** extended here with the bounds below; unknown keys remain startup errors. `[safety.containment] max_ttl` is additionally refused above `31536000s` (365 days; `nft` accepted `31536000s` in tests and refused larger values), which tightens `04b` §2's grammar by an upper bound (list the follow-up edit); `max_elements` 1 to 10 000, default 1 000, refused at or above 2^32 (the wrap is a measured hazard); `max_active` default 0 and not above `max_elements`.
- **Minimum versions**: none is claimed. The core primitives behave identically on the pairs measured; any pair not measured must pass the self-test before containment writes. `destroy` needs nft 1.0.9 and is never used.

## 10. Error handling and recovery

| Situation | Behaviour |
|---|---|
| helper absent, not `Ok` or lacking the ops | availability `Unavailable`/`NotInstalled`; step `not_run`; nothing sent |
| gate not `Open` | helper `containment_blocked` (detail: `blocked_leak`, `blocked_untested`, or untested platform without the owner switch); prefix target with `interval_path=false`: `prefix_unsupported_platform` and the UI text "Blocking whole networks is not enabled on this host; block single addresses" |
| attestation stale, key mismatch, report incomplete | hub gate refuses at dispatch (47c's reason); the human may use the unverified path |
| target overlaps the verified set | evaluator `Deny` (`ContainmentTargetExcluded`); helper `exclusion_overlap` independently |
| authorization expires inside the window | `approval_window_too_short` before apply; `authorization_expired` from the agent on a later phase |
| `apply` refused or failed | block `NotRun` (token kept); nothing in the kernel |
| `apply` ok, `confirm` not sent in 20 s, link lost, or refused | rollback if the session lives; otherwise expiry in at most 30 s; settle `Expired` |
| `confirm` answers `expired` | `Expired`; no automatic re-apply; a new block needs a new request, safety check and authorization |
| `confirm`/`rollback` answers `containment_lost` | `Lost`; audit `containment.lost`; attention item; the hub re-requests only through a normal request |
| timeout or dropped session after dispatch | stays pending; reconciled on the next `hello` (6.5.1); never recorded as a failure |
| hub restarts in any window | `Applying`/`Confirming`/`RollingBack` reconcile from `action_status`; `Applied` past 30 s is `Expired`; nothing is retried automatically |
| helper crash mid-request | the journal's `intent` without a result: on start the helper reads membership and classifies (`applied` if present, else `failed{crashed}`); kernel timeouts continue regardless |
| `nftables.service` start or reload with `flush ruleset` | table gone, elements gone, no timer fires: detected at the next request (6.4.1), `containment_lost`, empty re-creation |
| table drift (foreign change) | `table_drift`, no new containment, no automatic repair |
| journal unreadable | `journal_corrupt`, all containment refused until the owner repairs locally |
| set full | `max_active_reached` before nft; `set_full` is the second line |
| reboot | blocks are gone (volatile table); permits are invalid by `boot_id`; the hub settles them `Lost`/`Expired` on reconcile |

A flood from the blocked source can delay the hub's confirmation past 30 s: the block then expires. That fails safe for management and is ineffective for containment; say so in the docs and the UI text, and do not claim otherwise (no confirmation-latency measurement exists).

## 11. Security requirements

- **Processes (R3, R4).** `nft` and `conntrack` are spawned only through `ProcessExecutor` with argv, the fixed environment, an absolute program and a timeout; scripts go on stdin. No `sh -c`. Gate tests: no code path builds a script containing `destroy`, `flush ruleset`, or a table name other than the configured one (the only `delete table` is the layout and maintenance recreate of our own table); templates are `insta` snapshots; the TTL formatter emits only `<n>s`, `n >= 1`; step 1 uses `create` and a bare `add` on a possibly existing element appears only inside the `confirm` and `rollback` templates; no template contains `add set`, `add chain` or `add rule` for an existing table (the bootstrap starts with `create table`); no `ct` rule in a chain whose priority is below -200; the priority is a named constant that equals no manager's number (-300, -200, -140, -100, -90, 0, 10, 100, 110).
- **Authority.** No path creates a block except an owner-approved step (human) or a PolicyPermit (45) on a verified gate with passed drills; `AuthorizationKind::Policy` with `unverified` is refused at the hub (never issued) and at the agent (`PolicyRequiresVerifiedSafety`). The LLM catalogue is untouched. The unverified human path needs password, typed source and an explicit acknowledgement.
- **Helper independence.** The helper recomputes exclusions and checks everything for every caller, with `expected_safety` required (`malformed_safety`); `rollback` is the one deliberate exception, because it only removes a block (a decision to flag to the owner; pinned by `rollback_is_never_refused_for_a_safety_reason` and by a gate test that no other operation skips a check).
- **Targets.** Strict canonical parsing; IPv4-mapped targets refused; hostnames never reach nft; both families, both overlap directions; `/0` never; the helper's prefix floors apply to every caller.
- **Privilege statement.** The long-lived helper holds `CAP_NET_ADMIN`; only the self-test and drill units may create a namespace and links. The statement is part of the ADR and the helper docs.
- **No secrets.** None stored or logged; the password is never kept; journal and DTOs hold no key.
- **Resource bounds.** `max_elements` 1 to 10 000; per-call timeouts; list read cap 8 MiB; at most one helper request mutating at a time (the lock); the self-test cap of 120 s per run.
- **Boundary and ADR.** `xtask/src/arch/rules.rs`: `RAW_OPS` and EXEC-2 cards for the new link names (one site each in `skill_policy.rs` and `runtime/caps.rs`), `BOUNDARY_PAIRS` and cards for `AuthorizedContainment::of` and the `execute_containment` call (one site each, `crates/domovoi-engine/src/containment/phases.rs`), a rule that only the helper's `nft` module names the nft templates' renderer (`containment::templates` is imported only from `<helper>/src/nft/`, the self-test and tests), and the helper never imports the hub. A numbered ADR (next free) records: the closed `ContainCall`, the phase and plan-hash decision, the rollback exception, the gate and self-test, what the checker cannot see. Run `cargo xtask arch-check` and with `--base`.

## 12. Testing requirements

Kinds: `PURE` needs no kernel (runs everywhere, including Windows); `NS` needs root, `ip netns` and a real `nft` (Linux only; WSL for local runs); `LOAD` is an `NS` test that sends traffic at high rate. Fakes: `FakeProcessExecutor` (script `nft` answers per stdin text), `FakeClock`, `FakeAgentLink` (extend: containment phases, scripted results, ordered frame recording), the in-memory repos and `uow_apply` twin. Tests never sleep on the `Clock` port.

**Real elapsed time.** The kernel expires elements on its own clock. `NS` tests that need a kernel timer to fire (`kernel_timeout_expires_element_after_helper_sigkill`, `unconfirmed_block_expires_in_30s`, the expiry drills) set short timeouts (3 to 5 s) with direct `nft` calls in the harness and poll with a bounded loop (100 ms steps, hard cap 15 s); the helper's own 30 s stays a constant that tests assert on the rendered template, not by waiting 30 s.

**CI (decided).** A job `containment-ns` in `.github/workflows/ci.yml`, run when files under `crates/domovoi-core/src/containment/`, `<helper>/src/nft/`, `crates/domovoi-agent/src/containment/` or the workflow change; it installs `nftables`, `iproute2` and `conntrack`, uses **`sudo ip netns` or `sudo unshare --net`, never `unshare --user --map-root-user --net`** (recent Ubuntu refuses unprivileged user namespaces under AppArmor), runs the `NS` tests with `DOMOVOI_NS_TESTS=1` and skips each with a visible reason when a prerequisite (`nf_flow_table`, `conntrack`) is missing. A second job `containment-leak` (`workflow_dispatch` and a weekly schedule) runs the `LOAD` tests and **prints the runner's kernel release and `nft` version**: it learns about the runner's pair, not about the matrix. Kernel variation needs virtual machines and is the owner's validation matrix (section 14). Add both jobs to the table in `TESTING.md` with local commands. Neither job is added to the required `needs:` list at first: `containment-ns` joins it only after three consecutive green runs and the owner's agreement (the rule 47a sets for its `privileged` job), and `containment-leak` never does.

**Fixtures.** Recorded stderr and `nft -j` JSON per `nft` version come from the namespace suite on CI and from the research probe outputs only after redaction. The raw probe outputs contain host details (LAN addresses, container names, firewall rules): **committed fixtures use only the documentation ranges** `192.0.2.0/24`, `198.51.100.0/24`, `203.0.113.0/24` and `2001:db8::/32`, no real host or container name; research hosts are cited by report letter, not by name.

**Pure core.** `canonical_ipv4_and_ipv6_text_is_stable`, `mapped_v6_target_refused_and_mapped_exclusion_canonicalised`, `hostname_and_prefix_with_host_bits_refused`, `excluded_target_refused_for_overlap_in_both_directions_v4_and_v6`, `fixed_exclusions_cover_loopback_link_local_multicast`, `incomplete_exclusion_inputs_refuse_all_containment`, `coalesce_merges_overlapping_and_adjacent_ranges`, `ttl_zero_is_rejected_before_nft`, `ttl_at_or_below_30_is_refused`, `ttl_above_maximum_refused_not_clamped`, `max_elements_at_or_above_two_pow_32_is_rejected_at_config_load`, `max_ttl_above_one_year_is_rejected_at_config_load`, `max_active_zero_refuses_everything`, `containment_plan_hash_changes_with_each_input` (source, ttl, host, host key, contract, assurance each flip it), `containment_plan_hash_ignores_phase_and_execution_id`, `containment_plan_domain_is_not_plan_v2`, `contract_args_have_exactly_the_six_keys`, `prefix_len_args_always_carry_both_families`, `ceiling_covers_a_v6_target_under_a_v4_only_floor`, `contracts_validate_and_join_known_contracts`, `network_source_target_requires_a_target_arg`, `ipnet_identity_binds_host_and_key`, `contain_call_has_no_script_or_key_argument`.

**Templates and gates (PURE).** `templates_match_the_snapshots` (bootstrap hosts, bootstrap with prefixes, apply, confirm, rollback, rollback fallback, exclusion rewrite, reads), `no_template_uses_add_for_step_one`, `no_template_contains_destroy_flush_ruleset_or_a_foreign_table`, `ttl_formatter_emits_only_positive_seconds`, `chain_below_minus_200_has_no_ct_rule`, `chain_priority_is_not_a_known_manager_number`, `bootstrap_over_a_live_table_sends_nothing_when_the_json_matches`, `bootstrap_never_sends_add_set_add_chain_or_add_rule_over_a_live_table`, `nft_and_conntrack_are_spawned_only_through_the_process_port`, `no_sh_c_anywhere_in_the_helper`, `error_token_mapping_matches_recorded_stderr_per_nft_version`, `list_members_parses_json_fixture_per_nft_version`, `gate_blocks_apply_and_confirm_when_leak_state_is_not_open`, `prefix_target_is_refused_when_interval_path_is_disabled`, `gate_mapping_matches_the_table_of_6_6`, `selftest_result_is_keyed_by_kernel_nft_and_layout`, `leak_self_test_inconclusive_blocks_containment`, `leak_self_test_replacements_not_reached_is_inconclusive`, `failure_to_run_the_self_test_is_not_a_pass`, `expected_safety_missing_is_refused_malformed_safety`, `verified_identity_mismatch_is_safety_changed`, `unverified_skips_only_the_identity_comparison`, `rollback_is_never_refused_for_a_safety_reason`, `no_operation_but_rollback_skips_a_check`, `agent_refuses_unverified_with_policy_label`, `no_management_cidrs_refuses_containment`, `mutation_limit_counts_apply_only` (persistence across a helper restart is 45's RW-37 test `mutation_limit_survives_helper_restart`; 46 reuses the same limiter file), `journal_record_is_written_before_the_nft_call`, `torn_final_journal_line_is_ignored_and_corrupt_middle_refuses`, `containment_runs_under_the_single_state_lock`.

**Namespace tests (`NS`, named in synthesis 04 section 7.1; the first group needs no load).** `kernel_timeout_expires_element_after_helper_sigkill`, `create_existing_element_reports_already_present_and_keeps_ttl`, `add_on_existing_element_behaviour_is_recorded_per_kernel` (records refresh, no-op or shortening as a fixture; the helper never depends on it), `step_one_create_succeeds_on_absent_plain_and_interval_prefix` (also range and expired element), `rollback_of_absent_element_succeeds_on_plain_and_interval_sets` (also expired and exact `/24`), `rollback_on_full_set_falls_back_to_membership_read`, `rollback_with_covering_prefix_reports_covered`, `replace_of_expired_element_fails_and_does_not_recreate`, `replace_losing_to_rollback_is_classified_rolled_back_from_journal` (20-way race, 15 rounds), `bare_no_such_file_is_table_missing_not_expired`, `element_without_timeout_takes_set_default`, `max_active_counts_foreign_elements_from_json`, `max_active_counts_interval_sets_when_the_interval_path_is_on`, `block_applies_to_docker_published_port_before_dnat` (namespace replica of a Docker ruleset fixture), `input_only_chain_misses_published_port`, `in_chain_accept_protects_exclusion_from_our_drop`, `flush_ruleset_reports_containment_lost_and_recreates_empty_table`, `established_flow_is_cut_without_ct_accept`, `ct_established_accept_at_minus_310_does_not_exempt_an_established_flow` and `ct_established_accept_after_conntrack_priority_exempts_it`, `bootstrap_rerun_does_not_duplicate_rules` (a second bootstrap starting with `create table` is refused with `File exists`; this was never run in the research), `ensure_reports_table_drift_when_set_size_or_default_timeout_differs`, `layout_change_on_an_empty_table_recreates_it_and_on_a_used_table_drifts`, `exclusion_sets_accept_coalesced_ranges`, `size_wrap_applies_to_interval_timeout_sets_and_config_load_still_refuses_two_pow_32`, `offloaded_flow_is_cut_after_conntrack_delete` (needs `nf_flow_table`), `existing_flows_reports_may_persist_when_conntrack_fails`, `bridge_gap_is_reported_for_a_bridged_physical_nic_fixture`, `verification_probe_detects_a_reordered_or_rehooked_chain`, `reload_propagation_on_accept_instances_is_recorded`, `selftest_namespace_is_always_deleted`, `selftest_never_touches_the_default_namespace_or_the_production_table`.

**Load tests (`LOAD`, separate job).** `replace_never_exposes_an_absent_window` (UDP flood, assert 0 leaked; the split control must leak, proving sensitivity; per set type and per scenario; **expected to fail for the interval layout on kernel 6.8 with nft 1.0.9**, and a failing record there is the correct outcome, not a defect), `leak_self_test_control_run_leaks_so_the_test_can_see_a_window`, `leak_self_test_fails_on_any_leaked_packet`, `leak_self_test_reports_replacement_rate`, `interval_layout_replace_leak_matrix_lone_above_below_both_sides` (records, does not assert 0), `pairing_6_8_with_1_0_9_runs_the_full_namespace_suite_and_the_leak_test` (a matrix row for a VM; expected today: suite passes, interval leak test fails).

**Drills** (6.9): `drill_expiry_on_time_with_agent_stopped`, `drill_excluded_address_refused`, `drill_hub_unreachable_during_confirmation_window_block_gone_within_30s`, `drill_agent_restart_mid_ttl`, `drill_duplicate_dispatch_is_idempotent`, `drill_max_active_reached_is_refused`, `block_expires_with_agent_stopped`, `unconfirmed_block_expires_in_30s`, `excluded_address_refused`, `model_cannot_trigger_containment` (the last in the engine, against the closed `MetaTool` and `ReadTool` catalogues and the absence of any chat or tool path).

**Engine and hub** (fake clock, fake link): `apply_is_committed_applying_before_the_request_is_sent`, `confirm_is_sent_within_20s_of_the_apply_result`, `confirm_not_sent_in_time_rolls_back_or_expires`, `expired_confirm_does_not_reapply`, `phases_share_one_plan_hash_and_distinct_execution_ids`, `confirm_and_rollback_use_the_block_of_the_apply`, `policy_dispatch_refused_without_passed_drills`, `policy_permit_is_never_issued_with_unverified`, `verified_dispatch_carries_the_stored_identity`, `unverified_human_path_needs_password_typed_source_and_acknowledgement`, `unverified_approval_persists_assurance_and_audits_it`, `containment_input_overlap_uses_verified_report_lines_only`, `reconcile_maps_each_journal_answer_per_phase`, `applied_past_30s_without_confirm_settles_expired`, `containment_lost_settles_lost_and_never_reapplies`, `crash_between_applying_commit_and_send_leaves_not_run`, `registry_requires_the_nft_ops_in_the_helper_status`, `registry_code_stays_free_of_forbidden_strings`, `list_blocks_is_owner_initiated_only`, `containment_status_endpoint_works_when_unavailable`, `release_requires_step_up_and_typed_source`, `audit_rows_exist_for_every_phase_and_hold_no_secret`, `n1_upgrade_adds_an_empty_containment_table`. **Web**: the card variant, the timeline, the attention item, `p0_visible_in_every_profile` for them, axe over each, `containment_words_never_say_contained_for_may_persist`, `containment_words_never_claim_support`, and the smoke mock extension.

### 12.1 RW-37 tests owned here: 15

The RW-37 list has 76 names: 47c owns 60 (its section 12.2), 45 owns 1 (`mutation_limit_survives_helper_restart`) and this prompt owns the 15 below. Each is a named test at the level shown; where a test of the lists above already covers the behaviour, the RW-37 name is added as its own test (a single name, one owner).

| Test | Level | Notes |
|---|---|---|
| `helper_refuses_target_overlapping_exclusion_even_if_agent_requests` | PURE with fake helper | an agent request that claims the target is fine is still refused (`exclusion_overlap`) |
| `helper_refuses_overlap_without_expected_safety` | PURE | refused as `malformed_safety` before the overlap test; the 47c test `helper_refuses_missing_expected_safety` covers the missing field alone |
| `human_containment_with_no_pinned_key_uses_unverified_path` | engine | 47c's `decide` supplies the reason `key_not_pinned`; this test drives the approval card and the dispatch |
| `human_containment_with_expired_attestation_uses_unverified_path` | engine | reason `attestation_stale` |
| `human_containment_with_invalid_signature_uses_unverified_path` | engine | reason `signature_invalid` |
| `human_containment_with_incomplete_routes_uses_unverified_path` | engine | reason `report_incomplete` |
| `unattended_containment_refused_in_every_unverified_case` | engine | existing wire code `PolicyRequiresVerifiedSafety`; `unverified_requires_human` is not added |
| `agent_refuses_unverified_with_policy_label` | agent | helper-side counterpart is 47c's `agent_accepts_refuses_unverified_for_policy_authority` |
| `exclusion_overlap_refused_even_on_unverified_human_path` | engine and helper | |
| `no_management_cidrs_refuses_containment` | PURE | named in the templates list above; owned here |
| `human_approved_containment_shows_uncovered_paths` | engine, web card | 47c's `unprotected_paths_lists_every_uncovered_candidate` supplies the list; this test checks it is shown |
| `agent_limit_stricter_than_helper_applies` | agent | the lower of the agent ceiling and the helper maximum wins |
| `agent_limit_above_helper_is_refused` | agent | renamed from `_is_capped`: refused, never clamped |
| `ttl_above_maximum_refused_not_clamped` | PURE | |
| `helper_active_count_is_authoritative` | helper | the agent's own count never overrides the helper's JSON element count |

Run Linux-only tests in WSL as the repository rules describe, and say which ran where.

## 13. Acceptance criteria

1. Nothing under `crates/domovoi-proto/` changes; no golden frame changes; protocol stays 1.2.
2. `firewall.block_source` and `firewall.list_blocks` validate, join `known_contracts()`, and are available on a host only when the helper offers the nft operations; a ceiling entry with `prefix_len_v4` and `prefix_len_v6` covers and refuses as specified.
3. The helper sends only the section 6.4 templates; the gate tests of section 11 pass; no `destroy`, `flush ruleset`, `ct` rule, foreign table, `sh -c`, zero timeout or element without a timeout is representable.
4. Bootstrap is create-if-absent: over a live table nothing is sent when the JSON matches, drift refuses with `table_drift`, and a layout change recreates only an empty table.
5. `apply` is `create` with 30 s; `confirm` is one `delete`+`add` batch with `30 < ttl <= max_ttl`; `rollback` is the idempotent `add`+`delete` batch with the membership fallback; the failure table and the token table behave as specified on the recorded stderr of every `nft` version fixture.
6. Expired, rolled back, covered and lost are told apart by the table check, the JSON membership read and the journal, never by error text.
7. Exclusions are computed in both overlap directions and both families; mapped targets, `/0`, hostnames and prefixes below the floor are refused by the helper for every caller.
8. `expected_safety` is required for every phase; `Verified` mismatches are `safety_changed`; `Unverified` skips only the comparison; `rollback` is never refused for a safety reason.
9. The interval path exists only where the self-test passed it; a prefix target elsewhere is refused with `prefix_unsupported_platform` and the UI text; an inconclusive or missing result blocks.
10. The self-test runs only as the separate subcommand, from the installer and the one-shot unit; the long-lived helper creates no namespace; the namespace is always deleted.
11. The hub sends `confirm` within 20 s of the apply result or rolls back; an expired block is never re-applied automatically; every phase has its own execution id and shares one plan hash; a hub restart in any window settles or reconciles each block and retries nothing.
12. A policy-authorized block is dispatched only with a verified gate, a policy permit and passed drills; the unverified path is human-only with password, typed source and acknowledgement and is audited; no model, chat or background path creates a block.
13. `containment_lost` is detected on the next request, the empty table is re-created, and nothing is re-applied.
14. `cargo xtask arch-check` (and `--base`) pass with the new cards and the ADR; the new `ContainRelay` has one caller.
15. The `containment-ns` job runs the namespace suite and the `containment-leak` job exists; every named test exists and passes where it runs; tests that did not run are listed with the reason.
16. Non-test files are at most 500 lines (web files 250); no new `#[allow]`, `unwrap`, `expect` or `panic!` in library code; committed fixtures contain no real address or name.

## 14. Migration and compatibility

Existing installs: nothing changes until the owner enables the helper's `nft` group, sets `max_active > 0` and at least one management CIDR, and the self-test passes; the default (`max_active = 0`) refuses all containment. The new table is empty and the previous release ignores it. Existing agents and hubs without this prompt never see the contracts. Rollout order for an owner: install the helper (47), run `domovoi helper nft-selftest`, run `domovoi helper containment-drill`, confirm management paths against a verified attestation, then enable. Reboot: containment does not survive it. **Validation matrix** (synthesis 04 section 7.4) stays the owner's work (Debian 12, Ubuntu 22.04, Ubuntu 24.04 including an HWE kernel, Debian 13 or Raspberry Pi OS, Fedora or RHEL, Docker, Podman, a bridged NIC, an offload NIC, a reboot with `nftables.service` enabled): this prompt provides the harness, not the result, and the book lists tested pairs only as "validated on" notes.

## 15. Documentation requirements

- `docs/book/src/reference/helper.md` (created by 47a, extended by 47d): the `nft` group, `[nft]` and `[safety.containment]` keys and bounds (`max_elements` 1 to 10 000, `max_ttl` up to 365 days, `max_active` default 0), the table layout, the privilege statement (the long-lived helper holds `CAP_NET_ADMIN`; only the self-test and drill units create a namespace), the files under `/var/lib/domovoi-helper`, the layout maintenance command, never saving `inet domovoi` into a persisted ruleset.
- `docs/book/src/guide/containment.md` (new, add to `SUMMARY.md`): what a block is, the 30 s two-step in plain words, verified and unverified approval, what the owner sees (phases, `existing_flows`, bridge gap), flood caveat, table loss, hosts-only platforms and the prefix message, drills, and "validated on" notes with no support claim and no "never absent".
- `docs/book/src/reference/protocol.md`: how `run_capability` carries the three phases, the six args, the plan hash rule, the token spelling inside existing messages.
- `docs/book/src/security.md`: the helper decides; rollback is the one operation that skips safety checks and why; the unverified path and its bounded concern.
- `ARCHITECTURE.md`: the containment driver and block aggregate beside the install service; the helper `nft` group; invariants (no model path, single relay, helper independence); §4 only if a dependency edge changed.
- `docs/adr/0038-containment-boundary.md` (expected number; section 11). Run `cargo xtask docs-links` and `cargo xtask docs-config`.
- Planning-text edits to **list in the PR as follow-ups for the owner, not make here** (`docs/rework/` is not part of the published tree): `04` §8.3 and §9.1 (ceiling keys `prefix_len_v4`/`prefix_len_v6`, the six args and three phases, the token spellings, the rollback exception, the `max_ttl` upper bound, `max_active` default), `04b` §2 (the `max_ttl` bound), RW-37's `unverified_requires_human` wording (the wire has `PolicyRequiresVerifiedSafety`), the index and the README line that says 46 has no file.
- Commits and the PR must not mention prompts, the build plan or coding agents.

## Execution constraints

You are implementing **one** change in the Domovoi repository (a Rust workspace with an embedded React UI).
This document is your complete brief. The planning documents it cites are background: what you need is
written here.

1. **Inspect before relying on anything.** Before you edit, open every file, type and function this
   brief names.
   - Confirm that each one exists and behaves as described.
   - If something differs materially, write down the difference **before** changing code. Examples: a
     different signature, a missing module, behaviour already implemented, or a prerequisite prompt not
     merged.
   - If the difference changes what you would build, stop and report it instead of guessing.
2. **Follow the contracts here exactly.** Do not change public interfaces, wire formats, database
   schemas or security behaviour beyond what this brief specifies.
3. **Stay in scope.** No unrelated refactoring, renaming or reformatting. No new dependencies unless
   this brief names them. Add any new dependency to `[workspace.dependencies]` in the root
   `Cargo.toml` and reference it from crates with `{ workspace = true }`.
4. **Never weaken a security check, a gate test or an arch rule to make something pass.**
5. **Stop and report rather than invent.** Do this when the brief contradicts the code, an
   architectural decision is missing, or a prerequisite does not exist.
6. **Be truthful.**
   - Never claim a test passed unless you ran it and saw it pass.
   - Never present stubbed or partial work as complete.
   - Mark anything unfinished as unfinished.

## Repository rules

These come from `AGENTS.md`, which CI enforces.

- **Crate boundaries.** `domovoi-core`, `domovoi-proto` and `domovoi-engine` do no I/O. Only
  `domovoi-runner` spawns processes, always with argv vectors and never `sh -c`. Dependency edges follow
  `ARCHITECTURE.md` §4. `cargo xtask arch-check` enforces all of this.
- **Approvals.** `HumanApproval::new` is called only in `crates/domovoi-hub/src/auth/`, and
  `HumanApproval::restore` only in `crates/domovoi-store/src/`. The LLM tool catalogue stays closed.
  Nothing but an authenticated human approves an action.
- **Code rules.**
  - No `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!` or `dbg!` in library code.
  - No new `#[allow(...)]`.
  - `thiserror` in libraries; `anyhow` only in `domovoi` and `xtask`.
  - Every public item has a doc comment saying what it is for.
  - Non-test source files stay at or under 500 lines; web source files at or under 250 lines.
- **Data and time.** Never edit a merged migration; add a new one. Choose a migration's number by listing
  `crates/domovoi-store/migrations/` on your branch and taking the next unused number; never assume it
  from a plan or a predecessor. Renumber only your own unmerged file if the branch gained a migration
  first, and describe the migration by its contents, not its number. Time comes from the `Clock` port;
  tests advance the fake clock from `domovoi-testkit` and never sleep. Secrets are
  `secrecy::SecretString` and are never logged, put into prompts or returned from the API.
- **Web.** Read `web/DESIGN.md` before touching `web/`. Build only from the kit in
  `web/src/components/`. Use `useActionFeedback` for every mutation and `describeError` for every error
  message, and tokens only for styling.
- **Tests.** Name tests after the behaviour, not the function. Never delete or weaken `tests/gates.rs`.
  Review every `insta` snapshot you accept.
- **Branch and commits.**
  - Work on a branch named as the brief says.
  - Commit messages are one line in Conventional Commits form, with exactly one scope, for example
    `feat(store): ...`. No body and no trailers.
  - Committed files, commit messages and the PR description must not mention prompts, prompt numbers,
    the build plan, coding agents or AI assistance.
- **Checks to run.** Run them for what you changed only. CI runs the full suite.
  - `cargo fmt --all`
  - `cargo clippy -p <crate> --all-targets -- -D warnings` for each crate you edited
  - `cargo nextest run -p <crate>` for each crate you edited, or `cargo test -p <crate>` if nextest
    is not installed
  - `cargo nextest run -E 'rdeps(<crate>)'` when you changed a public item that other crates use
  - `cargo xtask arch-check` when you added or moved modules or touched a `Cargo.toml`
  - for `web/`: `npm run lint`, `npm run typecheck`, and the vitest files for the changed area
  - On Windows, the hub and agent run only under WSL 2; unit and integration tests run natively.

## Completion report

End with a report in exactly this structure:

1. **Implemented behaviour**: what now works, in observable terms.
2. **Files**: added, changed and removed, each with one line on why.
3. **Implementation decisions**: any choice the brief left to repository convention, and what you chose.
4. **Interfaces**: types, functions, endpoints and messages introduced or changed, with signatures.
5. **Database and protocol changes**: migrations (file names), schema changes, wire changes.
6. **Security**: how each security requirement in the brief is met, and where it is tested.
7. **Tests added**: the test names, and which acceptance criterion each covers.
8. **Commands run**: each exact command and its actual result (pass, fail, counts).
9. **Not run**: tests or checks you did not run, and why.
10. **Deviations**: every place you departed from the brief, and why.
11. **Known limitations and remaining work**.
