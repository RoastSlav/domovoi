# 39 — Policy evaluator

| | |
|---|---|
| Branch | `policy-evaluator` |
| Milestone | M7 evaluator foundation |
| Depends on (must be merged) | `24` |
| Parallel with | `26–37` (order-flexible; never concurrent) |
| Produces | `domovoi_engine::policy::{evaluate, Decision, Rule}` plus pure publication-validation and simulator helpers |
| Launch gate | none beyond prompt 24 |
| Suggested model | large |

## 1. Objective

Implement the **policy evaluator** of `04` §7 in `domovoi-engine`, as one pure function plus
supporting types:

1. Which rules **match** a request.
2. Which matching rules are **maximal** (the winners), and why.
3. The **ordinary outcome** of those winners: a single outcome, an intersected set of `Allow`
   constraints, or the `Deny` fallback when winners disagree.
4. **Locked `Deny`** overrides (a hard prohibition no narrower rule can beat), with the ordinary
   result recorded so the UI can show what the lock overrode.
5. The ten **hard limits** applied after rule resolution, in a fixed order, each recorded in the
   trace.
6. The **trace**: every matching rule, the winners, the overridden broader rules, the locked rules,
   the limits applied, the facts consulted, the policy version and the ceiling hash.

**The evaluator is pure, deterministic and does no I/O.** Given the same rules and the same request
it returns a byte-identical `Decision`. It reads no store, no agent link, no registry, no target
inventory, no helper, no clock, no SQLite, no HTTP and no audit log. Time is an explicit
`now: jiff::Timestamp` field on the request; every fact, the ceiling summary, the containment posture
and the autonomy state are **supplied as data**. It decides what authorizes what and nothing else: it
never constructs an approval, a permit or an authorization, never persists and never sends.

## 2. Architectural context

**RW-10, and the four decisions it implements.** Today an action is proposed and a human approves or
denies it; there is no rule layer at all. RW-10 adds one, and the shape of it is fixed by owner
decisions D-05, D-19, D-23 and D-26. In plain terms:

- **D-05 — the most specific rule wins, including over a broader `Deny`.** This deliberately
  replaces the usual "explicit deny always wins" rule for *ordinary* rules. A broad
  `All / container.restart / Any → Deny` is a default posture, and
  `media / container.restart / {plex} → Allow` is an exception the owner wrote on purpose. The
  exception must work, or the owner cannot express "no restarts anywhere except this one container
  on this one host". Specificity is compared per dimension and is a **partial order**; there is no
  numeric score.
  - A strictly more specific matching rule overrides a broader one.
  - At equal scope, `Deny` wins.
  - When winners are incomparable, `Deny` wins if any winner is `Deny`, otherwise
    `RequireApproval` wins over `Allow`.
  - The dangerous case — a narrow `Allow` overriding a broad `Deny` — is a legitimate owner intent,
    but it is exactly what an attacker or a mistaken edit would want, so the trace must name it and
    the UI must highlight it as a dangerous exception. The evaluator emits a warning for it
    (§6.7).
- **D-19 — incomparable conflicting maxima fall back.** `Deny > RequireApproval > Allow`. The value
  is that an owner cannot accidentally widen authority by writing two rules that neither contains
  the other: the disagreement resolves to the safer side. Publication shows every such conflict and
  every equal-scope conflict and requires an acknowledgement (§6.11), so the fallback is a safety
  net, not the normal way to learn about a conflict. Runtime and simulator call the same function.
- **D-23 — `Deny` means "not offered".** `Allow` means unattended: a permit may be issued.
  `RequireApproval` means the owner is offered the step. `Deny` means the step is not offered at
  all and there is no bypass: the owner must change the policy. **The evaluator therefore has three
  authorization outcomes and no fourth meaning for any of them.** `Unavailable` (§6.7) is not an
  authorization outcome: it is the hard-limit outcome that says the evaluator could not establish a
  safety precondition, and nothing may execute on it.
- **D-26 — locked `Deny`.** A `Deny` rule may be marked `locked`. A matching locked `Deny` is a hard
  prohibition: no narrower rule overrides it, whatever its specificity, because the owner declared
  it as an absolute. Several matching locks produce **one** `Deny` that lists every locked rule as
  a reason. Creating, editing, unlocking or removing a locked rule is a step-up action with a
  simulation preview and an audit entry; that is the storage and API side (prompt 40), not this
  module. This module only honours the flag and reports it.

**Why there is no numeric priority.** A number ("priority 10 beats priority 5") is unbounded, so two
edits will eventually pick the same number, and the tie then has to be broken by insertion order or
by an id — an invisible, unstable input that changes the security result. A product partial order
over three dimensions (host, capability, target) has no such ties: either one scope contains the
other, or the pair is genuinely incomparable and falls back to the safer outcome by D-19. The cost is
that the UI must explain specificity rather than display a number; `04` §7.5 already requires that.

**The ceiling and the hard limits.** RW-12/D-03 put the unattended ceiling on the agent: the hub
cannot widen it, and policy may only narrow it. The evaluator therefore does not *enforce* the
ceiling — the agent does — but it must refuse to `Allow` anything the ceiling does not cover, so a
`policy`-labelled request is never sent to a host that will refuse it, and the trace says why. The
same holds for the safety facts, the containment posture and the root-agent check: they are inputs
here, and the authoritative refusals stay where they are enforced (the helper, the agent).

## 3. Repository context

Everything in this section was read in this checkout. Where something is absent, it is absent here
today and a later prompt adds it.

### 3.1 The existing `policy` module must be preserved

`crates/domovoi-engine/src/policy.rs:1-26` is 26 lines: a module doc, `EnginePolicy` at `:8` with
`incident: IncidentPolicy`, `grouping_window: SignedDuration`, `tick_every: SignedDuration`
(`:9-15`), and its `Default` at `:18-26`. It is *the incident manager's tunables*, unrelated to the
capability policy. `crates/domovoi-engine/src/lib.rs:29` declares `pub mod policy;` and `:39`
re-exports `pub use policy::EnginePolicy;`.

`crate::policy::EnginePolicy` is imported by `src/correlate.rs:14`, `src/manager/decide.rs:17`,
`src/manager/dismiss.rs:15` and `src/manager/actor.rs:22`, and `EnginePolicy` is used by name in the
hub (`crates/domovoi-hub/src/runtime/settings.rs:10,14`), `crates/domovoi-hub/examples/dev_hub.rs:442`
and about twenty test modules. **None of that may change.** `EnginePolicy` keeps its name, its
fields, its `Default` and its current `pub use`. This prompt only *adds* items to the module.

### 3.2 Module layout decision

Use a module file plus a submodule directory, declared from `policy.rs`:

```text
crates/domovoi-engine/src/policy.rs          (changed: keeps EnginePolicy, adds `mod` lines and re-exports)
crates/domovoi-engine/src/policy/rule.rs     (new)
crates/domovoi-engine/src/policy/condition.rs(new)
crates/domovoi-engine/src/policy/request.rs  (new)
crates/domovoi-engine/src/policy/specificity.rs (new)
crates/domovoi-engine/src/policy/outcome.rs  (new)
crates/domovoi-engine/src/policy/decision.rs (new)
crates/domovoi-engine/src/policy/evaluate.rs (new)
crates/domovoi-engine/src/policy/publish.rs  (new)
crates/domovoi-engine/src/policy/simulate.rs (new)
crates/domovoi-engine/src/policy/error.rs    (new)
```

That layout is legal in this workspace and already used twice:

- `crates/domovoi-store/src/restore.rs:19-21` declares `mod durable; mod files; mod steps;` and the
  files live in `crates/domovoi-store/src/restore/{durable,files,steps}.rs`.
- `crates/domovoi/src/cmd/upgrade.rs:7-10` declares `mod fetch; mod install; mod package;
  mod target;` and the files live in `crates/domovoi/src/cmd/upgrade/`.

The workspace is `edition = "2024"` with `rust-version = "1.94"` (`Cargo.toml:8-9`), where
`policy.rs` carrying both its own items and `mod` declarations is the ordinary form. Do **not**
convert `policy.rs` into `policy/mod.rs`: that would move `EnginePolicy` in the file tree for no
reason and make the reviews of the incident manager noisier.

`domovoi_engine::policy::{evaluate, Decision, Rule}` must resolve (§13.1). Achieve that by
re-exporting from `policy.rs`:

```rust
pub use decision::Decision;
pub use evaluate::{evaluate, evaluate_shadow};
pub use publish::{validate_publication, PublicationReport};
pub use rule::Rule;
pub use simulate::simulate;
```

`EnginePolicy` stays where it is, in `policy.rs` itself.

### 3.3 The engine crate

`crates/domovoi-engine/Cargo.toml`: `autotests = false` (`:10`), so test files are **not**
discovered automatically; each target is declared. Normal dependencies already include
`domovoi-core` (`:13`), `jiff` (`:16`), `serde` (`:20`), `serde_json` (`:21`), `sha2` (`:22`),
`thiserror` (`:23`) and `uuid` (`:27`). Dev-dependencies already include `domovoi-testkit` (`:31`),
`insta` (`:35`) and `proptest` (`:36`). **Add nothing to either table.**

`ARCHITECTURE.md:53` puts `domovoi-engine` above `domovoi-core` only, and `ARCHITECTURE.md:84` (R2)
forbids the engine `std::fs`, `std::net`, `std::process`, `tokio::net`, `tokio::fs` and
`tokio::process`. This module uses none of them and no `tokio` at all: it is synchronous code.

### 3.4 The engine test layout

Five explicit test targets (`Cargo.toml:41-59`): `tests/engine/main.rs`, `tests/drills/drills.rs`,
`tests/notify/main.rs`, `tests/services/main.rs`, `tests/crash/main.rs`. Each aggregate target is a
`main.rs` listing `#[cfg(test)] mod name;` for each file beside it — for example
`crates/domovoi-engine/tests/engine/main.rs:4-15` lists `actor`, `correlate`, `decide`, `dismiss`,
`harness`, `properties`.

**The policy tests go in the `engine` target**, because that is the target for pure engine decision
functions and it already carries a `proptest` module:
`crates/domovoi-engine/tests/engine/properties.rs:93-94` uses `proptest!` with
`ProptestConfig::with_cases(64)`. No `[[test]]` entry is added.

Three files are added beside it and registered in `tests/engine/main.rs`:

- `crates/domovoi-engine/tests/engine/policy.rs` **(new)**
- `crates/domovoi-engine/tests/engine/policy_publish.rs` **(new)**
- `crates/domovoi-engine/tests/engine/policy_properties.rs` **(new)**

`crates/domovoi-engine/tests/gates.rs` exists (704 lines, included from `tests/services/main.rs:27`
through `#[path = "../gates.rs"]`) and is **not touched**: this prompt adds no invariant that a gate
test can check better than a named behaviour test, and no existing gate is weakened.

### 3.5 `domovoi-core` facts this prompt consumes

| Fact | Location |
|---|---|
| `PlanHash`, `HumanApproval`, `ApprovalChannel`, `PlanBinding` are re-exported from `domovoi_core::action` | `crates/domovoi-core/src/action/mod.rs:7` |
| `ApprovalChannel::Web { typed_confirmation: bool }` is where a typed confirmation is recorded | `crates/domovoi-core/src/action/approval.rs:68-79` |
| `HumanApproval` has no `Default` and no `Deserialize`; `new` is hub-auth-only | `crates/domovoi-core/src/action/approval.rs:88-110` |
| There is **no** `Authorization`, no `PolicyPermit` and no `ExecutionId` in the workspace today (`grep` over `crates/` finds none); prompt 22 creates them | verified absent |
| `Clock` is the only time port; `now(&self) -> Timestamp` | `crates/domovoi-core/src/ports/clock.rs:10-16` |
| `ValidatedArgs::canonical()` emits one `name:byte_len:value\n` line per argument | `crates/domovoi-core/src/skill/validate.rs:107-115` |
| `FactValue { Number(f64), Text(String) }` — a **digest** display value, not a policy fact; it has no `Eq`/`Ord` and must not be reused here | `crates/domovoi-core/src/digest.rs:9-17` |
| `IncidentPolicy` (the incident state machine's policy — unrelated to this module) | `crates/domovoi-core/src/incident/state.rs:53` |
| `Risk { Read, Write, Destructive }` (a manifest risk, not an `EffectClass`) | `crates/domovoi-core/src/skill/manifest.rs:31` |
| `HostId` is a UUIDv7 newtype, re-exported at the crate root | `crates/domovoi-core/src/lib.rs:52-55` |
| `EvidenceRef`, `Evidence`, `EvidenceValue` are *incident evidence*; they are **not** policy facts and must not be reused as such | `crates/domovoi-core/src/triage/report.rs:26`, `crates/domovoi-core/src/signal/evidence.rs` |
| `domovoi_core::in_cidr(ip, cidr)` exists for CIDR membership of one address | `crates/domovoi-core/src/addr.rs:29` |

**`domovoi_core::capability` does not exist in this checkout** — `crates/domovoi-core/src/capability/`
is absent. Prompt 24 creates it, and 24 must be merged before this branch starts, so at
implementation time the module exists. Every type this brief takes from it (`ContractId`,
`ContractVersion`, `ContractCategory`, `FactId`, `TargetName`, `TargetIdentity`, `EffectClass`,
`ConstraintKind`, `ContractMeta`, `ContractIndex`, `CeilingSummary`, `CeilingTargets`,
`IndexError`) is defined in `24` §6 and **must not be redefined here**.

### 3.6 Names this prompt owns

The shared-contract table in `README.md:55` gives prompt 39 only
`domovoi_engine::policy::{evaluate, Decision, Rule}`. There is no owner for `RuleId`,
`EvaluateRequest`, `Decision`'s sub-types or the trace enums, because nothing else consumes them, so
this prompt defines them. Two consequences worth stating up front:

- `RuleId` and `CanonicalArgs` are **new** types introduced here (§6.2, §6.5).
- `ContractMeta` is prompt 24's and is embedded by value in the request; nothing here copies its
  fields into a second type.

### 3.7 Test support

`domovoi-testkit` is a dev-dependency only (`ARCHITECTURE.md:61,87`). This module is pure data in,
data out, so it needs **no fake, no clock and no fixture**. `domovoi-testkit::approvals::{human,
web_typed, chat}` exist (`crates/domovoi-testkit/src/approvals.rs:12,25,37`) but are irrelevant: the
evaluator constructs no approval. No `domovoi-testkit` change is made.

## 4. Exact scope

| File | Change |
|---|---|
| `crates/domovoi-engine/src/policy.rs` | **changed**: keep `EnginePolicy` exactly as it is; add the `mod` declarations, the `pub use` list of §3.2, and a `//!` section explaining the module now holds two unrelated things (incident tunables and the capability policy evaluator) |
| `crates/domovoi-engine/src/policy/error.rs` | **(new)** `PolicyError` |
| `crates/domovoi-engine/src/policy/rule.rs` | **(new)** `Rule`, `RuleId`, `HostScope`, `CapabilityScope`, `TargetScope` |
| `crates/domovoi-engine/src/policy/condition.rs` | **(new)** `Condition`, `TimeOfDay`, `FactState`, `FactStatus`, `Condition::holds` |
| `crates/domovoi-engine/src/policy/outcome.rs` | **(new)** `Outcome`, `OutcomeReason`, `UnavailableReason`, `ConfirmationRequirement`, `HardLimit` |
| `crates/domovoi-engine/src/policy/request.rs` | **(new)** `EvaluateRequest`, `CanonicalArgs`, `ContainmentInput`, `AutonomyState`, `ManagementPath`, `resolve_contract` |
| `crates/domovoi-engine/src/policy/specificity.rs` | **(new)** `ResolvedScope`, `ResolvedCapability`, `scope_contains`, `is_strictly_more_specific`, `overlaps`, `resolve_against`, `resolve_in_index` |
| `crates/domovoi-engine/src/policy/decision.rs` | **(new)** `Decision`, `Constraints`, `Overridden`, `Warning` |
| `crates/domovoi-engine/src/policy/evaluate.rs` | **(new)** `RuleSelection`, `evaluate`, `evaluate_shadow`, `validate_rules`, `run` (private), the seven stages and the ten hard limits |
| `crates/domovoi-engine/src/policy/publish.rs` | **(new)** `validate_publication`, `PublicationReport`, `Finding`, `FindingKind` |
| `crates/domovoi-engine/src/policy/simulate.rs` | **(new)** `simulate` |
| `crates/domovoi-engine/tests/engine/main.rs` | **changed**: three `#[cfg(test)] mod …;` lines, in the alphabetical order the file already uses |
| `crates/domovoi-engine/tests/engine/policy.rs` | **(new)** decision-table, hard-limit and trace tests |
| `crates/domovoi-engine/tests/engine/policy_publish.rs` | **(new)** publication-validation and simulator tests |
| `crates/domovoi-engine/tests/engine/policy_properties.rs` | **(new)** the `proptest` properties (§12.2) |

Every source file stays at or below 500 lines (R8). `evaluate.rs` is the risk: if the seven stages
plus ten limits push it past 500, split the limits into `policy/limits.rs` **(new)** and say so in
the completion report. Do not add a second algorithm.

## 5. Explicit non-goals

- No rule storage, no policy-version table, no publication workflow, no acknowledgement records, no
  step-up authentication, no audit entry. Prompt 40 owns all of them.
- No HTTP endpoint, no SSE event, no web UI, no trace rendering. Prompt 40 and the UI prompts render
  what this module returns.
- No `PolicyPermit` issuance, no `PermitIssuer`, no `Authorization`, no `HumanApproval`. This module
  returns a `Decision`; turning a `Decision` into a permit is prompt 45's job through the
  `PermitIssuer` port.
- No permission evaluation for historical operations beyond `simulate` over caller-supplied
  requests. Loading history is prompt 40's.
- No `ContractIndex` construction or validation (prompt 24), no target resolver (prompt 25), no
  ceiling parsing from `agent.toml` (prompt 25 maps the wire type), no fact store, no attestation
  verification (prompt 47), no CIDR arithmetic (the overlap is supplied as data).
- No change to `EnginePolicy`, `correlate`, `manager`, `actions`, `triage`, `notify`, `cards`,
  `digest`, `arm` — none of them calls the evaluator after this prompt.
- No new crate, no new dependency, no migration, no wire message, no protocol change, no OpenAPI
  change, no `ARCHITECTURE.md` change, no book page.
- No shadow-mode wiring: `evaluate_shadow` exists and is tested, but no runtime path records its
  result until prompt 40.
- No rule-count or evaluation-budget limit. Policies are owner-authored and small; the maximal-
  element computation is quadratic and documented as such.

## 6. Required interfaces and contracts

Mark: **fixed** means names, variants, field names and semantics must match exactly; **convention**
means follow repository style (derives, doc comments, error wording).

### 6.1 Ownership boundary at the top

The evaluator never sees a `ContractIndex`. An `EvaluateRequest` carries the **resolved**
`ContractMeta` of its contract, so an unknown contract cannot be expressed at all: resolution
happens before evaluation (`resolve_contract`, §6.5) and a failure there is a `PolicyError`, never a
default `Allow`. Publication validation is the one place that *does* need the index, because it
compares rules in the abstract rather than against one request (§6.11).

### 6.2 `Rule` and its fields (`rule.rs`) **fixed**

```rust
/// A stable identifier for one rule. Owner-visible, never an evaluation input.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RuleId(String);   // [A-Za-z0-9][A-Za-z0-9_.:-]{0,63}; case-sensitive; compared byte for byte

/// The host dimension. `All` is the whole dimension; `Host(h)` is one host.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostScope { All, Host(HostId) }

/// The capability dimension, widest first.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CapabilityScope {
    Any,
    Effect(EffectClass),
    Category(ContractCategory),
    Contract(ContractId),
}

/// The target dimension. `Exact` is a non-empty set of owner-typed names.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TargetScope { Any, Exact(BTreeSet<TargetName>) }

/// One owner-authored rule. Plain data: no method hides any part of the evaluation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub id: RuleId,
    /// A disabled rule is ignored entirely: it neither matches nor explains.
    pub enabled: bool,
    /// Shadow rules are evaluated by `evaluate_shadow` and never enforced.
    pub shadow: bool,
    /// A hard prohibition. Valid only with `outcome: Deny` (§11).
    pub locked: bool,
    pub host: HostScope,
    pub capability: CapabilityScope,
    pub targets: TargetScope,
    pub conditions: Vec<Condition>,
    pub outcome: Outcome,
    /// Owner text shown in the editor and the trace. Never read by the algorithm.
    pub note: String,
    /// Opaque tag of the policy version this rule belongs to (prompt 40 owns the table).
    pub version_id: Option<Uuid>,
}
```

- Derives: `PartialEq` only on `Rule`; `Eq`, `Hash` and `Ord` are deliberately not derived on
  `Rule`, `Condition`, `Constraints` or `Decision`, and `Rule`s are never sorted by value — only by
  `RuleId`.
- `serde` on `Rule` is for the publication API and snapshots (prompt 40). It is not an input path:
  the evaluator takes `&[Rule]` that the hub already built. Add `Deserialize` anyway so prompt 40
  can round-trip a draft; the security rules of prompt 24 about provider-supplied authority do not
  apply here, because a rule is owner-written, not agent-supplied.
- `Rule` has **no methods**. Comparison and matching live in free functions (§6.8) with unit tests,
  and that is documented as the single implementation.

### 6.3 `Condition` (`condition.rs`) **fixed**

```rust
/// A time of day in UTC, no date, no zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TimeOfDay { pub hour: u8, pub minute: u8 }   // hour 0..=23, minute 0..=59

/// A condition on a request. Conditions never affect specificity; they decide only whether a
/// rule matches (04 §7.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Condition {
    /// The canonical argument `arg` is an integer satisfying `value <op> n`.
    ArgConstraint { arg: String, op: ConstraintKind, value: i64 },
    /// The wall clock of `request.now`, in UTC, is inside `[start, end)`.
    TimeWindow { start: TimeOfDay, end: TimeOfDay },
    /// The fact `fact` is `Known` and equal to `expected`.
    RequiresFact { fact: FactId, expected: bool },
    /// The request's requested rate is at most `max_per_hour`.
    MaxRate { max_per_hour: u32 },
    /// The request's requested TTL is at most `max_ttl`.
    MaxTtl { max_ttl: SignedDuration },
}
```

Exact match semantics, one clause each:

| Condition | Matches when | Does not match when |
|---|---|---|
| `ArgConstraint` | `args.get(arg)` parses as `i64` and `ConstraintKind::satisfied(n, value)` holds: `IntAtLeast` → `n >= value`, `IntAtMost` → `n <= value` | the argument is absent, empty, non-numeric, out of `i64` range, or the canonical line is malformed |
| `TimeWindow` | `start < end` and `start <= t < end`; or `start > end` (wrap) and `t >= start || t < end` | `start == end` (an empty window; `validate_rules` rejects it as a dead rule) |
| `RequiresFact` | `facts.get(fact) == Some(&FactState::Known(*expected))` | absent, `Unknown`, `Stale`, `Disputed`, or `Known` with the other value |
| `MaxRate` | `requested_rate_per_hour` is `Some(r)` with `r <= max_per_hour`, **or** `None` | `Some(r)` with `r > max_per_hour` |
| `MaxTtl` | `requested_ttl` is `Some(t)` with `t <= max_ttl`, **or** `None` | `Some(t)` with `t > max_ttl` |

- **`ConstraintKind` is prompt 24's** (`IntAtLeast | IntAtMost`, `24` §6.3). Do not define a second
  operator enum; the ceiling reads its bound through the same two variants (`24` §6.5), so the
  operator means the same thing in both places.
- `t` is `request.now` formatted as `HH:MM` **in UTC only**: `now.to_zoned(TimeZone::UTC)` and read
  the hour and minute. No DST, no locale, no configured zone, no `Clock`. Wrap-around
  (`22:00`–`06:00`) is supported by the second clause; a window is `[start, end)`, so `22:00`–`06:00`
  does not include `06:00`.
- `MaxRate`/`MaxTtl` with `None` on the request match: nothing is being requested that could exceed
  the bound. They still contribute a constraint on an `Allow` (§6.9 stage 5), so the permit is
  capped at the rule's bound either way.
- `ArgConstraint` compares the **canonical argument string** only. A non-numeric value never
  satisfies an integer constraint, so the rule does not match and evaluation falls through to
  broader rules or the default. It never falls through to a more permissive `Allow`.

### 6.4 `FactState` (`condition.rs`) **fixed**

```rust
/// What the hub knows about one safety-critical fact, as supplied by the caller.
///
/// A policy fact is a boolean predicate about the hub's world (for example
/// `host.not_in_maintenance`). It is deliberately not the digest's `FactValue`
/// (`crates/domovoi-core/src/digest.rs:9-17`), which is a display value for a measurement, has no
/// `Eq`/`Ord`, and is not a safety predicate. Do not unify them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FactState {
    /// Known, with its value.
    Known(bool),
    /// Never observed, or unreadable.
    Unknown,
    /// Last observation is older than the fact's freshness bound.
    Stale,
    /// Two sources disagree.
    Disputed,
}

/// The same four cases with the value dropped, for the trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactStatus { Known, Unknown, Stale, Disputed }
```

- Only `Known(expected)` matches a `RequiresFact` (decision-table row 15: "unknown ≠ true"). This is
  the whole point: an absent fact must never read as the safe value.
- `Decision.facts_used` records `FactStatus`, not the `Known` value.
- `FactState`/`FactStatus` are **not** `EvidenceValue`, `Evidence`, `DigestFact` or `digest::FactValue`.
  Those are incident evidence and digest measurements. A policy fact is a hub-known boolean predicate
  with a freshness rule, and the two must not be unified. If a later milestone needs non-boolean facts,
  that is a change to this type and to the trace schema, owned by whoever needs it — not something to
  pre-empt here.

### 6.5 The request (`request.rs`) **fixed**

```rust
/// Canonical arguments, as produced by the plan builder: one `name:byte_len:value\n` line per
/// argument, in the same grammar as `domovoi_core::skill::ValidatedArgs::canonical()`
/// (`crates/domovoi-core/src/skill/validate.rs:107-115`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CanonicalArgs(String);

impl CanonicalArgs {
    /// Holds the text as given. Never fails: an unparsable line simply yields no argument.
    pub fn from_canonical(text: impl Into<String>) -> Self;
    pub fn canonical(&self) -> &str;
    /// The value of `name`, or `None` when the name is absent or its line is malformed.
    pub fn get(&self, name: &str) -> Option<&str>;
}

/// One management path that the verified safety set does not cover.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ManagementPath(String);          // 1..=310 chars, printable ASCII, no spaces

/// The containment posture, computed elsewhere and supplied as data.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ContainmentInput {
    /// A verified, fresh helper attestation exists (key pinned, signature verified, ≤ 5 min old).
    pub attested: bool,
    /// Discovered management paths absent from the verified effective set (D-28).
    pub uncovered_paths: Vec<ManagementPath>,
    /// A previously owner-confirmed path is older than its confirmation bound.
    pub stale_confirmation: bool,
    /// The containment target overlaps the verified effective set (must deny, RW-37).
    pub target_overlaps_verified_set: bool,
    /// The helper reported that it cannot compute its safety set at all.
    pub helper_incomplete: bool,
}

impl ContainmentInput {
    /// The value for a request whose contract is not `Contain`.
    pub fn not_applicable() -> Self;        // every field false/empty
    /// Whether this value was populated at all.
    pub fn is_empty(&self) -> bool;
}

/// Autonomy and circuit-breaker state, supplied as data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AutonomyState {
    /// The global or host autonomy pause is in effect.
    pub paused: bool,
    /// The matched rule(s) are circuit-broken after repeated failures.
    pub circuit_broken: bool,
}

/// Everything the evaluator needs. Built by the caller from ports and repositories; nothing here
/// is fetched, and nothing is looked up during evaluation.
#[derive(Debug, Clone, PartialEq)]
pub struct EvaluateRequest {
    pub host: HostId,
    /// The resolved contract (prompt 24). Absent contract = no request (§6.1).
    pub contract: ContractMeta,
    pub contract_version: ContractVersion,
    /// The owner-typed name the request names, for rule matching.
    pub target: TargetName,
    /// Its canonical identity (RW-34), for pinning and permit binding.
    pub target_identity: TargetIdentity,
    pub args: CanonicalArgs,
    /// What the request asks for; `None` when the capability has no rate/TTL notion.
    pub requested_rate_per_hour: Option<u32>,
    pub requested_ttl: Option<SignedDuration>,
    /// The pinned subjects for this host and operation (RW-23/D-07).
    pub pinned: BTreeSet<TargetIdentity>,
    /// Resolutions of the target names the matching rules may name. A name that failed to
    /// resolve is absent, which contributes nothing (fail closed).
    pub rule_targets: BTreeMap<TargetName, TargetIdentity>,
    /// The host's last reported ceiling summary (prompt 24's type; prompt 25 maps the wire).
    pub ceiling: Option<CeilingSummary>,
    /// The ceiling hash the caller expects. `Some` on history replay, from the recorded value.
    pub expected_ceiling_hash: Option<String>,
    /// Every safety fact the hub knows for this host. Absent facts are treated as `Unknown`.
    pub facts: BTreeMap<FactId, FactState>,
    /// Whether the host's agent runs as root (`hello.runs_as_root`, D-29 not in effect).
    pub runs_as_root: bool,
    pub containment: ContainmentInput,
    pub autonomy: AutonomyState,
    /// The evaluation instant, UTC. The only time input; there is no `Clock` here.
    pub now: Timestamp,
    /// Opaque tag of the policy version being evaluated (prompt 40 owns the table).
    pub policy_version: Option<Uuid>,
}
```

Three constructor-side helpers, all pure:

```rust
/// The `ContractMeta` of `id`, checked against the index. `Err(UnknownContract)` when the index has
/// no such contract, `Err(CategoryNotInIndex)` when it has the contract but no effect for its
/// category. Callers build `EvaluateRequest` with the `Ok` value, so an unknown contract can never
/// reach `evaluate`.
pub fn resolve_contract(index: &ContractIndex, id: &ContractId) -> Result<ContractMeta, PolicyError>;

/// Whether a rule's target names can be resolved for this request. The caller builds
/// `EvaluateRequest.rule_targets` from this, so the evaluator never resolves anything.
pub fn names_to_resolve(rules: &[Rule]) -> BTreeSet<TargetName>;
```

`names_to_resolve` returns the union of every `TargetScope::Exact` set over the given rules, so the
caller can resolve exactly the names evaluation may need. It is the *only* coupling between the
evaluator and the target resolver, and it is one-directional.

Field notes that must survive implementation:

- `contract` is by value (`ContractMeta` is `Debug, Clone, PartialEq, Eq`, `24` §6.4). Its
  `safety_facts` and `constrainable_args` are read by limits 3 and 4.
- `contract_version` is used by **no** stage. It is carried so the trace names what was evaluated
  and prompt 45 can bind it into a permit. Do not invent version-based specificity.
- Rule matching uses `request.target` (`TargetName`, exact and case-sensitive, `24` §6.1). Pinning
  and permit binding use `request.target_identity`. Different questions, never interchanged.
- `CeilingSummary.enabled` (`24` §6.5) is `[unattended].enabled`; `false` covers nothing, exactly
  like `None` (hard limit 4).
- `expected_ceiling_hash` lets a caller demand the same ceiling the decision was taken against
  (history replay, `04` §7.6): `Some(h)` with a summary whose `hash == Some(h)` passes, every other
  combination fails coverage. Keeping it here is what makes `simulator_matches_live_evaluator` cover
  the hash case.
- `CeilingSummary.max_ttl` is `Option<SignedDuration>` in `24` §6.5 unless the documented
  `max_ttl_secs: Option<u64>` fallback was taken. **Use the merged type as it is**; if the fallback
  was taken, convert once in the coverage check (§6.10 limit 4) and say so in the report.

### 6.6 `Outcome`, `Unavailable`, typed confirmation (`outcome.rs`) **fixed**

```rust
/// What the policy says about one request. Three authorization outcomes (D-23) plus the
/// hard-limit outcome, which authorizes nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "reason", rename_all = "snake_case")]
pub enum Outcome {
    /// Unattended; a permit may be issued.
    Allow,
    /// Offered to the owner.
    RequireApproval,
    /// Not offered. The owner must change the policy (D-23). No bypass.
    Deny,
    /// The evaluator could not establish a safety precondition. Nothing may execute and nothing is
    /// offered: this is not an authorization outcome.
    Unavailable(UnavailableReason),
}
```

- Declaration order is the severity order used by the hard limits: `Allow < RequireApproval <
  Unavailable < Deny`. `Outcome`'s derived `Ord` therefore already sorts that way; do not reorder the
  variants.
- `D-23`'s three outcomes keep exactly their meanings. `Unavailable` is **not** a fourth
  authorization: `Outcome::Unavailable(_)` is never converted into a permit, never shown as
  approvable, and there is no rule outcome that produces it.

```rust
/// Why a hard limit produced `Unavailable`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnavailableReason {
    /// 04 §7.3 hard limit 8: the helper cannot compute its effective safety set.
    SafetyUnknown,
}

/// A malformed rule set or request is `Err(PolicyError)` from `evaluate`, never an outcome: it has no
/// place in the trace and must not be mistaken for a decision.

/// Whether the owner must type a confirmation phrase to approve the step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmationRequirement { None, Typed }

/// Why the ordinary resolution produced its outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeReason {
    /// No rule matched; the effect-class default applied (stage 3).
    Default,
    /// Every winner agreed on one outcome (stage 5). The name is 04 §7.5's.
    SingleWinner,
    /// Equal-scope winners disagreed; `Deny` won (D-05).
    EqualScopeDeny,
    /// Incomparable winners disagreed (D-19).
    IncomparableFallback,
    /// A matching locked `Deny` overrode the ordinary resolution (D-26).
    LockedDeny,
}
```

**Exactly five reasons, and no sixth.** `04` §7.5 fixes this vocabulary and stage 5's intersection
cannot fail, so there is no state these five do not describe:

- Every rule in `M` matched, and a rule matches only when its target scope contains
  `request.target`. So every winning `Exact(S)` has `request.target ∈ S`, and the intersection of the
  winners' `Exact` sets therefore **always contains `request.target`** and is never empty. `Any`
  winners are represented as `targets: None`, which is not an intersection at all.
- An empty `Exact(..)` set is refused by `validate_rules` (`PolicyError::EmptyTargetSet`), so it can
  never reach the matcher.

Do not add an "empty intersection" reason, do not withdraw an `Allow` for that reason, and do not
special-case it in the UI: the state does not exist. `allow_intersection_always_contains_the_requests_target`
is the property test that keeps this true, and `empty_target_intersection_is_unrepresentable` proves an
`Exact(∅)` rule is refused before evaluation.

**Typed confirmation.** `Decision.typed_confirmation` is `Typed` exactly when the outcome is
`RequireApproval` and either (a) the contract's effect is `Destructive` or (b) hard limit 5 or 6
fired; `None` otherwise, including for `Allow`, `Deny` and `Unavailable`. It is a *requirement
carried in the trace*, not an approval: the hub enforces it at the approve endpoint through
`ApprovalChannel::Web { typed_confirmation }` (`crates/domovoi-core/src/action/approval.rs:68-79`).

### 6.7 `Decision` and the trace (`decision.rs`) **fixed**

```rust
/// The constraints an `Allow` carries forward to the permit.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Constraints {
    /// The lowest `MaxRate` bound among the winners, further reduced by the requested rate.
    pub max_per_hour: Option<u32>,
    /// The lowest `MaxTtl` bound among the winners, further reduced by the requested TTL.
    pub max_ttl: Option<SignedDuration>,
    /// The intersection of the winners' `Exact` sets; `None` when any winner is `Any`.
    pub targets: Option<BTreeSet<TargetName>>,
    /// The conjunction of the winners' conditions, in the winners' `RuleId` order then in each
    /// rule's own order.
    pub conditions: Vec<Condition>,
}

/// A broader rule that a more specific winner overrode.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Overridden {
    /// The rule that lost, because a more specific rule matched.
    pub broader: RuleId,
    /// The rule that won.
    pub by: RuleId,
}

/// Something the UI must highlight.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Warning {
    /// A specific `Allow` overrode a broader `Deny`: a deliberate exception that an editor mistake
    /// or a compromised session would also produce. Rendered as dangerous (04 §7.5).
    DangerousException { allow: RuleId, deny: RuleId },
    /// The helper's safety set could not be verified (hard limit 5).
    SafetySetUnverified,
    /// These management paths are discovered but not protected (hard limit 6).
    ManagementPathsUnprotected { paths: Vec<ManagementPath> },
}

/// The complete, deterministic trace of one evaluation (04 §7.5).
#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    pub outcome: Outcome,
    /// How the resolution concluded. Always present: a malformed input is an `Err`, not a `Decision`.
    pub reason: OutcomeReason,
    /// Every matching enforced rule, sorted by id. Shadow matches are in the shadow decision.
    pub matched: Vec<RuleId>,
    /// The maximal matching rules, sorted by id.
    pub winners: Vec<RuleId>,
    /// Every matching locked rule, sorted by id. Non-empty implies `LockedDeny`.
    pub locked: Vec<RuleId>,
    /// Broader rules beaten by a more specific winner with a different outcome, sorted by
    /// `(broader, by)`.
    pub overridden: Vec<Overridden>,
    /// The ordinary resolution (stages 3-6) with no hard limits, present exactly when the final
    /// outcome differs from it. Its own `would_have_been` is always `None`.
    pub would_have_been: Option<Box<Decision>>,
    pub constraints: Constraints,
    /// The hard limits that applied, in the fixed order of §6.10.
    pub hard_limits_applied: Vec<HardLimit>,
    /// Every fact the resolution consulted, sorted by id: the contract's `safety_facts` and every
    /// `RequiresFact` of a matching rule.
    pub facts_used: Vec<(FactId, FactStatus)>,
    pub typed_confirmation: ConfirmationRequirement,
    /// Stable, sorted warnings.
    pub warnings: Vec<Warning>,
    pub policy_version: Option<Uuid>,
    /// The ceiling summary's hash, echoed for change detection. Never interpreted here.
    pub ceiling_hash: Option<String>,
}
```

Determinism rules, all of which the property tests check:

1. `matched`, `winners`, `locked` are sorted by `RuleId` (they are `BTreeMap` keys internally);
   `overridden` by `(broader, by)`; `facts_used` by `FactId`, deduplicated; `warnings` by their
   derived `Ord`.
2. `hard_limits_applied` is in the declaration order of `HardLimit`, never in discovery order.
3. `constraints.conditions` follows winner order, then each rule's own order.
4. No field depends on the order `rules` was passed in, on a `RuleId` *deciding* anything, or on
   wall-clock, allocation address or hash-map iteration.

`Decision` has no `Eq` by design; compare with `PartialEq`, as the property tests do.

### 6.8 Specificity helpers (`specificity.rs`) **fixed**, `pub(crate)`

```rust
/// A rule's scope, normalised so the three dimensions can be compared without any lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedScope {
    pub(crate) host: HostScope,
    pub(crate) capability: ResolvedCapability,
    pub(crate) targets: TargetScope,
}

/// The capability dimension with its category and effect filled in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ResolvedCapability {
    Any,
    Effect(EffectClass),
    Category { category: ContractCategory, effect: EffectClass },
    Contract { id: ContractId, category: ContractCategory, effect: EffectClass },
}

/// Whether `outer` contains `inner` on every dimension, i.e. every request `inner` matches
/// `outer` matches too.
pub(crate) fn scope_contains(outer: &ResolvedScope, inner: &ResolvedScope) -> bool;

/// `a` is strictly more specific than `b`: `a` is inside `b` on every dimension and properly
/// inside it on at least one (04 §7.2).
pub(crate) fn is_strictly_more_specific(a: &ResolvedScope, b: &ResolvedScope) -> bool;

/// Two scopes intersect on every dimension.
pub(crate) fn overlaps(a: &ResolvedScope, b: &ResolvedScope) -> bool;

/// Normalises a rule against the contract of one request. `None` when the rule's scope does not
/// contain the request.
pub(crate) fn resolve_against(rule: &Rule, meta: &ContractMeta) -> Option<ResolvedScope>;

/// Normalises a rule in the abstract, using the index for `Contract`/`Category` facts. `None` when
/// the rule names a contract or category the index does not know.
pub(crate) fn resolve_in_index(
    rule: &Rule,
    index: &ContractIndex,
) -> Result<ResolvedScope, PolicyError>;
```

**Host dimension.** `All` contains everything. `Host(h)` contains `Host(h)` only. Nothing else.

**Capability dimension.** Containment `outer ⊇ inner`:

| `outer` \ `inner` | `Any` | `Effect(e)` | `Category{c,f}` | `Contract{id,c,f}` |
|---|---|---|---|---|
| `Any` | yes | yes | yes | yes |
| `Effect(E)` | no | `e == E` | `f == E` | `f == E` |
| `Category{C,F}` | no | no | `c == C` | `c == C` |
| `Contract{ID,..}` | no | no | no | `id == ID` |

`Contract(c) ⊂ Category(category(c)) ⊂ Effect(effect(c)) ⊂ Any` is `24` §6.2/§6.4's relation read
through `ContractIndex::meta` and `effect_of_category`. A category belongs to exactly one effect
class because `ContractIndex::new` rejects `IndexError::CategorySpansEffects` (`24` §6.4), and that
is exactly why the order is well defined.

**Target dimension.** `Any` contains everything. `Exact(S1)` contains `Exact(S2)` iff `S2 ⊆ S1`.
`Exact(_)` never contains `Any`. `Exact(S1)` and `Exact(S2)` overlap iff `S1 ∩ S2 ≠ ∅`; otherwise
they are disjoint and neither contains the other.

**Strict specificity** is `scope_contains(b, a) && !scope_contains(a, b)`, dimension-wise on the
tuple. Conditions are never part of it (`04` §7.2): two rules that differ only in conditions are
equally specific, and at most one of them matches a given request anyway.

**Two facts that make the implementation small and safe:**

- *Resolution lemma.* In the set of rules that **matched** one request, every `Effect(e)` has
  `e == request.contract.effect`, every `Category(c)` has `c == request.contract.category`, and every
  `Contract(id)` has `id == request.contract.id`. So on a matched set the capability dimension is a
  total order `Contract < Category < Effect < Any`, and `resolve_against` can fill in the category
  and effect from the request instead of from an index. `evaluate` therefore needs **no**
  `ContractIndex` argument, which is why its signature is exactly the one this brief fixes.
  Test: `capability_rank_is_a_total_order_on_matches`.
- *No second implementation.* `validate_publication` compares rules with no request at all, so it
  uses `resolve_in_index`; the three comparison functions are shared. There is one definition of
  "more specific" in the crate, with unit tests, and `publish.rs` calls it.

### 6.9 The algorithm (`evaluate.rs`) **fixed**

One implementation, `fn run(rules: &[Rule], request: &EvaluateRequest, selection: RuleSelection)
-> Result<Decision, PolicyError>`, called by both public entry points:

```rust
/// Which rules take part: the enforced ones, or the shadow ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleSelection { Enforced, Shadow }

/// The decision the enforced policy takes on this request. Pure and deterministic.
///
/// `Err` means the rule set or the request is malformed: fail closed by refusing to produce a
/// decision, never by producing one the caller might act on.
pub fn evaluate(rules: &[Rule], request: &EvaluateRequest) -> Result<Decision, PolicyError>;

/// The decision the shadow rules take on the same request, through the same algorithm. Its
/// result is recorded and never enforced.
pub fn evaluate_shadow(rules: &[Rule], request: &EvaluateRequest) -> Result<Decision, PolicyError>;

/// Validates a rule set. Cheap; called by `run` itself, so an invalid set can never be evaluated.
pub fn validate_rules(rules: &[Rule]) -> Result<(), PolicyError>;
```

**Why two entry points rather than a `shadow: bool` parameter:** a bare `true` at a call site is
unreadable and invertible, and inverting it would *enforce a shadow rule* — the exact failure RW-10
forbids. `evaluate_shadow(..)` states its intent at the call site and cannot be mistyped into
`evaluate(..)` by a boolean flip. One algorithm still: both are three lines around `run`.

`run` is, in order:

**Stage 0 — guard.** `validate_rules(rules)` and the request checks of §6.13. On failure return
`Err(PolicyError::…)`. Never `Allow`, never a partial `Decision`, never a panic. A caller that ignores
the error has nothing to act on.

**Stage 1 — match.** Partition the rules into the selected set:
`rule.enabled && (selection == Shadow) == rule.shadow`. Keep those where
`resolve_against(rule, &request.contract)` is `Some` **and** every condition in `rule.conditions`
holds (§6.3). The result is `M`, a `BTreeMap<RuleId, (&Rule, ResolvedScope)>`. Record `matched`
(sorted ids), and `facts_used` (the contract's `safety_facts` plus every `RequiresFact` fact of a
matched rule, all with their `FactStatus`).

**Stage 2 — locked `Deny` (D-26).** Let `L` be the rules in `M` with `locked && outcome == Deny`
(validation already guarantees `locked` implies `Deny`).
- If `L` is non-empty: the ordinary resolution of stages 3–6 is still computed and stored as
  `would_have_been`; the result is `outcome = Deny`, `reason = LockedDeny`, `locked` = every id in
  `L` sorted, `winners = []`, `overridden` = `(winner, locked)` for every ordinary winner and every
  locked rule, sorted. Then stage 7 runs in "record only" mode: no limit may weaken a `Deny`, so
  `hard_limits_applied` stays empty. Return.
- Otherwise continue.

**Stage 3 — default.** If `M` is empty, the outcome is the effect-class default and `reason =
Default`:

| Effect class | Default outcome | Typed confirmation |
|---|---|---|
| `Read` | `Allow` (budgeted) | `None` |
| `Mutate` | `RequireApproval` | `None` |
| `Contain` | `RequireApproval` | `None` |
| `Destructive` | `RequireApproval` | `Typed` |

`constraints` is empty. Continue to stage 7 (the default `Allow` for `Read` still has to survive the
hard limits — target pinning, facts, ceiling, autonomy, root).

**Stage 4 — winners.** `W` = the maximal elements of `M`: every `(a, b)` in `M` with
`is_strictly_more_specific(b.scope, a.scope)` removes `a`. Two rules with equal resolved scopes are
both maximal. Record `winners` (sorted ids) and `overridden`: `(broader, by)` for every `broader ∈ M`
that some `by ∈ W` is strictly more specific than, **where their `outcome`s differ**. Equal scopes
never appear as overrides.

**Stage 5 — one outcome.** If every rule in `W` has the same `outcome`, that is the result, with
`reason = SingleWinner`.
- For `Allow`: combine constraints by intersection — `max_per_hour` = the minimum of the winners'
  `MaxRate` bounds and (when present) the requested rate; `max_ttl` = the minimum of the winners'
  `MaxTtl` bounds and (when present) the requested TTL; `targets` = the intersection of the winners'
  `Exact` sets, or `None` when any winner is `Any`; `conditions` = the conjunction of the winners'
  conditions in winner order.
- **The target intersection is never empty.** Every winner matched, so every winning `Exact(S)` has
  `request.target ∈ S`; the intersection therefore contains `request.target` by construction. Do not
  add an empty-intersection branch, a downgrade, or a sixth reason: there is nothing to handle. The
  property test `allow_intersection_always_contains_the_requests_target` pins the invariant, and
  `empty_target_intersection_is_unrepresentable` pins that `Exact(∅)` is refused by validation.
- For `Deny`: `constraints` empty. `typed_confirmation = None` (D-23: a `Deny` is not offered, so it
  has nothing to confirm).
- For `RequireApproval`: `constraints` empty; `typed_confirmation` per §6.6.

**Stage 6 — mixed outcomes.** Otherwise:
- If every winner has the **same resolved scope** (`a.scope == b.scope` for all pairs) the result is
  `Deny`, `reason = EqualScopeDeny` (D-05), and `typed_confirmation = None`.
- Otherwise the winners are incomparable: `Deny` if any winner is `Deny` (`reason =
  IncomparableFallback`), else `RequireApproval` (`reason = IncomparableFallback`) (D-19).
- `constraints` empty; `typed_confirmation` per §6.6.

**Stage 7 — hard limits.** Apply the ten limits of §6.10 in order. Each may only move the outcome
*up* the severity order `Allow < RequireApproval < Unavailable < Deny`, never to `Allow`, never away
from `Deny`. Record each applied limit in `hard_limits_applied`. Set `would_have_been` when the
outcome differs from the stage 3–6 result.

**Order independence.** Stages 1–6 are set operations over `M`; no stage reads the input order of
`rules` and no stage uses a `RuleId` to break a tie. Ids are used only to *sort* the trace. This is
property-tested (`evaluation_is_order_independent`).

### 6.10 The ten hard limits **fixed**

Applied in this order, after stage 6, to the outcome produced by stages 3–6. "Applies" means the
precondition holds **and** the outcome is not already at least as severe. Each applied limit is
recorded by its enum variant, which is declared in this order:

```rust
/// The hard limits of 04 §7.3, in application order. Declaration order is the trace order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HardLimit {
    DestructiveNeverAllowed,
    TargetNotPinned,
    SafetyFactUnknown,
    CeilingDoesNotCover,
    ContainmentUnverified,
    ManagementPathUncovered,
    ContainmentTargetExcluded,
    HelperSafetyIncomplete,
    AutonomyInactive,
    AgentRunsAsRoot,
}
```

| # | Variant | Precondition | Effect |
|---|---|---|---|
| 1 | `DestructiveNeverAllowed` | outcome is `Allow` and `contract.effect == Destructive` | `RequireApproval`, `Typed` (D-06) |
| 2 | `TargetNotPinned` | outcome is `Allow` and `request.target_identity` is not in `pinned` and not equal to any `rule_targets[name]` for a name in a winning `Allow` rule's `Exact` set | `RequireApproval` (D-07) |
| 3 | `SafetyFactUnknown` | outcome is `Allow` and any `fact ∈ contract.safety_facts` has a state other than `Known(_)` (absent counts as `Unknown`) | `RequireApproval` |
| 4 | `CeilingDoesNotCover` | outcome is `Allow` and the ceiling does not cover contract + target + args + rate + TTL, or `ceiling` is `None`, or `enabled == false`, or `expected_ceiling_hash` disagrees | `RequireApproval` |
| 5 | `ContainmentUnverified` | `contract.effect == Contain` and `!containment.attested` | `RequireApproval` + `Typed`, warning `SafetySetUnverified` (RW-37) |
| 6 | `ManagementPathUncovered` | `contract.effect == Contain` and (`containment.uncovered_paths` is non-empty or `containment.stale_confirmation`) | `RequireApproval` + `Typed`, warning `ManagementPathsUnprotected { paths }` (D-28) |
| 7 | `ContainmentTargetExcluded` | `containment.target_overlaps_verified_set` | `Deny`; `reason` stays the ordinary resolution's (`reason` describes stages 3–6), and `hard_limits_applied` records this limit — holds for humans as well |
| 8 | `HelperSafetyIncomplete` | `containment.helper_incomplete` | `Unavailable(SafetyUnknown)` |
| 9 | `AutonomyInactive` | `autonomy.paused \|\| autonomy.circuit_broken` | `RequireApproval` |
| 10 | `AgentRunsAsRoot` | outcome is `Allow` and `request.runs_as_root` | `RequireApproval` (D-29 not in effect) |

Details that must be implemented exactly:

- **Limit 2 target set.** The union is the pinned identities plus the resolutions, for *this*
  request, of the target names named by the winning `Allow` rules (`request.rule_targets`). A name
  with no resolution contributes nothing, so an unresolvable rule target cannot widen anything. With
  no winners (stage 3 default) the union is `pinned` alone.
- **Limit 4 coverage.** One helper, `fn ceiling_covers(ceiling: Option<&CeilingSummary>,
  expected_hash: Option<&str>, request: &EvaluateRequest) -> bool`, which requires all of:
  - a summary exists and `enabled == true`;
  - `expected_ceiling_hash` agrees: `None` expected accepts any summary hash; `Some(h)` requires
    `summary.hash == Some(h)`. A summary with no hash and a caller that expects one **fails**
    (fail closed);
  - some `CeilingEntry` has `contract == request.contract.id` (exact id, no wildcards, `24` §6.5);
  - the entry's `CeilingTargets` covers `request.target`: `Any`, or `Names` containing the name;
  - `request.requested_rate_per_hour` is `Some(r)` with `r <= entry.max_per_hour`, or `None`;
  - `request.requested_ttl` is `Some(t)` with `t <= entry.max_ttl` when the entry has one, or the
    entry has none, or the request has none;
  - every `entry.args` key that the contract declares as constrainable (`contract.constrainable_args`,
    `24` §6.3) holds for the request's value: `IntAtLeast` → `v >= bound`, `IntAtMost` → `v <= bound`.
    A declared key the entry does not bound is not a constraint; an entry key the contract does not
    declare is ignored (the agent rejects such a config at load). A non-numeric argument value fails
    the coverage.
- **Limit 7 and 8 only for `Contain`.** They read `request.containment`. A request whose contract is
  not `Contain` must carry `ContainmentInput::not_applicable()`; a populated one is
  `PolicyError::ContainmentOnNonContain` at stage 0.
- **Limits 5 and 6 add `Typed` even when the outcome is already `RequireApproval`** (stage 3's
  default for `Contain`, or D-19's fallback), and are still recorded: the trace must show that the
  unattended path was blocked by the safety posture rather than by the rules.
- **Limit 9 records when it fires on an outcome that is already `RequireApproval`**, and never
  applies to `Deny` or `Unavailable`.
- **No limit ever produces `Allow`**, and no limit converts `Deny` into anything else; limit 7 is
  the only one that can produce `Deny` from `Allow`/`RequireApproval`, and it wins over limit 8
  because it comes first.

### 6.11 Publication validation (`publish.rs`) **fixed**

```rust
/// One finding of a publication review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub kind: FindingKind,
    /// The two rules involved. Equal to each other only for the ceiling note.
    pub first: RuleId,
    pub second: RuleId,
    /// What the pair resolves to at runtime, when that is meaningful.
    pub resolution: Option<Outcome>,
    /// Whether either rule is a shadow rule. A shadow pair never needs an acknowledgement.
    pub shadow: bool,
    /// Owner-facing, one sentence, no prompt numbers, no ids beyond the two above.
    pub note: String,
    /// Whether publishing must wait for the owner to acknowledge this finding.
    pub requires_acknowledgement: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FindingKind {
    /// Equal resolved scope, differing outcomes: resolves to `Deny` (D-05).
    EqualScopeConflict,
    /// Overlapping incomparable rules, differing outcomes: resolves by D-19.
    IncomparableConflict,
    /// An enabled rule that a locked `Deny` makes ineffective (D-26).
    IneffectiveUnderLock,
    /// An `Allow` naming a target no host's ceiling covers. Informational.
    TargetOutsideEveryCeiling,
}

/// Everything a publication review found, worst first.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PublicationReport { pub findings: Vec<Finding> }

impl PublicationReport {
    /// Whether publishing must wait for acknowledgements.
    pub fn is_blocked(&self) -> bool;
    /// The findings that block, for the acknowledgement screen.
    pub fn blocking(&self) -> impl Iterator<Item = &Finding>;
}

/// Reviews a draft rule set before publication, using the same specificity helpers as runtime.
pub fn validate_publication(
    rules: &[Rule],
    index: &ContractIndex,
    ceilings: &BTreeMap<HostId, CeilingSummary>,
) -> Result<PublicationReport, PolicyError>;
```

- **Errors first.** `validate_rules(rules)` must pass. Then every rule must resolve in the index;
  `resolve_in_index` returns `Err(UnknownContract)` or `Err(CategoryNotInIndex)` for a rule naming a
  contract or category the index does not know. Publication refuses; it never guesses an effect.
- **Equal-scope conflicts.** Every unordered pair of enabled rules whose resolved scopes are equal
  and whose outcomes differ: `FindingKind::EqualScopeConflict`, `resolution = Some(Deny)`,
  `requires_acknowledgement = !shadow`.
- **Incomparable conflicts.** Every unordered pair of enabled rules that `overlaps` and where
  neither contains the other, with different outcomes: `FindingKind::IncomparableConflict`,
  `resolution = Some(Deny)` when either is `Deny`, else `Some(RequireApproval)` (D-19),
  `requires_acknowledgement = !shadow`.
- **Ineffective under a lock.** Every pair `(locked deny L, enabled rule R)` with `R.outcome != Deny`,
  `R != L`, and `scope_contains(L, R)`: `FindingKind::IneffectiveUnderLock`, `resolution =
  Some(Deny)`, `requires_acknowledgement = !shadow`.
- **Ceiling note.** For every enabled `Allow` rule with `TargetScope::Exact(S)`, if no
  `(host, summary)` in `ceilings` covers the rule's contract for any name in `S` (same
  `ceiling_covers` helper shape as limit 4, restricted to what a rule can express), emit one finding
  per uncovered name with `requires_acknowledgement = false`. An empty `ceilings` map produces no
  findings: nothing is known, so nothing is claimed.
- Ordering: findings are sorted by `(kind, first, second)`, so the report is deterministic.
- `Outcome` here is used for the *resolution of a rule pair*, so it is always `Allow`,
  `RequireApproval` or `Deny`; a rule outcome is never `Unavailable` (validation rejects it).

### 6.12 Simulator (`simulate.rs`) **fixed**

```rust
/// Evaluates each request through the same algorithm, returning one element per request, in the
/// same order. No I/O, no effects, no permits.
pub fn simulate(
    rules: &[Rule],
    requests: &[EvaluateRequest],
    selection: RuleSelection,
) -> Vec<Result<Decision, PolicyError>>;
```

- `simulate` calls `run` once per request, so `simulate(rules, requests, RuleSelection::Enforced)`
  and `requests.iter().map(|r| evaluate(rules, r))` produce equal vectors. That equality is
  property-tested (`simulator_matches_live_evaluator`).
- Loading historical requests (replaying recorded facts, pinned subjects and ceiling hash) and
  diffing against the decision actually taken are prompt 40's; this function only maps inputs to
  decisions.
- A malformed request (or rule set) is `Err(PolicyError::…)` in that element; the other elements are
  still evaluated. No element is dropped, so `len()` always equals `requests.len()`.

### 6.13 Validation (`evaluate.rs`) **fixed**

`validate_rules` checks, in this order, returning the first failure:

| Check | Error |
|---|---|
| two rules share a `RuleId` | `PolicyError::DuplicateRuleId` |
| `locked` is set and `outcome != Deny` | `PolicyError::LockedNotDeny` |
| `outcome` is `Unavailable(..)` | `PolicyError::UnavailableRuleOutcome` |
| `TargetScope::Exact` is empty | `PolicyError::EmptyTargetSet` |
| a `TimeWindow` has `start == end` | `PolicyError::DegenerateTimeWindow` |
| an `ArgConstraint::arg` is not `[a-z][a-z0-9_]{0,63}` | `PolicyError::BadArgKey` |

The request checks in stage 0, in this order:

| Check | Error |
|---|---|
| `contract.effect != Contain` and `!containment.is_empty()` | `PolicyError::ContainmentOnNonContain` |
| `expected_ceiling_hash.is_some()` and `ceiling` is `None` | `PolicyError::CeilingHashMissing` |
| `facts.len() > MAX_FACTS` (1024) | `PolicyError::TooManyFacts` |

`MAX_FACTS` is a constant in `request.rs`; the bound exists so a caller cannot make the trace
arbitrarily large. Nothing else about a request can be invalid, because every field is a validated
type.

### 6.14 Errors (`error.rs`) **fixed**

```rust
/// Why a rule set, a request or a publication review was refused. Never carries a secret, never
/// carries owner text.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PolicyError {
    #[error("rule {rule}: `locked` is only valid on a `deny` rule, found {found}")]
    LockedNotDeny { rule: RuleId, found: &'static str },
    #[error("rule {rule}: a rule outcome cannot be `unavailable`")]
    UnavailableRuleOutcome { rule: RuleId },
    #[error("duplicate rule id: {0}")]
    DuplicateRuleId(RuleId),
    #[error("rule {rule}: `targets: Exact(..)` is empty; write `Any` instead")]
    EmptyTargetSet { rule: RuleId },
    #[error("rule {rule}: the time window {start}..{end} is empty")]
    DegenerateTimeWindow { rule: RuleId, start: String, end: String },
    #[error("rule {rule}: {key:?} is not a valid argument name")]
    BadArgKey { rule: RuleId, key: String },
    #[error("unknown contract id: {0}")]
    UnknownContract(ContractId),
    #[error("category {0} is not in the contract index")]
    CategoryNotInIndex(ContractCategory),
    #[error("contract {0} is not indexed with a category and effect")]
    ContractNotIndexed(ContractId),
    #[error("a containment posture was supplied for a contract that does not contain")]
    ContainmentOnNonContain,
    #[error("a ceiling hash was recorded but no ceiling summary was supplied")]
    CeilingHashMissing,
    #[error("{count} facts were supplied; at most {max} are accepted")]
    TooManyFacts { count: usize, max: usize },
}
```

`PolicyError` is the error type of `evaluate`, `evaluate_shadow`, `simulate`, `validate_rules`,
`resolve_contract` and `validate_publication`. An `Err` produces no `Decision`, so a caller has
nothing it could act on, and `Result` is `#[must_use]`, so a caller cannot silently drop a refusal.

## 7. Ordered implementation steps

Each step compiles and has its own tests. Do them in this order.

1. **Verify the repository context and write the differences down.** Open every file in §3. Confirm
   `EnginePolicy` and its four importers; confirm `proptest` is a dev-dependency; confirm the
   `engine` test target and its `#[cfg(test)] mod` list; confirm `crates/domovoi-core/src/capability/`
   exists (prompt 24 merged) and read `ContractMeta`, `ContractIndex::meta`,
   `effect_of_category`, `CeilingSummary`, `CeilingEntry`, `CeilingTargets`, `ConstraintKind`,
   `TargetName`, `TargetIdentity`'s actual definitions; confirm no `Authorization`/`PolicyPermit`
   exists. If `capability` is missing, **stop** and report.
2. `error.rs`: `PolicyError` and its messages.
3. `outcome.rs`: `Outcome`, `UnavailableReason`, `ConfirmationRequirement`,
   `OutcomeReason`, `HardLimit`. Unit test the severity order and the serde shape.
4. `rule.rs`: `RuleId` (parse, accept, reject cases), `HostScope`, `CapabilityScope`, `TargetScope`,
   `Rule`. Serde round-trip test.
5. `condition.rs`: `TimeOfDay`, `Condition`, `FactState`, `FactStatus`, `Condition::holds`. Unit
   tests for every row of §6.3's table, including wrap-around and non-numeric arguments.
6. `request.rs`: `CanonicalArgs` (including malformed lines), `ManagementPath`, `ContainmentInput`,
   `AutonomyState`, `EvaluateRequest`, `resolve_contract`, `names_to_resolve`, `MAX_FACTS`.
7. `specificity.rs`: `ResolvedScope`/`ResolvedCapability`, the three comparisons, `resolve_against`,
   `resolve_in_index`. Unit tests for every containment-table cell, the subset rule for targets, the
   incomparable pair, and the resolution lemma.
8. `decision.rs`: `Constraints`, `Overridden`, `Warning`, `Decision`.
9. `evaluate.rs`, stages 0–3: guard, match, locked `Deny`, default. Tests
   `no_match_uses_effect_default`, `locked_deny_beats_narrower_allow`,
   `multiple_locked_denies_one_deny_all_reasons`, `locked_deny_records_would_have_been`.
10. `evaluate.rs`, stages 4–6: winners, one outcome (with constraint intersection and the empty
    intersection), mixed outcomes. Tests for the decision table rows 2–10 and the `overridden` and
    `Warning::DangerousException` behaviour.
11. `evaluate.rs`, stage 7: the ten limits, `ceiling_covers`, `typed_confirmation`. Tests for
    decision-table rows 11–17 and every limit.
12. `publish.rs`: `validate_publication` and its four finding kinds.
13. `simulate.rs`: `simulate`.
14. `policy.rs`: the `mod` lines, the `pub use`s, the module doc. Then `cargo xtask arch-check`
    (modules moved/added).
15. Test files: `tests/engine/policy.rs`, `policy_publish.rs`, `policy_properties.rs`, and the three
    `mod` lines in `tests/engine/main.rs`. The 22 required tests plus the extra coverage of §12.
16. Run the checks in §Repository rules for `domovoi-engine` only, and write the completion report.

## 8. Persistence and migration

**None. This prompt adds no table, no column, no migration, no stored value and no index.**

- `Rule.version_id` and `EvaluateRequest.policy_version` / `Decision.policy_version` are
  `Option<Uuid>` **opaque tags supplied by the caller**. This module never generates, parses or
  interprets a version id; it copies it into the trace so prompt 40 can join the decision to the
  version that produced it.
- Prompt 40 owns the policy tables, version numbering, drafting and publication, the
  acknowledgement records, step-up on publish, and `policy.lock_changed` audit entries. Nothing in
  this module is a precondition for those tables beyond the value types above.
- Nothing in this module reads or writes a store, calls a `Clock`, or generates a UUID. `evaluate`
  takes `now` and the tags from its caller, so a replay of a past decision is possible with the
  recorded values and gives the same byte-identical `Decision`.
- No migration number is claimed. If a later prompt in this branch needs one, it takes the next
  free number from `crates/domovoi-store/migrations/` as `AGENTS.md` requires.

## 9. Protocol and compatibility

**No wire change, no protocol version change, no message, and no compatibility window.** This is a
new pure module in `domovoi-engine`, unreachable from the hub, the agent or the binary after this
prompt: nothing constructs an `EvaluateRequest` outside tests yet.

- No `domovoi-proto` change; no new field in `hello`, `agent_status`, `run_capability` or
  `action_status`.
- No HTTP endpoint and no OpenAPI change, so `cargo xtask openapi` and `npm run gen:api` are not run.
- No configuration key, so `cargo xtask docs-config` is not run.
- Existing installs, agents, skills, actions and incidents are unaffected: no behaviour changes
  anywhere.

## 10. Security requirements

Each of these is implemented somewhere above and tested by a named test in §12.

1. **No allowlist of rule shapes.** Evaluation takes whatever rules it is given and applies the
   same stages. There is no special case for a rule id, note, owner or origin.
2. **An unknown contract or an unknown category is an error, never a default `Allow`.**
   `resolve_contract` and `resolve_in_index` return `PolicyError`; `evaluate` cannot be reached
   without a resolved `ContractMeta`; a rule set or request that fails validation returns
   `Err(PolicyError::…)` and produces no `Decision`.
3. **An unknown, stale or disputed fact never allows.** Only `Known(expected)` satisfies
   `RequiresFact`, and hard limit 3 downgrades any `Allow` for a contract with a declared safety fact
   that is not `Known`.
4. **A locked `Deny` cannot be overridden.** Stage 2 runs before specificity and returns `Deny`
   regardless of what the winners say; the ordinary result is recorded only as `would_have_been`.
5. **Shadow never enforces.** Only `evaluate` is the enforced path; the algorithm is identical, and
   the selected rule set excludes every shadow rule from `evaluate` and every enforced rule from
   `evaluate_shadow`.
6. **`Destructive` is never `Allow`.** Hard limit 1 converts an `Allow` to `RequireApproval` + typed
   confirmation (D-06), whatever the rules say.
7. **A ceiling failure never allows.** Hard limit 4 covers a missing summary, `enabled == false`, no
   matching entry, uncovered targets, an over-limit rate or TTL, an unsatisfied declared argument
   bound, a non-numeric argument, and a hash disagreement.
8. **A root agent is never `Allow`.** Hard limit 10 (D-29 not in effect).
9. **Nothing here approves, permits or authorizes.** The module constructs no `HumanApproval`, no
   `PolicyPermit` and no `Authorization`; `xtask arch-check`'s authorization scans therefore see no
   new call site. `Decision` is data; only prompt 45 turns an `Allow` into a permit through
   `PermitIssuer`.
10. **No rule order and no rule id influences a result.** Both are property-tested. Ids sort the
    trace; they never break a tie, and there is no tie to break (§2).
11. **The trace explains every override, including the dangerous one.** `overridden` lists every
    broader rule a more specific winner beat when the outcomes differ, and
    `Warning::DangerousException { allow, deny }` marks the specific-`Allow`-over-broad-`Deny` case
    so the UI can style it as dangerous. A silent override would defeat the purpose of D-05.
12. **The trace carries no observed fact values.** `facts_used` holds `FactStatus` only, never the
    `Known(bool)` value the hub observed. A rule's own `RequiresFact { expected }` is echoed inside
    `constraints.conditions` because the owner wrote it; that is configuration, not observation.
13. **No I/O, so no attack surface.** No store, no clock, no HTTP, no audit log, no filesystem, no
    environment, no random. The engine's R2 restrictions are met by construction.

## 11. Error and recovery behaviour

| Situation | Behaviour |
|---|---|
| A rule names a `ContractId` the index does not have | `resolve_contract` / `resolve_in_index` → `PolicyError::UnknownContract`; `evaluate` with such a rule cannot arise from a validated set, and `validate_publication` refuses |
| A rule names a `ContractCategory` no contract uses | `PolicyError::CategoryNotInIndex`; publication refuses |
| `locked: true` on a non-`Deny` rule | `PolicyError::LockedNotDeny`; `evaluate` returns `Err`; publication refuses |
| A rule's `outcome` is `Unavailable(..)` | `PolicyError::UnavailableRuleOutcome`; `evaluate` returns `Err` |
| Two rules share an id | `PolicyError::DuplicateRuleId`; `evaluate` returns `Err`. Rejected rather than defined: prompt 24's `ContractIndex` refuses duplicate ids, and an id that is not unique makes `matched`, `winners` and `overridden` ambiguous, so the same rule applies here |
| `TargetScope::Exact(∅)` | `PolicyError::EmptyTargetSet`; `Any` is the way to say "every target" |
| A `TimeWindow` with `start == end` | `PolicyError::DegenerateTimeWindow` (a window that can never match is a dead rule) |
| Malformed input: an argument value that is not an integer, a `CanonicalArgs` line that does not parse, a fact absent from the map | not an error: the argument condition does not match, the fact reads as `Unknown`, and the rule does not match. Fail towards the default |
| `ceiling` is `None` | hard limit 4 if the outcome is `Allow` → `RequireApproval` |
| `ceiling` is stale or has no hash while the caller expects one | hard limit 4 (coverage fails) → `RequireApproval` |
| Ceiling hash mismatch | hard limit 4 → `RequireApproval`; the summary's hash is echoed into `ceiling_hash` |
| `containment.attested == false` for a `Contain` contract | hard limit 5 → `RequireApproval` + `Typed`, warning `SafetySetUnverified` |
| `containment.helper_incomplete` | hard limit 8 → `Unavailable(SafetyUnknown)`; nothing executes and nothing is offered |
| `ContainmentInput` populated for a non-`Contain` contract | `PolicyError::ContainmentOnNonContain`; `evaluate` returns `Err` |
| The rule set is empty | not an error: stage 3 applies the effect-class default, then stage 7. An empty policy is a policy, and it does not `Allow` a `Mutate`, `Contain` or `Destructive` capability |
| `expected_ceiling_hash` is set and `ceiling` is `None` | `PolicyError::CeilingHashMissing`; `evaluate` returns `Err` |
| More than `MAX_FACTS` facts supplied | `PolicyError::TooManyFacts`; `evaluate` returns `Err` |
| An evaluation that cannot proceed for any other reason | `Err(PolicyError::…)` for a malformed rule set or request, or `Unavailable(SafetyUnknown)` for limit 8. Never a panic, never a partial `Decision` |
| Any of the above | **never `Allow`** |

There is no retry, no recovery and no state: the function either returns a complete `Decision` or an
`Err`, and an `Err` is a refusal no caller may act on. Every `Unavailable` decision is a refusal the
caller must journal (prompt 40) and the UI must render as "could not decide", distinct from both `Deny`
and `RequireApproval`.

## 12. Testing requirements

Locations: unit tests live in a `#[cfg(test)] mod tests` at the bottom of the file they test, which
is the engine convention (`src/correlate.rs:140`, `src/actions/executor.rs:451`,
`src/notify/text.rs:33`). Integration tests live in the `engine` target: `tests/engine/policy.rs`,
`tests/engine/policy_publish.rs`, `tests/engine/policy_properties.rs`, each registered with
`#[cfg(test)] mod …;` in `tests/engine/main.rs`.

Build the request fixture from plain values: `ContractMeta` for `container.restart` (category
`container.lifecycle`, effect `Mutate`) or for a `Read` contract, a `TargetName::parse("plex")`, a
`TargetIdentity::parse("container:abc/plex")`, host `media`, `now` = `Timestamp::from_second(…)`, no
clock and no fake. No `domovoi-testkit` change.

### 12.1 The 22 required tests, named verbatim from `04` §7.7

"Table" means one `#[test]` iterating a table of cases with an `assert_eq!` per row (the project
style in `crates/domovoi-core/tests/serde_snapshots.rs` and the engine's `decide.rs`).

| # | Test | Setup → action → expected | Kind |
|---|---|---|---|
| 1 | `specific_allow_overrides_broader_deny` | Decision table row 3: `All / container.restart / Any → Deny` plus `media / container.restart / {plex} → Allow`; target `plex` → `Allow`, `reason = SingleWinner`, `winners = [narrow]`, `overridden = [(broad, narrow)]`, `warnings` contains `DangerousException { allow: narrow, deny: broad }` | table (rows 3, 4, 5, 13) |
| 2 | `broader_deny_applies_without_exception` | Row 4 (target `radarr`) and row 5 (other host) → `Deny`, `reason = SingleWinner`, only the broad rule matched, `overridden` empty | table |
| 3 | `equal_scope_deny_wins` | Row 6: two rules with identical scopes, `Allow` and `Deny` → `Deny`, `reason = EqualScopeDeny`, both in `winners`, `typed_confirmation = None` | table |
| 4 | `incomparable_conflict_falls_back_to_deny` | Row 7: `All / container.restart / {plex} → Allow` and `media / Category(container.lifecycle) / Any → Deny` → `Deny`, `reason = IncomparableFallback` | table |
| 5 | `incomparable_allow_vs_approval_requires_approval` | Row 7b: same pair with the second rule `RequireApproval` → `RequireApproval`, `reason = IncomparableFallback` | table |
| 6 | `evaluation_is_order_independent` | **proptest**: generate 0–8 rules from a strategy over `(HostScope, CapabilityScope, TargetScope, Outcome, conditions)` and a request from a strategy over `(host, contract, target, facts, ceiling)`. For each of 10 permutations, `evaluate` must return an equal `Decision` (and an equal serialized trace). Also assert the `matched`/`winners`/`locked`/`overridden` vectors are sorted | **proptest** (`ProptestConfig::with_cases(64)`, the engine precedent) |
| 7 | `equal_scope_allows_intersect_constraints` | Rows 9 and 10: `Exact({plex,jellyfin}) → Allow, MaxRate 3` plus `Exact({plex}) → Allow, MaxRate 1`; equal-scope pair at 3/h and 2/h → `Allow` with `constraints.max_per_hour == 1` and `== 2` respectively; also a `MaxTtl` pair and a targets pair | table |
| 8 | `no_match_uses_effect_default` | Empty rule set for each `EffectClass` → `Allow`/`RequireApproval`/`RequireApproval`/`RequireApproval`, `reason = Default`, `typed_confirmation = None/None/None/Typed`; and a disabled rule and a non-matching target both behave like no rule | table (4 rows) |
| 9 | `locked_deny_beats_narrower_allow` | Row 16: locked `All / container.restart / Any → Deny` plus `media / container.restart / {plex} → Allow` → `Deny`, `reason = LockedDeny`, `locked` = the broad rule, `winners` empty, `would_have_been` is `Some(Allow)` | table |
| 10 | `multiple_locked_denies_one_deny_all_reasons` | Row 17: two matching locked `Deny`s → one `Deny`, `locked` lists both, sorted | table |
| 11 | `locked_flag_ignored_unless_deny` | `validate_rules` on a rule with `locked: true, outcome: RequireApproval` → `Err(PolicyError::LockedNotDeny)`; also `locked: true, outcome: Allow` → same; and `evaluate` over that set → `Err`, never `Allow`, never a `Decision` | table |
| 12 | `destructive_never_allowed` | Row 11: `All / Any / Any → Allow` with a `Destructive` contract → `RequireApproval`, `hard_limits_applied == [DestructiveNeverAllowed]`, `typed_confirmation = Typed` | table |
| 13 | `ceiling_caps_allow` | Row 12 and the four failure shapes: entry absent, `enabled == false`, target not in `Names`, requested rate above `max_per_hour`; each → `RequireApproval` with `CeilingDoesNotCover`; a covering summary → `Allow` | table |
| 14 | `unknown_safety_fact_never_allows` | Row 15: `RequiresFact(host.not_in_maintenance)` with the fact absent, `Unknown`, `Stale`, `Disputed`, and `Known(other)` → the rule does not match (falls to the broader rule or the default); a contract with `safety_facts` and the fact `Unknown` → `RequireApproval` with `SafetyFactUnknown` | table |
| 15 | `model_target_cannot_widen_scope` | Row 13b: `media / container.restart / Any → Allow` with target `postgres` not in `pinned` and not named by any winner → `RequireApproval`, `hard_limits_applied == [TargetNotPinned]`; the same rule with `postgres` pinned → `Allow` | table |
| 16 | `containment_exclusion_denies_even_with_allow` | Rows 14: `media / Effect(Contain) / Any → Allow` with `containment.target_overlaps_verified_set = true` → `Deny` with `ContainmentTargetExcluded`, and the same for a request whose outcome was already `RequireApproval` | table |
| 17 | `containment_requires_confirmed_management_paths` | Rows 14b/14c/14d: uncovered paths, `stale_confirmation`, and `attested == false` each → `RequireApproval` + `Typed` with `ManagementPathUncovered` or `ContainmentUnverified` and the matching warning; the covered case → `Allow` | table |
| 18 | `shadow_rules_never_enforced` | Row 18: only a shadow `media / container.restart / {plex} → Allow` → `evaluate` gives `RequireApproval` (the default) with an empty `matched`; `evaluate_shadow` gives `Allow` with that rule in `matched`; and a set with one enforced and one shadow rule never mixes them in either decision | table |
| 19 | `deny_is_not_offered_for_approval` | Every `Deny` case in the table: `typed_confirmation == None`, `constraints` empty, and no `Warning::SafetySetUnverified`/`ManagementPathsUnprotected` added to convert it into something offerable; assert that `Outcome::Deny` never carries `Typed` | table |
| 20 | `publish_reports_incomparable_conflicts` | Row 7's pair through `validate_publication` → one `IncomparableConflict` finding with `resolution = Some(Deny)`, `requires_acknowledgement = true`, and `is_blocked()`; the 7b pair → `resolution = Some(RequireApproval)` | table |
| 21 | `publish_reports_equal_scope_conflicts` | Row 6's pair → one `EqualScopeConflict` with `resolution = Some(Deny)` and `requires_acknowledgement = true`; the same pair with one shadow rule → `requires_acknowledgement = false` | table |
| 22 | `simulator_matches_live_evaluator` | **proptest**: generate rules and 1–6 requests; assert `simulate(rules, &requests, Enforced) == requests.iter().map(|r| evaluate(rules, r)).collect::<Vec<_>>()`, the same for `Shadow` with `evaluate_shadow`, and that `len()` matches | **proptest** |

### 12.2 Extra coverage required beyond the 22

Unit tests, in the file that defines the behaviour:

| Test | Behaviour |
|---|---|
| `specificity_is_antisymmetric` | `proptest`: `is_strictly_more_specific(a, b)` implies `!is_strictly_more_specific(b, a)`, and it is irreflexive |
| `host_scope_containment` | `All` ⊇ `Host(h)`, `Host(h)` ⊇ itself, `Host(a)` ⊉ `Host(b)` |
| `capability_rank_order_is_contract_category_effect_any` | the containment table of §6.8, every cell |
| `capability_rank_is_a_total_order_on_matches` | the resolution lemma: for a request, the matched scopes compare as a total order |
| `target_exact_containment_is_strict_subset` | `Exact({plex}) ⊂ Exact({plex,jellyfin})`, `Exact(_) ⊂ Any`, equal sets are not strictly comparable |
| `incomparable_scopes_overlap_only_when_every_dimension_intersects` | disjoint `Exact` sets do not overlap; overlapping sets with neither containing the other are incomparable |
| `strict_specificity_ignores_conditions` | two rules differing only in conditions are equally specific |
| `time_window_includes_start_excludes_end` | `09:00`–`17:00` at `09:00` matches, at `17:00` does not |
| `time_window_wraps_across_midnight` | `22:00`–`06:00` at `23:30` and `05:59` matches, at `12:00` does not |
| `arg_constraint_needs_a_numeric_value` | `IntAtLeast{24}` matches `24` and `30`, rejects `23`, `""`, `abc`, `2 4`, and an absent argument |
| `max_rate_and_max_ttl_match_within_bound` | at the bound matches, above it does not, `None` matches |
| `requires_fact_matches_only_known_expected` | the four non-matching states |
| `canonical_args_reads_name_length_value_lines` | `container:4:plex\n` → `get("container") == Some("plex")`; two arguments; a duplicate name returns the first |
| `canonical_args_fail_closed_on_malformed_line` | a line with no second `:`, a length that disagrees with the value, and an empty name yield `None` |
| `unknown_contract_is_rejected_not_defaulted` | `resolve_contract` on an id absent from a real `ContractIndex` → `Err(UnknownContract)` |
| `category_absent_from_index_is_rejected` | a `Rule` with an unindexed category → `Err(CategoryNotInIndex)` from `resolve_in_index` and from `validate_publication` |
| `rule_outcome_unavailable_is_rejected` | `PolicyError::UnavailableRuleOutcome` |
| `empty_exact_target_set_is_rejected` | `PolicyError::EmptyTargetSet` |
| `invalid_rule_set_never_allows` | for each `PolicyError`, `evaluate` returns `Err` and produces no `Decision` at all |
| `empty_rule_set_uses_effect_default` | `Mutate` → `RequireApproval`, `Read` → `Allow` (then hard limits) |
| `missing_ceiling_is_not_allowed` | `ceiling = None` → `RequireApproval` with `CeilingDoesNotCover` |
| `stale_ceiling_fails_coverage` | `expected_ceiling_hash` set, summary hash different or absent → `RequireApproval`; `ceiling = None` with an expected hash → `Err(PolicyError::CeilingHashMissing)` |
| `missing_containment_attestation_requires_typed_confirmation` | `Contain` + `attested == false` → `Typed` + `SafetySetUnverified` |
| `incomplete_containment_readiness_is_unavailable` | `helper_incomplete` → `Unavailable(SafetyUnknown)`, and with `target_overlaps_verified_set` also set → `Deny` (limit 7 precedes limit 8) |
| `root_agent_is_never_allowed` | `runs_as_root` with an otherwise `Allow` request → `RequireApproval` + `AgentRunsAsRoot` |
| `paused_autonomy_requires_approval` | `paused` → `RequireApproval` + `AutonomyInactive` |
| `circuit_broken_rule_requires_approval` | `circuit_broken` → `RequireApproval` + `AutonomyInactive` |
| `equal_scope_allows_intersect_target_sets` | `Exact({plex})` ∩ `Exact({plex,radarr})` → `{plex}` |
| `equal_scope_allows_conjoin_conditions` | two `Allow` winners contribute both conditions, in winner order |
| `allow_intersection_always_contains_the_requests_target` | `proptest`: for any rule set `evaluate` accepts and any matching request, an `Allow`'s `targets` is `None` or contains `request.target`; the intersection is never empty |
| `empty_target_intersection_is_unrepresentable` | a rule with `TargetScope::Exact(BTreeSet::new())` is refused by `validate_rules` (`EmptyTargetSet`) and never reaches the matcher; two `Allow` winners whose `Exact` sets are disjoint cannot both match one request, so the case has no decision to test |
| `several_hard_limits_apply_in_trace_order` | one request that trips limits 1, 3, 4, 9, 10 → `hard_limits_applied` is exactly `[DestructiveNeverAllowed, SafetyFactUnknown, CeilingDoesNotCover, AutonomyInactive, AgentRunsAsRoot]` in that order regardless of input order |
| `locked_deny_records_would_have_been` | the inner decision has `would_have_been == None`, empty `hard_limits_applied`, and the lock's `overridden` pairs are sorted |
| `publish_reports_rules_ineffective_under_a_lock` | a locked `Any/Any/Any → Deny` plus an enabled `Allow` → one `IneffectiveUnderLock` per rule, `requires_acknowledgement = true` |
| `publish_notes_allows_outside_every_ceiling` | an `Allow` naming a target no supplied ceiling covers → a `TargetOutsideEveryCeiling` finding with `requires_acknowledgement = false`; an empty `ceilings` map → no finding |
| `publication_report_is_deterministic` | the same draft validated twice, and with the rules shuffled, gives an equal `PublicationReport` |
| `evaluate_constructs_no_authorization` | a text scan over `src/policy/` proving no file names `HumanApproval`, `PolicyPermit` or `Authorization` at all — not in code and not in a doc comment — so the authorization scans of `xtask arch-check` see no new call site (mirror the walk in `tests/gates.rs:30-56`; a test path is exempt from the source scan, so the three names appear only in this test file) |

Do **not** add a gate test to `crates/domovoi-engine/tests/gates.rs`; the last row above is a plain
test in `tests/engine/policy.rs`, so nothing shared is touched.

### 12.3 Test hygiene

- No `tokio`, no `sleep`, no fake clock: the module is synchronous and time is a field.
- No snapshot tests. The trace is a Rust type here; prompt 40 owns its wire shape and any golden
  fixture.
- Every test name states the behaviour, not the function.

## 13. Acceptance criteria

1. `domovoi_engine::policy::{evaluate, Decision, Rule}` resolves, and so do
   `evaluate_shadow`, `validate_publication`, `simulate`, `RuleId`, `EvaluateRequest`,
   `Outcome`, `OutcomeReason`, `HardLimit`, `PolicyError`, `Constraints`, `Warning`.
2. `domovoi_engine::policy::EnginePolicy` is unchanged: same name, same three fields, same
   `Default`, still re-exported as `domovoi_engine::EnginePolicy` (`lib.rs:39`), and
   `crate::policy::EnginePolicy` still resolves for `correlate.rs:14`, `manager/decide.rs:17`,
   `manager/dismiss.rs:15` and `manager/actor.rs:22`. The incident manager's tests still pass
   unchanged.
3. `evaluate` and `evaluate_shadow` are pure: no `Clock`, no store, no agent link, no registry, no
   filesystem, no `tokio`, no `uuid` generation, and no argument besides `&[Rule]` and
   `&EvaluateRequest`.
4. The seven stages run in the order of §6.9 and the ten hard limits in the order of §6.10, with
   `hard_limits_applied` in that order.
5. The effect-class default table is implemented exactly: `Read` → `Allow`, `Mutate` →
   `RequireApproval`, `Contain` → `RequireApproval`, `Destructive` → `RequireApproval` + `Typed`.
6. Specificity is the product partial order of §6.8 with no numeric score, conditions excluded, and
   `overlaps`/`scope_contains`/`is_strictly_more_specific` are the only implementation in the crate.
7. Equal-scope winners with different outcomes give `Deny`; incomparable winners give `Deny` if any
   winner is `Deny`, else `RequireApproval`; neither ever inspects rule order or ids.
8. A matching locked `Deny` gives `Deny`/`LockedDeny` with every locked rule listed and the ordinary
   resolution in `would_have_been`.
9. The five `OutcomeReason` values are exactly `04` §7.5's, with no sixth; an `Allow`'s target set is
   `None` or contains the request's target, so the intersection is never empty.
10. `evaluate` never returns `Allow` for: a `Destructive` contract, a target outside `pinned` ∪ the
    winners' resolved targets, an unknown/stale/disputed declared safety fact, an uncovered or
    unverifiable ceiling, or a root agent; and a malformed rule set or request returns
    `Err(PolicyError)`, so it produces no `Decision` to act on.
11. No type in the module constructs, names or returns `HumanApproval`, `PolicyPermit` or
    `Authorization`; `crates/domovoi-hub/src/auth/` is untouched.
12. `validate_publication` reports equal-scope and incomparable conflicts with their resolution, and
    marks which findings need an acknowledgement; shuffle-invariant and sorted.
13. `simulate` returns exactly `evaluate` per request, in order, with the same length.
14. All 22 tests of `04` §7.7 exist under their exact names, in that order, plus the extra coverage of
    §12.2; every property test is a real `proptest` property over permutations or generated inputs.
15. `cargo fmt --all`, `cargo clippy -p domovoi-engine --all-targets -- -D warnings`,
    `cargo nextest run -p domovoi-engine` and `cargo xtask arch-check` pass. No other crate is
    edited.
16. Every public item has a doc comment stating what it is for; every file is at or below 500 lines.
17. No dependency, no migration, no wire type, no endpoint, no config key, no OpenAPI document and no
    `ARCHITECTURE.md` line changed.

## 14. Documentation requirements

- **`ARCHITECTURE.md` is not edited here.** Decision D-15 (`03` §RW-10, `99` D-15) updates it once per
  milestone, and the last M2 prompt does the M2 pass; the M7 pass belongs to the prompt that closes
  M7. Note the new module in the completion report so that pass can include it.
- **No book page.** `docs/book/src/SUMMARY.md` has no policy page today (verified), so there is
  nothing to extend, and the owner-facing explanation of rules, specificity and shadow mode belongs
  with the editor and the simulator that show it. Add none here.
- **Module docs.** `policy.rs`'s `//!` doc currently says "Tunables of the incident manager"
  (`src/policy.rs:1`). Extend it to state that the module now holds two unrelated things: the
  incident manager's tunables (`EnginePolicy`) and the capability policy evaluator (`Rule`,
  `Decision`, `evaluate`), and say in one sentence that the evaluator is pure and decides what
  authorizes what, adding nothing to the incident manager. Each new file gets a `//!` line saying
  its responsibility.
- **Doc comments** say what an item is for, not its name restated (R10). The security-relevant ones
  must say *why*: why `Unavailable` is not an authorization, why `locked` implies `Deny`, why an
  unknown fact never matches, why the intersection is an intersection.
- **No prompt numbers, no build-plan or agent references** in any committed file.
- **No OpenAPI or config reference regeneration** (no API or config change). `cargo xtask docs-links`
  is not needed: no links are added to the book.

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
