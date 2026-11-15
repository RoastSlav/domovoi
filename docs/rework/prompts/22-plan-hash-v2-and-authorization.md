# 22 - Plan hash v2 and authorization

| | |
|---|---|
| Branch | `plan-hash-v2-and-authorization` |
| Milestone | M1 |
| Scope | Plan hash v2 (contract + version, `impl_hash`, canonical target identity, canonical arguments, `execution_id`); the approval binding columns; `Authorization` as a wrapper with private internals; `PolicyPermit` with a restricted `issue` factory; the architecture rules that cover every authorization constructor |
| Depends on (must be merged into `overhaul-investigation` before implementation) | 18, 20, 21 |
| Parallel with | none; 23 depends on this one |
| Produces | `ExecutionId`, `KeyFingerprint`, `Authorization`, plan hash v2, the approval binding columns |
| Gate | none beyond the listed implementation dependencies (18, 20, 21) |
| Suggested model | large |

## 1. Objective

Replace the v1 plan hash with a versioned plan identity that binds what actually runs, and introduce the
authorization value the later execution path consumes:

1. `PlanHash::compute_v2`, over the canonical contract identity, the implementation hash, the canonical
   target identity, the canonical arguments and the execution identity.
2. The approval binding columns, so a stored action records **the plan hash the human was shown** and
   who showed it to them.
3. `ExecutionId`, the opaque idempotency key of `03 RW-36`, which at M1 equals the `ActionId`.
4. `Authorization`, a wrapper whose internals are private and whose only constructors are the `human(..)`
   and `policy(..)` factories, plus the `PolicyPermit` type with its restricted `issue` factory. No
   `PermitIssuer` and no policy evaluation exist in this milestone.
5. Extension of the existing architecture scan so every authorization constructor has an owner, without
   changing what the compiler can and cannot enforce.

The single question this milestone must answer is: **is the approval that exists now an approval of
exactly the plan that will be dispatched, and can the hub tell which hash family bound it?**

This prompt is the contract owner for `ExecutionId`, `Authorization` and plan hash v2 (`07 §4`). Prompt 23
consumes them; 26 and 45 consume them later. Their names and shapes are fixed here.

## 2. Scope and non-goals

This prompt owns:

- `domovoi-core`: `PlanHash::compute_v2`, `PlanContract`, `PlanTarget`, `KeyFingerprint`, `ImplHash`,
  `ExecutionId`, `Authorization`, `PolicyPermit`, `ApprovalProvenance`, `PlanBinding` (unchanged) and the
  version selector that lets an action carry one hash family or the other.
- `domovoi-store`: one migration adding the binding columns, and the row mapping that reads and writes
  them.
- `domovoi-engine`: the action writer's approve path takes an `Authorization` and records the binding;
  the dispatch path recomputes the hash for **the action's own version**, and refuses on any difference.
- `domovoi-hub`: the approve handlers pass the displayed hash through as they do today, plus the
  provenance from the authenticated session; `ActionDto` gains the version label.
- `xtask`: one new architecture rule (plus its unit tests) confining every authorization constructor, and
  one confining the `plan:v2` literal.
- `web`: the plan tag says which binding family it is; a legacy action is labelled `v1 binding`.

This prompt does **not** own, and must not implement:

| Owner | Excluded work |
|---|---|
| 23 | `AuthorizedExecution`, the one `execute(...)` path, the one `collect_evidence` read boundary, the arch rules that stop transports reaching `AgentLink` |
| 24 | capability contract types (`ContractId`, `CapabilityContract`, `EffectClass`), the contract index, built-ins, the skill adapter |
| 25 | target identity **resolution** against the registry, availability, the catalogue, inventory UI |
| 26 | the operation/step/execution tables; the approval binding columns move to the execution detail there |
| 39, 40 | policy evaluation, decisions, policy versions, shadow, publication |
| 45 | `PermitIssuer`, policy-authorized issuance, the autonomy ceiling, guardrails; the unresolved-`OutcomeUnknown` unattended-reuse guard that prompt 21 defines but does not enforce |
| 47 | `SafetyIdentity`, helper attestation, the `expected_safety` values a permit binds |
| 20 | the wire. It is frozen at 1.2 by that prompt and this one changes nothing on it (`03 RW-42`) |
| 21 | hub reconciliation, `OutcomeUnknown` commit, the epoch rules |

Consequences:

- **No permit is issued, ever, by this change.** `PolicyPermit::issue` is a restricted data factory: private
  fields, no policy knowledge, no authority, and no product-code call site until `crates/domovoi-hub/src/policy/`
  lands in 45. The policy branch is reached through an accessor and refused in the writer with a named error.
  If you find yourself writing a `PermitIssuer`, a policy lookup, a ceiling check or an autonomy switch,
  stop: that is 45.
- **No second execution path.** Do not add an executor, a dispatch entry point or a service method that
  runs a skill. The writer and executor keep their shape; 22 changes what they hash and what they carry.
- **No wire change and no protocol version bump.** `run_capability` already carries `execution_id`,
  `plan_hash`, `authorization_kind` and `expires_at` (`03 RW-42`). Nothing here adds, renames or removes a
  message or field.
- **No reconciliation.** Do not touch `hello` handling, epoch comparison or `action_status` answers.
- **No policy evaluation, no autonomy, no containment.**

## 3. Drafting, launch and dependency

**Drafting (now).** This brief is written from the approved architecture (`03 RW-08`, `RW-11`, `RW-36`,
`RW-42`; `04 §8.1`, `§8.2`, `§8.4`, `§15`; `05 §4b`), the merged implementation already in the branch, and
the contract the earlier prompt **documents** define. Drafting needs no merge.

**Launching (later).** Implementation starts only when 18, 20 and 21 are merged into
`overhaul-investigation` and the model has read their merged code — not the names in this brief. At the
time this was written none of the three was merged: 18 is ready, and 20 and 21 have documents but no
merged implementation. Launch order is 20, then 21, then 22, then 23 (`07 §3`); they are never taken
concurrently.

**Dependency on prompt 21's document.** Everything about reconciliation, `OutcomeUnknown` and the epoch
is outside this brief. It needs only two things from that area, and takes them from `03 RW-09`,
`03 RW-36` and `04 §15`: a `Dispatched` execution is never auto-failed, and the idempotency key never
changes after dispatch. Prompt 21's document states the same rule for M1 — the execution id is the action
id — so the two agree on the only point of contact. Read its merged code at launch; if it settles an
execution-identity rule that contradicts `03 RW-36`, follow 21 and report the difference.

**What exists, what this prompt adds, what a later prompt owns.**

| Layer | Now (verified in this checkout) | This prompt | Later |
|---|---|---|---|
| Plan identity | `PlanHash` (32 bytes, hex), v1 over skill + manifest + host + canonical args (`approval.rs:24-33`) | `compute_v2` with its own domain line and field encoding; both versions in one type | 25 replaces the target producer; 26 moves the hash to the execution detail |
| Approval binding | the approve request carries the v1 hash and the writer refuses a mismatch (`writer.rs:283-285`) | the same check for both versions, plus `approved_plan_hash`, `approval_session_ref`, `approval_client` | 27 moves the endpoint to steps |
| Authorization | `HumanApproval` only; approve carries a `HumanApproval` (`writer.rs:39-45`) | `Authorization` wrapper with private internals and two factories, `PolicyPermit` with a restricted `issue`, refusal of the policy branch | 45 issues permits through `PermitIssuer`; 23/26 consume `Authorization` |
| Execution identity | `ActionId` only | `ExecutionId` newtype, `= ActionId` at M1 | 26/27 set `execution_id = step_id` |
| Enforcement | one line-based scan for `HumanApproval::new`/`restore` (`scan.rs:105-118`) | extended to every authorization constructor and to the `plan:v2` literal | 23 adds the boundary rules |

## 4. Verified current state (re-check before editing)

Read these before changing anything. If one now differs materially, record the difference before editing
and stop if it changes the design.

| Area | File | State when this was written |
|---|---|---|
| v1 plan hash | `crates/domovoi-core/src/action/approval.rs` | `sha256_newtype!(PlanHash)` at `:9-14`; `compute` at `:24-33` hashing `"plan:v1\n{skill}\n{manifest}\n{host}\n{canonical args}"`; `short()` at `:18-21` = first 8 hex |
| plan binding | same | `PlanBinding::{Full, Prefix}` `:36-63`; `MIN_PREFIX = 3` `:48`; `Prefix` matches a leading prefix only |
| approval value | same | `ApprovalChannel { Web{typed_confirmation}, Chat{platform, platform_user} }` `:65-81`; `HumanApproval` `:83-93` with private `user`, `channel`, `at`; `new` `:100-103`; `restore` `:109-112`; `Serialize` only — no `Default`, no `Deserialize` |
| action state | `crates/domovoi-core/src/action/request.rs` | `ActionStatus` `:15-42` (10 statuses); `ActionRequest` `:129-145` with `plan_hash: PlanHash`; `propose` `:152-178` generates `ActionId::new()` at `:163` and computes the v1 hash at `:169`; `from_stored` `:180-199`; `plan_hash()` `:258` |
| transitions | `crates/domovoi-core/src/action/transition.rs` | `ActionChange` `:11`; `ActionError` `:20-44`; `dispatch` `:104-118` compares the recomputed hash and moves to `Rejected { reason: "the plan changed after approval" }` on a difference |
| manifest hash | `crates/domovoi-core/src/skill/manifest.rs` | `ManifestHash` `:18-24` over the canonical JSON of the manifest `:143-151`; `SkillSummary` `:172-197` carries `manifest_hash` `:184`. **Before prompt 20** it carries no script digest; **after prompt 20** it carries the required `entry_script_digest: ScriptDigest` (prompt 20 §5.1-5.3). Read the merged code |
| validated args | `crates/domovoi-core/src/skill/validate.rs` | `ValidatedArgs` `:79-87`, only obtainable through validation; `canonical()` `:107-116` emits `name:byte_len:value\n` per value in declared order |
| hash newtype | `crates/domovoi-core/src/sha256.rs` | `sha256_newtype!` `:4-60`: `digest` is `pub(crate)`, `Display` is 64 lowercase hex, `FromStr` parses 64 hex, `Serialize` writes a string, `Deserialize` parses one |
| ids | `crates/domovoi-core/src/id.rs` | `define_id!` `:8-58` (UUIDv7, `new`, `from_uuid`, `Display`, `FromStr`, serde transparent); `ActionId` `:76-79`; **no `ExecutionId` exists** |
| target identity | `crates/domovoi-core/src/signal/keys.rs` | `TargetKey` `:75-107`: trimmed, 1–128 chars, no control characters, validated at construction |
| enrolled agent key | `crates/domovoi-core/src/host.rs` | `AgentIdentity { agent, host, public_key: [u8; 32], version, enrolled_at, revoked_at }`; `HostRepo::live_agent(host) -> Option<AgentIdentity>` is the single non-revoked agent of a host. **No key-fingerprint type exists**: `KeyFingerprint` is new in this prompt |
| incident subject | `crates/domovoi-core/src/incident/state.rs` | `Incident` `:77-102` holds `host`, `check: CheckKey`, `target: Option<TargetKey>`; accessor `target()` `:213` |
| approval construction | `crates/domovoi-hub/src/auth/approval.rs` | `web_approval` `:13-22` and `chat_approval` `:26-41`; the only two `HumanApproval::new` call sites in the product code |
| web approve handler | `crates/domovoi-hub/src/api/actions.rs` | `:107-137` loads the action first, requires a 64-hex `plan_hash` (400 otherwise) and passes `PlanBinding::Full` |
| chat approve | `crates/domovoi-hub/src/auth/interactions/buttons.rs` | `:160` builds `PlanBinding::Prefix` from the button |
| DTOs | `crates/domovoi-hub/src/api/dto.rs` | `ActionDto.plan_hash` `:300`; `ApproveRequest.plan_hash: Option<String>` `:370` |
| writer approve | `crates/domovoi-engine/src/actions/writer.rs` | `WriteCommand::Approve { approval: HumanApproval, chat_token, binding: PlanBinding }` `:38-45`; `approve` `:270-305` checks the chat token, then `binding.matches(&action.plan_hash())` `:283` → `ApprovalError::PlanChanged`; one guarded commit at `:291-300` |
| writer dispatch | same | `dispatch` `:333-356`: `freshness::check` `:340`, then `action.dispatch(plan, now)` `:347`, one commit; a change of plan becomes `Rejected`, not an error |
| dispatch recomputation | `crates/domovoi-engine/src/actions/executor.rs` | `:314` `PlanHash::compute(...)` from the current manifest and args, then `ask_dispatch` `:315`; `SkillDispatch.plan_hash` `:328-333` goes to the agent |
| freshness | `crates/domovoi-engine/src/actions/freshness.rs` | `check` `:42-57` rejects an expired approval, a missing incident and a resolved incident; it **reads the incident**, which is where a target identity is available at dispatch |
| cards | `crates/domovoi-engine/src/cards.rs` | `:112` `plan_short`; `:135` three hash bytes as the chat prefix |
| store rows | `crates/domovoi-store/src/repos/actions.rs` | `Bound` `:90-108`, `insert_action` `:158-174`, `update_action_from` `:176-195`; every column bound explicitly; `HumanApproval::restore` at `:59` |
| schema | `crates/domovoi-store/migrations/0001_init.sql` | `actions` table `:108-127`: `plan_hash TEXT NOT NULL` `:115`, `approved_by`, `approved_channel`, `approved_at` `:119-121`, `expires_at INTEGER NOT NULL` `:122` |
| migrations present | `crates/domovoi-store/migrations/` | `0001` … `0010_llm_admission.sql`. **Read the directory at branch cut; do not trust this row** |
| approval scan | `xtask/src/arch/scan.rs`, `xtask/src/arch/rules.rs` | scan `:105-118` rejects `HumanApproval::restore(` and `HumanApproval::new(` outside `HUMAN_APPROVAL_ALLOWED_PREFIXES` / `HUMAN_APPROVAL_RESTORE_ALLOWED_PREFIXES` (`rules.rs:168-175`, which also allow `crates/domovoi-testkit/`) |
| other arch rules | `xtask/src/arch/scan.rs` | `allow(clippy::disallowed_methods)` is only allowed in `domovoi-runner` `:91-97`; file length `:132-139`; crate-root docs `:141-155`; test-file exemption `:51-53` |
| clippy config | `clippy.toml` | `disallowed-methods` `:9-15` covers process spawning, `std::env::set_var`, `std::env::remove_var` and `std::thread::sleep`. **It has no approval entry** |
| rules file needs an ADR | `xtask/src/arch/git.rs` | `:82-95` fails `arch-check --base` when `xtask/src/arch/rules.rs` changed without a new numbered ADR |
| gate test | `crates/domovoi-core/tests/gates.rs` | `:67` `no_approval_without_human`; it must be extended, never weakened |
| web | `web/src/features/approvals/ActionCard.tsx`, `web/src/api/hooks/actions.ts` | `PlanTag planHash` at `:155`; the approve mutation sends `plan_hash` `:46-60` |

**Discrepancies to carry into the change, not to "fix" silently:**

1. `03 RW-11` cites `approval.rs:54` for `HumanApproval` and `:16-27` for v1. The fields and behaviour
   match; the line numbers are stale (struct `:83-93`, `compute` `:24-33`). Use the code.
2. `03 RW-11` lists four enforcement mechanisms: clippy `disallowed-methods`, an arch rule on
   `#[expect(clippy::disallowed_methods)]`, the text scan and a dev-only testkit check. In this tree only
   the **text scan** and the dev-only testkit rule (`rules.rs:157`) exist; `clippy.toml` has no approval
   entry, and the attribute rule (`scan.rs:91-97`) is about `allow(clippy::disallowed_methods)` in
   `domovoi-runner` only. Section 9 says what is actually there and what this prompt adds.
3. `03 RW-11` says v2 binds the entry-script bytes. Prompt 20 makes protocol 1.2 carry
   `SkillSummary::entry_script_digest`, so the hub reads that field instead of computing anything itself;
   this prompt imports the type and never defines a second one. If the merged 20 reports the digest under a
   different name, follow the merged code and report the difference.
4. `07 §6.1` records 18 as ready but not merged, while the v1 approve-time binding it specifies is already
   in this checkout (`api/actions.rs:115-133`, `writer.rs:283-285`, `ActionCard.tsx:155`, from the
   `plan hash on approval` change). Treat the code as the state, report the stale row rather than editing
   `07`, and keep it working: this prompt extends that check, it does not replace it.

## 5. Requirements and behavioural invariants

### 5.1 The v2 inputs

Plan hash v2 is SHA-256 over a canonical encoding of exactly five inputs, in this order:

| # | Input | Type | Where it comes from |
|---|---|---|---|
| 1 | capability contract id and version | `PlanContract` | the plan. At M1 the skill adapter supplies the skill name and the contract version `1`; 24 replaces the source, not the field |
| 2 | implementation hash | `ImplHash` | the manifest hash the hub holds plus the required `entry_script_digest` of that same skill summary, both from `hello.skills` (5.4) |
| 3 | canonical target identity | `PlanTarget` | the action's host, **its enrolled agent key fingerprint** and the linked incident's `CheckKey` and `TargetKey` at propose time (5.5); 25 replaces the producer, not the shape |
| 4 | canonical arguments | `ValidatedArgs::canonical()` | already stored on the action as a map, re-validated from the manifest |
| 5 | execution identity | `ExecutionId` | `= ActionId` at M1 (`03 RW-36`) |

Nothing else is in the hash. In particular **not**: the incident id, the proposer, the risk class, the
expiry, the approval time, the plan's short form, the entry path, the skill's `run_as`, or any UI text.
Each of those can change without invalidating an approval, and adding one would silently widen the
binding in a later change.

**The enrolled key fingerprint is not optional.** `03 RW-34` binds a host to `HostId` **plus** the
enrolled agent key fingerprint, and names re-enrolment as a change that invalidates a bound plan. A hash
that covered only `HostId` would survive a re-enrolment, which is the exact defect this input closes.

### 5.2 Canonical encoding

The encoding is produced by one function and one only. It is **not** a naive newline join:

```text
plan:v2\n
contract:{n}:{contract_id}@{version}\n
impl:{64 lowercase hex}\n
target:{n}:{canonical target}\n
args:{n}:{canonical args}\n
execution:{uuid}\n
```

Rules, all enforced by construction rather than by a parser:

1. **Length-prefixed variable-length fields.** `{n}` is the byte length of the following field text,
   ASCII decimal, no leading zeros, no sign. A field can therefore contain `\n`, `:` or `@` without
   becoming ambiguous.
2. **This is why the prefix is required.** `ValidatedArgs::canonical()` already ends each value with
   `\n` (`validate.rs:107-116`), and v2 puts `execution` *after* the arguments, so an untagged join would
   let two different `(args, execution_id)` pairs produce identical bytes. Do not reorder the fields and
   do not remove the prefix.
3. **Fixed shapes.** `contract_id` matches `^[a-z][a-z0-9_.-]{0,63}$`; `version` is a `u32` in decimal
   without leading zeros; `impl` and any digest are exactly 64 lowercase hex characters; `execution` is
   the canonical UUID text form of `ExecutionId`.
4. **One byte form per value.** UTF-8, `\n` only (never CRLF), no BOM, no trailing spaces, no empty
   fields, no extra blank line, exactly one trailing `\n`. Case is lower except where the value is a
   user-supplied identifier whose own type already fixed it.
5. **The target text** is itself `host:{uuid}/{fingerprint}\ncheck:{n}:{check}\ntarget:{n}:{target}`
   with `target` omitted entirely when the incident carries none — written as the literal `target:absent`.
   `{fingerprint}` is the enrolled agent key's SHA-256 (64 lowercase hex) and is **always present**;
   there is no `key:absent` form. That keeps "no target" distinct from "empty target" and is the reason
   `PlanTarget` is a type rather than a `String`.
6. **Rejection of non-canonical input is part of the constructors**, not of the hashing function.
   `PlanContract::new` takes a raw `&str` and returns an error for an id outside the grammar, so that check
   has one home. `PlanTarget::new` and `ImplHash::from_parts` are **infallible**: every argument is already a
   validated type (`HostId`, `KeyFingerprint`, `CheckKey`, `TargetKey`, `ManifestHash`, `ScriptDigest`), so
   there is nothing left to reject and the canonical text is produced by construction rather than by parsing.
   Do not give either of them a `&str` overload and do not make them return `Result` for symmetry.
   `PlanHash::compute_v2` takes the typed values, so a caller cannot hand it a raw string; a hash function
   that returned `Result`
   would push the same checks into every call site and leave the unvalidated path open.

`PlanHash::compute_v2(contract, impl_hash, target, args, execution)` is `#[must_use]` and takes the five
typed inputs; it has no string overload, no default argument and no `impl Into<String>`.

### 5.3 One type, two families

v1 and v2 are the **same** `PlanHash` type, distinguished by the domain line (`plan:v1`, `plan:v2`). That
keeps `PlanBinding`, the DTO, the columns and the chat prefix working unchanged. It also means the hash
bytes alone do not say which family they belong to, so:

- a stored action **must** record its family. This brief does it by column presence: `plan_hash_v2 IS
  NULL` means v1 binding, `plan_hash_v2 IS NOT NULL` means v2. There is no separate version column and
  no placeholder value.
- the version selector lives in one method, `ActionRequest::plan_hash_for_approval()`: `plan_hash_v2()`
  when present, else `plan_hash()`. The writer and the DTO both use it; nothing else branches on the
  columns.
- the two families must never collide on the same inputs, which the domain line guarantees. A test
  asserts it rather than assuming it.

Domain separation is the only defence against a downgrade, so it must not be removed, shortened or made
configurable.

### 5.4 `ImplHash` binds the manifest and the entry executable

`ImplHash` is a `sha256_newtype!` over:

```text
impl:v1\nmanifest:{64 hex}\nentry:{64 hex}\n
```

Both fields are always present. There is **no** `entry:absent` form, no optional field, no zero-filled
placeholder and no fallback.

- **Where the two values come from.** The hub already holds each agent's `SkillSummary` from `hello.skills`.
  Prompt 20 makes `SkillSummary::entry_script_digest: ScriptDigest` mandatory in protocol 1.2, computed by the
  agent's catalogue from the raw bytes of the resolved entry executable. The hub reads that field; it does
  not compute a script digest itself and never has the file.
- **`ScriptDigest` is prompt 20's type**, defined in `domovoi-core`'s skill module. Import it; do not define a
  second digest type here and do not alias `ManifestHash` or `PlanHash` into that role.
- `ImplHash::from_parts(manifest: ManifestHash, entry: ScriptDigest) -> Self` is the only constructor.
  There is no `from_manifest`, and no path that hashes the manifest alone.
- If an agent's summary is missing the field, the hub cannot build `ImplHash` and cannot compute v2: refuse
  to propose a v2 action for that skill rather than hashing the manifest alone. Under 1.2 the field is
  mandatory, so this is a protocol error, not a supported older agent.
- **What this binds.** The manifest bytes and the declared entry executable's bytes, which is exactly
  `03 RW-11`'s requirement. It does **not** bind transitive dependencies, interpreters, sourced scripts,
  other files in the skill folder, external programs or the rest of the runtime environment. Say that in the
  code comments and the PR; do not claim more.
- The agent independently rechecks the entry bytes against the digest it announced before every run (prompt
  20, `recheck_entry`). That is the agent's own guard; it is not a substitute for the hub's recomputation and
  the hub does not rely on it.

### 5.5 Target identity at M1

`PlanTarget` holds the host, the host's **enrolled agent key fingerprint** and the subject the plan names:
`host` (`HostId`), `host_key` (`KeyFingerprint`), `check` (`CheckKey`), `target`
(`Option<TargetKey>`), taken from the linked incident and the host's enrolment record. Both key types are
already validated at construction (`signal/keys.rs:75-107`), so the text is canonical by construction.

**`KeyFingerprint` is this prompt's type**, because prompt 22 lands first and prompt 25 consumes it. It
lives in `crates/domovoi-core/src/host.rs`, next to `AgentIdentity`, whose `public_key: [u8; 32]`
(`host.rs`) is the only input:

```rust
sha256_newtype!(
    /// SHA-256 over an agent's enrolled ed25519 public key bytes: the hub-computed half of a host's
    /// canonical identity (03 RW-34). Exactly 64 lowercase hex on the wire and in display.
    KeyFingerprint
);

impl KeyFingerprint {
    /// Hashes the 32 raw public-key bytes. The hub computes this from its own enrolled record;
    /// it is never accepted from an agent message, an incident or a client.
    #[must_use]
    pub fn of_public_key(public_key: &[u8; 32]) -> Self;
}

/// The canonical host identity text: `host:{host_id}/{fingerprint}`.
///
/// This is the **only** place the format is written. `PlanTarget::canonical()` uses it, and prompt 25's
/// registry parses it into a `TargetIdentity` for the values it reports, so a plan hash and a registry
/// resolution cannot drift apart. Infallible: both inputs are validated types.
#[must_use]
pub fn host_target_text(host: HostId, key: &KeyFingerprint) -> String;
```

- **Where the value comes from.** `HostRepo::live_agent(host)` (`core/src/ports/repos/hosts.rs`) returns
  `Option<AgentIdentity>` for the one non-revoked agent of that host; take `public_key` and hash it. There
  is no second key store, no new table, and no host-key type besides `KeyFingerprint`.
- **It is re-read, never remembered.** A propose-time value is what the human saw; the dispatch guard
  re-reads `live_agent(host)` and takes the fingerprint again. A display object, a cached DTO or the
  action row is never the source of the value at dispatch.
- **No enrolled agent, no v2 plan.** `live_agent(host) == None` (never enrolled, revoked, or the record is
  gone) means the host has no canonical identity: proposing a v2 action returns
  `PlanError::NoEnrolledKey`, and dispatch refuses. Do not hash a placeholder, a zero fingerprint, or
  `HostId` alone.
- **Re-enrolment invalidates the plan.** A new enrolment writes a new `AgentIdentity` with a new
  `public_key`, so the fingerprint and therefore `PlanTarget::canonical()` and the v2 hash change. An
  approval taken before it is `Rejected` at dispatch as `PlanChanged`, which is exactly `03 RW-34`'s
  "re-enrolment" invalidation row.
- **The host comes from the incident, not from a second argument.**
  `PlanTarget::of_incident(incident, host_key)` takes the incident and the already-derived fingerprint; it
  derives the host and the subject from the incident itself, so a caller cannot approve a plan on host A
  that names incident B. `PlanTarget::new(host, host_key, check, target)` exists for a caller that already
  holds a host and no incident — at M1 only tests — and it is not a shortcut around that rule.
- Recomputing it at dispatch needs the incident, which the writer already reads in `freshness::check`
  (`freshness.rs:42-57`). Read the incident once and pass it to both checks; do not read it twice.
- An incident that vanished or was resolved is already a rejection (`Stale::IncidentGone`,
  `Stale::IncidentResolved`). An incident whose target changed now also changes the hash and becomes
  `Rejected`.
- Prompt 25 replaces this producer with a registry-resolved identity: host, capability, canonical target
  kind and value, where a container identity embeds the same host identity text. The **field, its
  position and its encoding do not change**; only the text inside it does. That is why this is a producer
  change and not a hash-version change.
- Approvals created before 25 lands, and approved under the M1 producer, mismatch after it and are
  rejected at dispatch. Record that consequence in the PR description; do not add a compatibility shim
  that recomputes the old text.
- **Consumers.** Prompts 26 (operation/execution detail) and 45 (permit issuance) bind this same target
  text. When either is implemented it must take the `TargetIdentity` value and revalidate it at its own
  boundary — the stored text is evidence of what was approved, never a substitute for reading the
  current enrolment. Do not add a second resolver or a second identity format.

### 5.6 Behavioural invariants

1. **The approval is bound to the plan the human saw.** The approve request carries the hash the surface
   displayed. The writer refuses a mismatch before any status change, for both families.
2. **The displayed value is stored.** `approved_plan_hash` records exactly the hash the approval carried,
   independently of the plan columns, and is never rewritten. It is `NULL` for actions approved before
   this change, because for those the hub cannot honestly claim which value was shown.
3. **Dispatch recomputes the hash of the action's own family** from the current manifest, arguments,
   target, identity and entry-script digest. Any difference is a rejection, not an error, and never a
   dispatch. The `run_skill` request is a separate v1 value and is not the approval binding (6.5).
4. **`execution_id` never changes after dispatch.** At M1 it is the `ActionId`; nothing rewrites it.
5. **No approval survives a plan change of any covered input**: contract, version, implementation,
   target, arguments or execution identity.
6. **A legacy action keeps v1 semantics for its whole life.** It is approved, dispatched and rejected
   under v1, and is labelled as v1 in the API and the UI.
7. **Authority is unchanged.** Only `HumanApproval`, constructed only in `crates/domovoi-hub/src/auth/`,
   authorizes anything at M1. A policy authorization is reached through `policy_permit()` and refused with a
named error.
8. **Refusals are silent in the audit log**, as today: no approval entry is written when a hash
   mismatches (`writer.rs:270-305` writes only after the check).

## 6. Data model, interfaces, state transitions and transaction rules

### 6.1 New and changed core types

In `crates/domovoi-core/src/action/` (200-line modules; `plan.rs` is new and holds v2, the encoding and
its tests):

```rust
// plan.rs
pub struct PlanContract { /* id: String, version: u32 */ }
impl PlanContract {
    pub fn new(id: &str, version: u32) -> Result<Self, PlanError>;
    pub fn skill(skill: &SkillName, version: u32) -> Result<Self, PlanError>; // M1 adapter
    pub fn id(&self) -> &str;
    pub fn version(&self) -> u32;
}

pub struct PlanTarget { /* host: HostId, host_key: KeyFingerprint, check: CheckKey, target: Option<TargetKey> */ }
impl PlanTarget {
    /// Infallible: all four arguments are already validated types, so the canonical text is
    /// produced by construction. Do not add a `&str` form and do not make it return `Result`.
    pub fn new(host: HostId, host_key: KeyFingerprint, check: CheckKey, target: Option<TargetKey>) -> Self;
    /// The host, the subject and the key fingerprint all come from `incident` and `host_key`; there is no
    /// separate host argument. `host_key` is `KeyFingerprint::of_public_key(&live_agent(incident.host()).public_key)`.
    pub fn of_incident(incident: &Incident, host_key: KeyFingerprint) -> Self;
    pub fn canonical(&self) -> String;   // host_target_text(host, &host_key)\ncheck:...\ntarget:...
}

sha256_newtype!(/* docs */ ImplHash);
// `ScriptDigest` is prompt 20's type in `domovoi-core`'s skill module. Import it; do not define a second one.

impl ImplHash {
    /// SHA-256 over the manifest hash and the required entry-script digest. Both are always present.
    pub fn from_parts(manifest: ManifestHash, entry: ScriptDigest) -> Self;
}

impl PlanHash {
    pub fn compute_v2(
        contract: &PlanContract,
        implementation: ImplHash,
        target: &PlanTarget,
        args: &ValidatedArgs,
        execution: ExecutionId,
    ) -> Self;
}

// approval.rs (unchanged shape, one added type)
pub struct ApprovalProvenance { /* session_ref: Option<String>, client: String */ }

// authorization.rs
/// Who authorised a run. The representation is private, so the two cases are not public
/// variants and `Authorization { .. }` cannot be built outside this module.
pub struct Authorization { /* source: AuthorizationSource, all private */ }

/// Private on purpose: never exported, never named outside this module.
enum AuthorizationSource { Human(HumanApproval), Policy(PolicyPermit) }

impl Authorization {
    /// Calls are allowed only from `crates/domovoi-hub/src/auth/` and from tests.
    pub fn human(approval: HumanApproval) -> Self;
    /// Calls are allowed only from `crates/domovoi-hub/src/policy/` and from tests.
    pub fn policy(permit: PolicyPermit) -> Self;
    #[must_use] pub fn human_approval(&self) -> Option<&HumanApproval>;
    #[must_use] pub fn policy_permit(&self) -> Option<&PolicyPermit>;
    #[must_use] pub fn kind(&self) -> AuthorizationKind;
}

/// A packaged policy decision. Every field is private; the only constructor is the restricted
/// `issue` factory below, which evaluates nothing and grants nothing.
pub struct PolicyPermit { /* all fields private */ }

impl PolicyPermit {
    /// Calls are allowed only from `crates/domovoi-hub/src/policy/` and from tests. This is a
    /// data factory, **not** a `PermitIssuer`: it performs no policy evaluation, holds no
    /// registry, and no product code calls it in M1.
    pub fn issue(
        decision_id: &str,
        rule_ids: Vec<String>,
        policy_version: &str,
        plan_hash: PlanHash,
        issued_at: Timestamp,
        expires_at: Timestamp,
        ceiling_hash: &str,
        safety_identity: Option<&str>,
    ) -> Result<Self, PermitError>;
    #[must_use] pub fn plan_hash(&self) -> PlanHash;
    #[must_use] pub fn expires_at(&self) -> Timestamp;
    #[must_use] pub fn decision_id(&self) -> &str;
}
```

The two `Authorization` factories are the only way to build the value: there is no public variant, no
`From`/`Into`, no `Default` and no `Deserialize`. `authorization_kind` is derived from `kind()`, never from the
caller's word. `PolicyPermit::issue` exists so that the refusal test in §10.2 can build a real policy
authorization; it is restricted by the same scan as the two factories (§9.2), and it is deliberately not the
operational issuer, which stays absent until 45.

`04 §8.1` and `03 RW-11` write this type as an enum with two public variants. This brief keeps the same two
cases and the same meaning but puts them behind a private internal enum, because a public variant is a
construction path the call-site scan cannot see. Record the deviation from the canonical sketch in the ADR;
do not edit `03` or `04` to match.

`ExecutionId` goes in `crates/domovoi-core/src/id.rs` via `define_id!`, next to `ActionId`:

```rust
impl ExecutionId {
    pub fn from_action(action: ActionId) -> Self;  // M1: the idempotency key is the action id
    pub fn as_action(self) -> ActionId;            // for the M1 dispatch path
}
```

- `PolicyPermit` fields, from `04 §8.1` and `03 RW-11`: `decision_id`, `rule_ids`, `policy_version`,
  `plan_hash`, `issued_at`, `expires_at`, `ceiling_hash`, `safety_identity: Option<..>` (required only
  for `Contain`, `03 RW-37`). The three policy identifiers, `ceiling_hash` and `safety_identity` have no
  domain type in this milestone; hold them as documented opaque strings (sorted and deduplicated for
  `rule_ids`) and say so in their doc comments. 39/40 define the decision types and 45 issues permits
  through `PermitIssuer`; this struct is deliberately not `#[non_exhaustive]` so 45 can fill in real types
  in one place.
- `PolicyPermit` derives `Debug`, `Clone`, `PartialEq`, `Eq`, `Serialize`. **No `Default`, no
  `Deserialize`, no `pub` fields, no constructor other than `issue`.** Its accessors are `#[must_use]`.
- `Authorization` derives the same set over private internals. **No `Default`, no `Deserialize`, no public
  variant and no `From`/`Into` implementation**, from `HumanApproval` or anything else: a transport must
  name `Authorization::human(..)` or `Authorization::policy(..)`, which is exactly what the scan in §9.2
  checks. `kind()` is the only way to learn which case it is.
- `PermitError` is a small `thiserror` enum for `issue`'s own arguments: an empty `decision_id` or
  `policy_version`, an empty entry in `rule_ids`, and an `expires_at` that is not after `issued_at`. It says
  nothing about policy and nothing about authority.
- `ActionRequest` gains `plan_hash_v2: Option<PlanHash>`, `approved_plan_hash: Option<PlanHash>`,
  `provenance: Option<ApprovalProvenance>` (or the two provenance strings stored separately — pick one
  and keep the store row mapping in step) and `authorization_kind: Option<AuthorizationKind>` where
  `AuthorizationKind { Human, Policy }` serialises as `"human"` / `"policy"`.
- `ActionRequest::propose` computes v2 (`plan_hash_v2: Some(..)`, v1 still computed and kept as
  `plan_hash` so the compatibility path and the audit line have it). `from_stored` accepts both families
  and rejects a row whose stored hashes disagree with its status only at load, never by repairing it.
- `ActionRequest::approve` gains the authorization and the provenance, sets `approved_plan_hash` from the
  **request's** binding (not from the plan) and `authorization_kind` from the authorization. The state
  machine (`Proposed → Approved`) and `ActionError` are unchanged.
- `ActionRequest::propose` gains the canonical target, because v2 binds it and the current signature takes
  only `incident: IncidentId`. Take a `PlanTarget` (built by the caller from the incident **and the host's
  `KeyFingerprint`**) and say so in the report; do not read the incident or the enrolment inside
  `domovoi-core`, which does no I/O. `propose` returns `Err(PlanError::NoEnrolledKey)` rather than
  inventing a target when the caller has no fingerprint to pass.
- `PlanError` gains `NoEnrolledKey` with the message "the host has no enrolled agent key; a v2 plan cannot
  bind its identity". It is the only new variant, and it is a propose-time refusal: dispatch treats a
  missing key as a stale plan, not as an error to report to the approver.
- `Transition::dispatch` keeps the signature prompt 21 leaves, including its epoch argument, and still
  compares exactly one hash. The caller passes the hash of the action's own family.

### 6.2 The writer

`WriteCommand::Approve` becomes:

```rust
Approve {
    id: ActionId,
    authorization: Authorization,
    chat_token: Option<String>,
    binding: PlanBinding,
    provenance: ApprovalProvenance,
    reply: oneshot::Sender<Decision>,
}
```

Order inside `approve` (all before any write):

1. load the action;
2. ask the chat-token check, unchanged;
3. `binding.matches(&action.plan_hash_for_approval())` → `ApprovalError::PlanChanged`;
4. read the authorization through its accessors: `human_approval()` continues with that approval;
   `policy_permit()` — and any other case — returns the new `ApprovalError::NotYetSupported` and writes
   nothing. Do not match a variant; there is no public one.
5. build the transition, then one `commit_action` carrying the row, the audit entry and the journal entry,
   as today (`writer.rs:456-473`).

Nothing about the queue, the recovery handshake or the guarded write changes.

`WriteCommand::Dispatch` keeps `{ id, plan }`; `plan` is the **approval-family** hash — `compute_v2` for a v2
action, `compute` for a v1 action — and the writer keeps rejecting on mismatch. The executor must never
compute v1 for a v2 action's guard or the reverse: derive the family from the action, never from a feature
flag. This is a different value from the one sent to the agent in `run_skill`; see 6.5.

**A v2 guard re-reads the enrolled key.** Before computing `compute_v2`, the executor loads
`HostRepo::live_agent(action.host())` and takes `KeyFingerprint::of_public_key(&agent.public_key)`. It does
not reuse a fingerprint from the DTO, the action row or its own earlier pass: the whole point is that a
re-enrolment between approval and dispatch changes the value. `None` from `live_agent` (revoked, removed,
never enrolled) cannot produce a hash, so the action takes the existing stale path — `Rejected` with the
plan-changed reason, no agent call — instead of hashing without the key.

`ApprovalError` gains `NotYetSupported` (a policy authorization at M1) and keeps `PlanChanged`. Both are
logged at debug by `refused()` (`writer.rs:251-264`).

### 6.3 The hub

- `web_approval` and `chat_approval` keep returning `HumanApproval` and stay the only constructors. They
  gain no authority. The provenance is assembled next to them, in `crates/domovoi-hub/src/auth/`:
  `session_ref` is an opaque, non-reversible reference to the authenticated session (never the cookie,
  never a token), `client` is `"web"` or `"chat:<platform>"`.
- `POST /api/v1/actions/{id}/approve` keeps its body and its order: load the action, require a 64-hex
  `plan_hash` (400), then approve with `PlanBinding::Full`. It wraps the approval with
  `Authorization::human(..)` and passes the provenance. Every first-party caller sends the hash of the
  action's own family, which `ActionDto.plan_hash` now means.
- `ActionDto.plan_hash` is `plan_hash_for_approval()` as 64 hex — unchanged field, corrected meaning.
  Add `plan_hash_v1: Option<String>` (the stored v1 hash, for audit display) and
  `plan_binding: "v1" | "v2"` so a client never has to infer the family.
- Regenerate `web/src/api/openapi.json` and `web/src/api/schema.d.ts` (`cargo xtask openapi`, then
  `npm run gen:api`). Never hand-edit them.

### 6.4 Transaction rules

- Approval is **one** `UnitOfWork` commit: the action row with `plan_hash` family, `approved_plan_hash`,
  `approval_session_ref`, `approval_client`, `authorization_kind`, the audit entry and the journal entry.
  A failed audit fails the approval, as today.
- Dispatch is one commit: `Approved → Dispatched` or `Approved → Rejected`. There is no state where the
  row is dispatched with a hash that was not compared.
- The expiry sweep is what prompt 21 leaves: it resolves a `Proposed` action to `Expired` and an
  `Approved` action whose approval expired before dispatch to `NotRun`. Recovery is untouched. A `Proposed`
  v2 action that expires never had a binding written, which is correct: `approved_plan_hash` is `NULL` until
  approval. An approved action that expires keeps the binding it was approved with.
- Reads never repair: a row missing `plan_hash_v2` is a v1 action, not a corrupt one.

### 6.5 Two hashes, two jobs at M1

M1 keeps the existing agent request path: an action is still executed with `RunSkill`, and the agent still
recomputes **v1** there (`crates/domovoi-agent/src/requests/skills.rs:98-103`: `PlanHash::compute` over the
local skill name, manifest hash, host id and validated args, compared with `run.plan_hash`). Sending a v2
hash in that field would make every action fail on the agent. So the two hashes are deliberately different
values and must never share a variable name or a field:

| Value | Which hash | Carried by | Purpose |
|---|---|---|---|
| `approval_plan_hash` | the action's **family**: v2 for new actions, v1 for legacy | the approve request `plan_hash`, `ActionDto.plan_hash`, `approved_plan_hash`, `WriteCommand::Dispatch { plan }` | what the human approved and what the hub's dispatch guard re-checks |
| `runskill_plan_hash` | **always v1** | `SkillDispatch.plan_hash` (`executor.rs:328-333`), the wire `RunSkill.plan_hash` | the agent's existing local manifest/host/arguments consistency check |

Rules:

1. **New actions:** approval, the UI/API hash, `approved_plan_hash` and the dispatch guard use v2, including
   the entry-script digest, the canonical target, the arguments and `execution_id`. The hub recomputes v2
   immediately before dispatch and refuses a mismatch (`WriteCommand::Dispatch` → `Rejected`).
2. **The `RunSkill` request:** `runskill_plan_hash` is computed with `PlanHash::compute` exactly as today, for
   every action, v1 or v2. The existing agent-side check is untouched; it was never the human approval
   binding and does not become one.
3. **The entry bytes are guarded twice, independently:** the hub's v2 recomputation (which includes the
   digest) and the agent's own pre-execution `recheck_entry` from prompt 20. Neither substitutes for the
   other.
4. **The stored row keeps both.** `plan_hash` holds the v1 hash (legacy family, and the value used to build
   `RunSkill`), `plan_hash_v2` holds the v2 approval hash. `plan_hash_for_approval()` selects the approval
   family and is the only selector the writer and the DTO use.
5. **Legacy actions** use v1 for the approval and for `RunSkill`, exactly as before; nothing about them
   changes.
6. **Exact call sites and names**, so the two cannot be confused:
   - `actions/executor.rs`: compute `approval_plan_hash` and `runskill_plan_hash` into two separately named
     bindings; pass the first to `ask_dispatch`, the second into `SkillDispatch`.
   - `actions/writer.rs::dispatch`: `plan: PlanHash` is the approval-family value; compare it with
     `action.plan_hash_for_approval()` only.
   - `cards.rs`, `api/dto.rs`, `api/actions.rs`: the approval-family hash only.
   - `domovoi-proto` and `domovoi-agent`: untouched. Do not add, rename or reinterpret a wire field, and do
     not introduce `run_capability` here; it belongs to 24/25.

## 7. Files and components expected to change

| Path | Change |
|---|---|
| `crates/domovoi-core/src/host.rs` | `KeyFingerprint` (a `sha256_newtype!` over the enrolled public key) and `host_target_text`, beside `AgentIdentity` |
| `crates/domovoi-core/src/action/plan.rs` (new) | `PlanContract`, `PlanTarget` (including the host key fingerprint), `ImplHash` (from prompt 20's `ScriptDigest`), the canonical encoder, `PlanHash::compute_v2`, `PlanError`, tests |
| `crates/domovoi-core/src/action/authorization.rs` (new) | `Authorization`, `PolicyPermit`, `AuthorizationKind`, tests |
| `crates/domovoi-core/src/action/approval.rs` | `ApprovalProvenance`; doc comment on v1 stating it is the legacy family; no behavioural change |
| `crates/domovoi-core/src/action/request.rs` | new fields, `propose` computes v2, `approve` takes the authorization and provenance, `plan_hash_for_approval()`, `plan_hash_v2()`, `approved_plan_hash()`, `authorization_kind()` |
| `crates/domovoi-core/src/action/mod.rs` | re-exports |
| `crates/domovoi-core/src/action/transition.rs` | no signature change; doc comment naming the family |
| `crates/domovoi-core/src/id.rs` | `ExecutionId` |
| `crates/domovoi-core/src/lib.rs` | re-exports follow the existing pattern |
| `crates/domovoi-store/migrations/00NN_plan_binding.sql` (new) | section 8.1 |
| `crates/domovoi-store/src/repos/actions.rs` | columns, `Bound`, insert and update, `from_stored` mapping |
| `crates/domovoi-store/src/rows/action.rs` (if it holds the mapping) | the same fields |
| `crates/domovoi-engine/src/actions/writer.rs` | `WriteCommand::Approve`, `approve`, `ApprovalError::NotYetSupported`, dispatch wiring |
| `crates/domovoi-engine/src/actions/executor.rs` | compute two separately named values: the approval-family hash for `ask_dispatch` and the always-v1 `runskill_plan_hash` for `SkillDispatch` (6.5); load `live_agent` and pass the incident and the key fingerprint it already reads |
| `crates/domovoi-engine/src/actions/freshness.rs` | return the incident (or take it as an argument) so the target can be recomputed from one read |
| `crates/domovoi-engine/src/actions/mod.rs` | `ActionHandle::approve` signature |
| `crates/domovoi-engine/src/cards.rs` | unchanged behaviour; the plan tag text says which family, from one constant |
| `crates/domovoi-hub/src/auth/approval.rs` | provenance assembly next to the constructors |
| `crates/domovoi-hub/src/api/actions.rs`, `dto.rs` | `Authorization::human(..)` wrapping, provenance, DTO fields |
| `crates/domovoi-hub/src/auth/interactions/buttons.rs` | unchanged prefix binding; the approve call gains the provenance |
| `xtask/src/arch/rules.rs` | `POLICY_PERMIT_ALLOWED_PREFIXES`, `AUTHORIZATION_HUMAN_ALLOWED_PREFIXES`, `AUTHORIZATION_POLICY_ALLOWED_PREFIXES`, `PLAN_HASH_V2_ALLOWED_PREFIXES` and their doc comments |
| `xtask/src/arch/scan.rs` | the two new scan rules and their unit tests |
| `docs/adr/00NN-plan-hash-v2-and-authorization.md` (new) | required because `rules.rs` changed (`git.rs:82-95`); NN read from `docs/adr/` at branch cut |
| `web/src/features/approvals/ActionCard.tsx` | the plan tag shows `v2` or `v1 binding`; text from one constant; `InfoTip` explains what v2 covers and what it does not |
| `web/src/api/openapi.json`, `web/src/api/schema.d.ts` | regenerated, never edited |
| `web/src/test/fixtures.ts`, `web/src/dev/RespondDemos.tsx` | the new DTO fields |

Do not touch: `crates/domovoi-proto/**` (wire frozen), `crates/domovoi-agent/**`, `crates/domovoi-core/src/skill/**`
except reading, `crates/domovoi-hub/src/agent_server/**` except if a call site needs the new hash (it
does not), `web/src/api/hooks/actions.ts` except to type the new fields.

## 8. Migration and protocol treatment

### 8.1 The migration

One new file. **Choose its number by listing `crates/domovoi-store/migrations/` on this branch and taking
the next unused number** (`07 §7`, `_blocks.md`). Do not copy a number from this brief, from `07`, from
`main` or from another prompt: 17, 19, 26, 28, 34, 36, 40 and 42 also add migrations. Prompt 21 adds none,
so do not reserve one for it. The suffix describes the contents: `_plan_binding.sql`.

Contents:

```sql
ALTER TABLE actions ADD COLUMN plan_hash_v2 TEXT;
ALTER TABLE actions ADD COLUMN approved_plan_hash TEXT;
ALTER TABLE actions ADD COLUMN approval_session_ref TEXT;
ALTER TABLE actions ADD COLUMN approval_client TEXT;
ALTER TABLE actions ADD COLUMN authorization_kind TEXT
    CHECK (authorization_kind IS NULL OR authorization_kind IN ('human', 'policy'));

-- Every row that already has an approver was approved by a human; the approve path is the only
-- writer of approved_by, and it has no policy branch before this change.
UPDATE actions SET authorization_kind = 'human' WHERE approved_by IS NOT NULL;
```

Plus `CHECK` expressions that the two hash columns are 64 lowercase hex or `NULL`, written as
`length(x) = 64 AND x NOT GLOB '*[^0-9a-f]*'`, and a `CREATE INDEX` only if a test needs one — no index
for a column nothing queries.

Rules:

- `approved_plan_hash` is **not** backfilled. `NULL` means "approved before the hub recorded the displayed
  hash", which is the truth for those rows; backfilling it would assert a value the hub never received.
- The `authorization_kind` backfill above is deterministic from existing data, so it is allowed; nothing
  else is backfilled.
- No permit columns, no `safety_identity`, no `plan_target`, no version column: see 8.3.
- Never edit a merged migration. If another branch adds a migration first, renumber **your own** file
  before integration.

### 8.2 v1 to v2 compatibility

- **New actions** propose with `plan_hash_v2` set and are approved, dispatched and rejected under v2. The
  v1 hash is still computed and stored for the audit line and for the cards that show a short code.
- **Actions created before the upgrade** keep `plan_hash_v2 = NULL`, are approved against the v1 hash,
  recomputed as v1 at dispatch, and appear in the API and the UI as `v1 binding`. They expire or finish on
  that path; they are never migrated to v2.
- **An action that was approved under v1 is never re-planned under v2.** There is no code path that
  recomputes a v1 action's hash with `compute_v2`: the family comes from the row, so a v1 action cannot
  reach a v2 recomputation. If a future change wants to move an approved action to v2, it must require a
  new human approval, and that is not this prompt.
- **An action that is recomputed under v2 but was approved under v1** is therefore impossible by
  construction, and the guard is tested: `v1_actions_keep_v1_binding_after_upgrade`.
- **The API is unchanged in shape** (`plan_hash` still the field the client sends); its meaning is now
  "the hash of this action's family". A client that cached a v1 hash for a v2 action gets a 409, which is
  the intended behaviour of a stale card.

### 8.3 Columns this prompt deliberately does not add

- **Permit columns** (`decision_id`, `rule_ids`, `policy_version`, `issued_at`, `ceiling_hash`,
  `safety_identity`): M1 issues no permit operationally — the only `PolicyPermit::issue` call in the product
  is absent, and a test builds one solely to prove the refusal — so the columns would have no writer and no
  reader. `authorization_kind` is the discriminator M1 needs; 45 adds the permit columns with the issuer, and
  26 moves the whole set to the execution detail (`03 RW-11`, `04 §8.1`). Record this deferral in the PR.
- **A plan version column**: presence of `plan_hash_v2` is the version (5.3).
- **`plan_target`**: the target is derived from data the row already has (host, and the incident's
  `check`/`target`), so it needs no column and cannot drift from the args and incident it is computed
  from.
- **`expires_at`**: already on `actions` (`0001_init.sql:122`) and already enforced at dispatch
  (`freshness.rs:42-57`). `04 §8.1` lists it for the execution detail, where 26 will add it again for
  steps; nothing is needed here.

### 8.4 Protocol

No change. `PROTOCOL_VERSION` stays at the value prompt 20 leaves (1.2), no message, field, golden frame
or `RefusalCode` changes, and `run_capability`'s `plan_hash` and `execution_id` already carry what this
prompt produces. A "wire change" here would be a coordinated `03 RW-42` decision and is out of scope.

## 9. Security and failure handling

### 9.1 What this change does and does not prove

- **It proves**: the approval stored for an action names the plan the surface showed, for that action's
  hash family; and dispatch only proceeds when the plan recomputed now is byte-identical to it under the
  same encoding.
- **It does not prove**: that the process computing the hash is honest. A compromised hub can compute any
  hash it likes. Runtime limits remain dispatch recomputation, the agent's local checks, the ceiling, the
  helper allowlist and the audit chain (`03 RW-11` "Honesty", `04 §8.4`).
- **It does prove which entry executable was approved**, to the extent the agent's report is honest: the
  digest is over the resolved file's bytes, so a replaced or edited entry invalidates the approval. It says
  nothing about transitive dependencies, interpreters or other files, and the PR must not imply otherwise.
- **It does prove which host and enrolled agent key were approved**: the target text carries `HostId` and the
  hub-computed fingerprint of the enrolled public key, so re-enrolment invalidates the approval. It does not
  prove the agent still controls that key at execution time; the handshake's signature check does that, and
  the agent's own local checks remain the last line.
- At M1 it resolves the target only as far as the incident subject and the host identity (5.5); say that in
  the DTO doc comments and the UI tooltip.

### 9.2 The enforcement mechanism that exists, and what this prompt adds

The claim to make is the true one: **`HumanApproval::new` is protected by a line-based text scan, not by
the compiler.** `xtask/src/arch/scan.rs:112-118` looks for the literal `HumanApproval::new(` on a
non-comment line in a non-test file and reports `INV-1` unless the path starts with one of
`rules.rs:168-175`. A same-named function, a re-export under another path, a macro or a `From`
implementation would not be caught by text; the compiler enforces nothing here. Do not write a comment,
doc or PR sentence claiming a compile-time seal.

This prompt extends the same mechanism, in the same place:

1. **`rules.rs`** gains `POLICY_PERMIT_ALLOWED_PREFIXES = ["crates/domovoi-hub/src/policy/"]`,
   `AUTHORIZATION_HUMAN_ALLOWED_PREFIXES = ["crates/domovoi-hub/src/auth/", "crates/domovoi-testkit/"]`,
   `AUTHORIZATION_POLICY_ALLOWED_PREFIXES = ["crates/domovoi-hub/src/policy/",
   "crates/domovoi-testkit/"]` and `PLAN_HASH_V2_ALLOWED_PREFIXES =
   ["crates/domovoi-core/src/action/"]`. Each gets a doc comment naming the ADR and the invariant, in the
   style of the existing four. The permit and policy prefixes have no caller until 45, and are recorded now
   so the ownership is fixed before the issuer exists rather than edited into a security rule later.
2. **`scan.rs`** reports a violation when a non-comment line outside the allowed prefixes contains
   `PolicyPermit::issue(`, `Authorization::human(` or `Authorization::policy(`; when a line outside
   `crates/domovoi-core/src/action/` contains the literal `plan:v2`; and when any line outside
   `crates/domovoi-core/src/action/authorization.rs` contains `pub enum Authorization`, which would reopen
   the direct-construction hole this design closes. Test paths keep their existing exemption
   (`scan.rs:51-53`); that is deliberate, because tests must be able to build the values — including the
   `PolicyPermit::issue(..)` fixture §10.2 needs.
3. **Unit tests in `scan.rs`**, next to `human_approval_outside_auth_is_rejected`: one that rejects each
   factory in a hub transport, in the engine and in the store, and accepts it in the allowed prefix and in a
   test file; one that rejects a second `plan:v2` implementation outside core; one that rejects a
   `pub enum Authorization` outside its own module.
4. **`crates/domovoi-core/tests/gates.rs`** keeps `no_approval_without_human` (`:67`) and extends it: an
   `Authorization` can only reach `Approved` through `human_approval()` at M1. Extend
   `approvals_and_actions_cannot_be_deserialized` (`:153`) so `Authorization` and `PolicyPermit` are covered
   by the same "not deserialisable" assertion, rather than adding a second copy of the trait trick.
5. **An ADR** at `docs/adr/`, because `xtask/src/arch/rules.rs` changed (`git.rs:82-95`). It records the
   ownership rules, the wrapper-with-private-internals decision (and why a public enum was rejected), and
   why they are text scans.

What this prompt does **not** add: a `clippy.toml` `disallowed-methods` entry for the constructors. Adding
one would require `#[allow(clippy::disallowed_methods)]` at the legitimate call sites, and
`scan.rs:91-97` rejects that attribute outside `domovoi-runner`; the change would either break the build or
force a weakening of an existing rule. Record that reasoning in the ADR.

### 9.3 Bypass analysis

| Route | Why it does not bypass the binding |
|---|---|
| Another constructor | `PlanHash::compute_v2` takes `PlanContract`, `ImplHash`, `PlanTarget`, `ValidatedArgs`, `ExecutionId`; all but the last are only obtainable through validating constructors. A second encoder is caught by the `plan:v2` scan rule |
| A public `Authorization` variant | the type is a wrapper with a private internal enum, so `Authorization::Human(..)` and `Authorization::Policy(..)` do not exist; the two factories are the only paths and the scan covers both. A `pub enum Authorization` reappearing is a scan violation |
| Deserialisation | neither `Authorization` nor `PolicyPermit` derives `Deserialize`, and all their fields are private, so no transport can decode one |
| `Default` | neither type implements it, and `ApprovalProvenance` has no default session |
| Testkit | dev-only (`rules.rs:157`); it cannot appear in a shipped binary |
| A different approve endpoint | there is one approve path (`writer::approve`) and one dispatch path; 23 owns making that structural, and this prompt must not add a second |
| A hand-written row | writing to `hub.db` is in the threat model as an accepted v1 limitation (`04 §8.4`); a forged row is caught only by the audit chain and dispatch recomputation, and a forged hash that matches a forged plan is still limited by the agent's local checks |
| A stale card | the approve request carries the displayed hash; a plan that moved on returns `PlanChanged` (409) |
| A v2 action forced down the v1 path | the family comes from the row, not from the request or a flag; `v1_actions_keep_v1_binding_after_upgrade` and its mirror test assert it |

### 9.4 Failure handling

| Failure | Behaviour |
|---|---|
| `plan_hash` missing or not 64 hex | 400, nothing changes (as today) |
| `plan_hash` from another plan | `PlanChanged` → 409, action still `Proposed`, no audit entry |
| a policy authorization | `NotYetSupported`, nothing written, logged at debug |
| approval expired before dispatch | `Approved → NotRun` with `action.not_run` (prompt 21), never sent; not `Rejected` |
| plan changed between approve and dispatch | `Approved → Rejected` with the existing reason, one audit entry |
| target subject changed | as above, from the recomputed target |
| incident gone or resolved | existing `Stale` reasons, unchanged |
| store failure during approve | `ApprovalError::Store`, nothing written |
| legacy row with `approved_plan_hash = NULL` | loads, dispatches, is labelled v1; never repaired |

## 10. Tests and acceptance criteria

Name tests after the behaviour. Keep the exact names marked **(required)**; they come from `02 §4` and
`04 §8.1` and later prompts cite them.

### 10.1 Core (`crates/domovoi-core`)

- `plan_hash_v2_covers_every_v2_input`: changing any one of contract, version, implementation, target,
  arguments or execution identity changes the hash.
- `plan_hash_v2_is_domain_separated_from_v1`: the same inputs under `plan:v1` and `plan:v2` never collide.
- `plan_hash_v2_encoding_is_unambiguous`: a crafted pair where a naive newline join collides (arguments
  ending without a trailing separator, an execution id containing a would-be separator) produces two
  different hashes; the exact byte string is asserted once, as a literal, so a future refactor cannot
  drift the format silently.
- `plan_hash_v2_rejects_a_non_canonical_contract_id`: uppercase, empty, over-long and `@`-containing ids.
- `plan_hash_v2_rejects_a_non_canonical_target`: a target with control characters or surrounding
  whitespace never reaches the encoder, because `TargetKey` refuses it.
- `impl_hash_binds_the_entry_script_digest`: the `impl:v1` string always carries both 64-hex fields; changing
  the `ScriptDigest` changes the `ImplHash` and the v2 hash, and there is no manifest-only constructor.
- `changed_entry_script_digest_invalidates_approval`: approve a v2 action, change the skill summary's
  `entry_script_digest`, dispatch → `Rejected`, and no agent request is made.
- **`approval_bound_to_displayed_plan_hash`** **(required)**: a `PlanBinding::Full` of the displayed hash
  matches that plan and nothing else; a prefix of 3 bytes matches, 2 bytes does not (extend the existing
  `plan_binding_*` tests rather than duplicating them).
- `execution_id_for_an_action_equals_the_action_id`: `ExecutionId::from_action(id).as_action() == id`.
- `plan_target_of_incident_takes_the_incidents_host`: `PlanTarget::of_incident(&incident, key).canonical()`
  names `incident.host()` and the key, so a plan cannot be computed for a different host than the incident's.
- **`plan_hash_changes_when_enrolled_agent_key_changes`**: one incident, one host, two `KeyFingerprint`
  values; `compute_v2` differs, and the encoded target text differs only in the `host:` field. The two
  fingerprints are both 64 lowercase hex, so the difference is in the value hashed, not in its length.
- **`target_text_embeds_the_fingerprint`**: `PlanTarget::canonical()` begins with
  `host_target_text(host, &key)` and contains no other fingerprint position; a `PlanTarget` cannot be
  built without one, and `host_target_text` is the only function that writes `host:{uuid}/{fingerprint}`.
- **`key_fingerprint_is_sha256_of_the_public_key`**: `KeyFingerprint::of_public_key(&key)` equals a
  SHA-256 over those 32 bytes, displays as 64 lowercase hex, and rejects a non-hex parse.
- **`host_target_text_is_the_only_formatter`**: `host_target_text(host, &key)` is
  `host:{uuid}/{64 hex}`, and `PlanTarget::canonical()` starts with exactly that string followed by `\n`;
  changing the key changes both, and nothing else in the crate writes the `host:` prefix.
- **`propose_refuses_a_host_without_an_enrolled_key`**: `PlanError::NoEnrolledKey`, and no v2 hash is stored.
- `no_approval_without_human` (existing gate, `tests/gates.rs:67`, extended): only the `human_approval()`
  case can approve at M1.
- `authorization_has_no_public_variants`: the authorization module exposes no `pub enum Authorization`, and
  the only public ways to obtain a value are `human(..)` and `policy(..)`.

### 10.2 Engine (`crates/domovoi-engine`)

- `approve_refuses_a_hash_from_another_plan_v2`: `PlanChanged`, action still `Proposed`, no
  `ActionApproved` event, no audit row.
- `approve_records_the_displayed_hash_and_provenance`: the stored row carries `approved_plan_hash` equal to
  the request's hash, `approval_session_ref`, `approval_client` and `authorization_kind = human`.
- **`approval_bound_to_displayed_plan_hash`** (engine half): an approval carrying the hash of the plan
  displayed succeeds; one carrying a stale hash does not.
- **`changed_args_invalidate_approval`**: approve, change one argument in the store, dispatch → `Rejected`.
- **`dispatch_refuses_after_host_reenrollment`**: approve a v2 action, then replace the host's
  `AgentIdentity` with one whose `public_key` differs (the real `put_agent` path, not a test-only field
  write), dispatch → `Rejected`, no agent request. Replacing it back with the original key does not
  resurrect the approval: the row is terminal.
- **`dispatch_refuses_when_the_host_has_no_live_agent`**: revoke the agent after approval; dispatch cannot
  recompute a target, so it refuses on the stale path and sends nothing.
- **`changed_target_invalidates_approval`** (`04 §8.1` gate name): approve, change the incident's `target`,
  dispatch → `Rejected`, and the reason names the plan.
- **`dispatch_rejects_a_mismatched_plan_hash`**: a dispatch whose recomputed hash differs leaves the action
  `Approved` when the writer refuses and `Rejected` when the state machine refuses — assert the actual
  path, do not assume.
- **`v1_actions_keep_v1_binding_after_upgrade`** **(required)**: seed a pre-upgrade row (`plan_hash` = v1
  over a known plan, `plan_hash_v2` and `approved_plan_hash` `NULL`, `approved_by` set), run the migration,
  read it back, approve and dispatch it; it succeeds under v1, is never recomputed with `compute_v2`, and
  reports `plan_binding = v1`.
- `v1_action_dispatch_recomputes_the_v1_hash`: the existing recomputation test still passes and is named
  here rather than duplicated.
- **`legacy_v1_binding_remains_unchanged`**: the v1 text stays exactly `plan:v1\n{skill}\n{manifest}\n{host}\n`
  plus the canonical args, with **no** key fingerprint and no target identity. Build the same action before
  and after the host is re-enrolled: the v1 hash is byte-identical both times, while the v2 hash of the same
  plan changes. This is the test that proves the fingerprint was added to v2 only.
- `policy_authorization_is_refused_at_m1`: build a real one — `PolicyPermit::issue(..)` with valid
  arguments (test paths are exempt from the call-site scan), wrap it with `Authorization::policy(..)`, and
  assert that both `Approve` and `Dispatch` return `NotYetSupported`, write nothing, and reach no agent.
  Do not assert this with a value the type cannot produce.
- `new_v2_action_binds_the_approval_and_dispatch_with_v2`: for an action created after the upgrade, the
  approve request, `plan_hash_for_approval()`, `approved_plan_hash` and the writer's dispatch comparison all
  use the v2 hash; a stale v2 hash is refused.
- `runskill_request_carries_the_v1_local_check_hash`: the `SkillDispatch.plan_hash` sent to the agent is
  `PlanHash::compute(..)` over skill, manifest, host and canonical args — v1 — even for a v2 action, and the
  agent's existing `run_skill` check accepts it. Assert the two values differ for a v2 action.
- `legacy_v1_action_keeps_v1_for_approval_and_runskill`: a pre-upgrade action is approved, guarded and sent
  with the v1 hash on both paths.

### 10.3 Store (`crates/domovoi-store`)

- `migration_records_approving_rows_as_human`: after the migration, every row with `approved_by` has
  `authorization_kind = 'human'` and every other row has `NULL`.
- `legacy_actions_load_without_a_v2_binding`: a row with the v2 columns `NULL` round-trips and reports the
  v1 family.
- `v2_binding_columns_round_trip`: all five new columns survive insert → read → update.
- `the_hash_columns_reject_a_non_hex_value`: the `CHECK` refuses a 63-character or uppercase value.

### 10.4 Hub (`crates/domovoi-hub`) and web

- `approve_api_accepts_the_current_v2_hash` (200, `ActionDto.plan_hash` equals the stored v2 hex).
- `approve_api_refuses_a_v1_hash_for_a_v2_action` (409, still proposed).
- `action_dto_labels_a_legacy_action_v1_binding` (`plan_binding = "v1"`, `plan_hash_v1` present).
- `action_card_marks_legacy_actions_as_v1_binding` (web): the card shows the v1 label and the tooltip text
  from the shared constant; run it as a page test so axe covers what it leaves on screen.

### 10.5 Architecture (`xtask/src/arch/scan.rs`)

- **`authorization_constructors_are_confined`** (required, the rule covering every constructor):
  `HumanApproval::new`, `HumanApproval::restore`, `PolicyPermit::issue`, `Authorization::human` and
  `Authorization::policy` are each rejected outside their allowed prefix and accepted inside it and in a
  test file.
- `plan_hash_v2_has_one_implementation`: the `plan:v2` literal outside `crates/domovoi-core/src/action/`
  is a violation.
- `authorization_is_not_a_public_enum`: a `pub enum Authorization` outside
  `crates/domovoi-core/src/action/authorization.rs` is a violation.

### 10.6 Acceptance criteria

1. A plan hash v2 exists, is computed by one function, and covers exactly the five inputs of 5.1.
2. The encoding is length-prefixed, has one canonical byte form, and rejects non-canonical input in the one
   constructor that takes raw text; the constructors that take validated types are infallible and say so.
3. An approval is bound to the hash the surface displayed, for both families, and the value is stored as
   `approved_plan_hash`.
4. Dispatch recomputes the hash of the action's own family and never dispatches a mismatched plan.
5. A changed contract, implementation, target subject, **enrolled agent key**, argument or execution
   identity invalidates an approval, each with a named test. The key fingerprint is part of the value
   hashed, not only of the DTO.
6. A host with no enrolled agent key cannot produce a v2 plan (`PlanError::NoEnrolledKey`), and dispatch
   with no live agent refuses on the stale path.
7. Legacy v1 actions keep v1 semantics end to end, are labelled `v1 binding`, and are never recomputed
   under v2 — proven by `legacy_v1_binding_remains_unchanged` across a re-enrolment.
8. `Authorization` is a wrapper with private internals: there are no public variants, the two factories are
   the only construction paths, only `human_approval()` can approve, and the policy case is refused with a
   named error.
9. `PolicyPermit` has the fields of `04 §8.1`, private fields, no `Default`, no `Deserialize`, and its only
   constructor is the restricted `issue` factory. No `PermitIssuer` and no policy evaluation exist.
10. `ExecutionId` exists, equals the `ActionId` at M1, and the id is never rewritten.
11. The architecture scan covers every authorization constructor, the single `plan:v2` implementation and
    the absence of a public `Authorization` enum, with unit tests, and the ADR that the rules change
    requires is in the same PR.
12. The migration exists with the columns of 8.1, chosen by reading the directory, and no permit column is
    added.
13. No wire change, no protocol bump, no second execution path, no reconciliation change, no policy
    evaluation.
14. A v2 action uses the v2 hash for the human approval and the hub's dispatch guard, while the M1
    `run_skill` request carries the v1 local-check hash, and the two are distinct named values at every call
    site.

## 11. Documentation and architecture updates

- **`ARCHITECTURE.md`**: §5.6 states `PlanHash = hash(skill name, manifest hash, host, canonical args)`
  and INV-4 names the plan hash. Add the v2 definition, the two families and the version selector in the
  same place; extend the INV-1 row to say the scan now covers every authorization constructor and that
  `HumanApproval::new` remains hub-auth-only. State plainly that the scan is textual.
- **`docs/book`**: the action/approval page says what an approval binds (v2: contract and version,
  implementation — the manifest hash plus the resolved entry executable's SHA-256 — target subject,
  arguments, execution id) and what it does not (transitive dependencies, interpreters, other files, any
  proof against a compromised hub). No prompt numbers, no build-plan references.
- **ADR** `docs/adr/00NN-plan-hash-v2-and-authorization.md`: the ownership rules, the wrapper-with-private-
  internals decision, the text-scan mechanism and its limits, why no `clippy.toml` entry is used, the
  version-presence scheme, and the two-hash rule of 6.5.
- **`docs/rework/`**: nothing. The planning documents are not edited by an implementation prompt.
- **Generated files**: `web/src/api/openapi.json` and `web/src/api/schema.d.ts` regenerated; `cargo xtask
  docs-config` only if a config value is added (it is not).

## 12. Implementation order and completion requirements

1. **Read the merged prerequisites.** 18's approve-time binding, 20's frozen 1.2 wire, `ScriptDigest` and
   `SkillSummary::entry_script_digest`, and 21's reconciliation and expiry rules, as merged. Confirm the four
   discrepancies in section 4 and report any new one before editing.
2. **List the migrations directory** and write down the number you will take, and the ADR number from
   `docs/adr/`.
3. **Core, types first:** `KeyFingerprint` in `host.rs` and its unit tests.
4. **Core, encoding:** `PlanContract`, `PlanTarget` (with the key), `ImplHash` (consuming prompt 20's
   `ScriptDigest`), the encoder, `compute_v2` and the encoding tests. Nothing else depends on the store or
   the writer, so this is self-contained.
5. **Core, authority types:** `ExecutionId`, `Authorization`, `PolicyPermit`, `ApprovalProvenance`, then the
   `ActionRequest` fields and `approve`/`plan_hash_for_approval`. Update `lib.rs` re-exports.
6. **Store:** the migration, then the row mapping.
7. **Engine:** writer command and approve, the family-specific hash in the executor with the `live_agent`
   key read, the single incident read shared with `freshness`.
8. **Hub:** `Authorization::human(..)` wrapping, provenance, DTO fields, then regenerate the OpenAPI types.
9. **Web:** the binding label and its tooltip; the fixture and the dev demo.
10. **xtask and ADR:** rules, scan rules, scan unit tests, the ADR. Run `cargo xtask arch-check` and
    `cargo xtask arch-check --base origin/overhaul-investigation`.
11. **Tests and checks** for what changed: `cargo fmt --all`; `cargo clippy -p domovoi-core -p
    domovoi-store -p domovoi-engine -p domovoi-hub -p xtask --all-targets -- -D warnings`; `cargo nextest
    run -p domovoi-core`, `-p domovoi-store`, `-p domovoi-engine`, `-p domovoi-hub`, `-p xtask`; for the
    web area `npm run lint`, `npm run typecheck` and the approvals vitest file. Rustdoc for every new
    public item. Do not run the whole workspace suite or `cargo xtask ci`: CI does that on the PR.

Completion requirements:

- Every named test in section 10 exists, with the required names spelled exactly.
- No `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!` or `dbg!` in library code; no new
  `#[allow(...)]`; every new public item documented.
- Non-test source files at or under 500 lines; `plan.rs` and `authorization.rs` are separate modules for
  that reason.
- The migration number and the ADR number were read from the directories, not copied from this brief.
- `PolicyPermit` has private fields, no `Default`, no `Deserialize`, and only the restricted `issue`
  constructor; no `PermitIssuer` and no policy evaluation exist anywhere in the diff.
- The §5.4 contract is implemented as written: both `impl` fields always present, `ScriptDigest` imported
  from prompt 20 rather than defined, and no manifest-only path.
- The v1 local-check hash and the v2 approval hash are separate named values at every call site; no generic
  `plan` variable carries both.
- No `run_capability`, protocol, agent or `hello` change appears in the diff.
- Every deferral in 8.3 and the M1 target-identity limit in 5.5 are stated in the PR description as
  limits, not as done.
- The completion report below is filled in with real command output, not claims.

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
