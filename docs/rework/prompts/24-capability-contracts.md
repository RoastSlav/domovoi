# 24 — Capability contracts

| | |
|---|---|
| Branch | `capability-contracts` |
| Milestone | M2 |
| Depends on (must be merged) | 23 (`single-execution-boundary`). This prompt itself touches only `domovoi-core`, so it does not use anything 23 adds; the dependency exists only for ordering. |
| Can run in parallel with | 53, 54, 55, 56 (research spikes; they change no product code) |
| Produces (shared contracts) | `domovoi_core::capability::{ContractId, ContractVersion, ContractCategory, FactId, TargetName, TargetIdentity, EffectClass, TargetKind, Reversibility, Idempotency, ExecutionPath, VerificationHint, ConstrainableArg, ConstraintKind, ContractSource, CapabilityContract, ContractError, ContractIndex, IndexError, CeilingSummary, CeilingEntry, CeilingTargets, SkillFloor, FloorError}`; `capability::builtin::{builtin_contracts, contract_id_of}`; `capability::skill::{skill_contract, effective_effect}`. No table, no endpoint, no wire change. |
| Consumes | Existing `ReadTool`, `ReadToolCall`, `ReadTool::json_schema`, `SkillSummary`, `Risk`, `SkillName`, `ParamSpec`. If prompt 20 changed `SkillSummary.risk` to an open (forward-compatible) type, the skill adapter must read it through that type (see section 6, `skill_contract`); verify in code. |
| Launch gate | none |
| Suggested model | medium |

## 1. Objective

Give the hub one typed description of **what each capability is and how dangerous it is**, owned by
`domovoi-core` and never supplied by an agent or a skill author. Concretely:

1. Core contract types: id, version, input schema, effect class (`Read | Mutate | Contain |
   Destructive`), category, target kind, reversibility, idempotency, timeout, output limit,
   verification hint, the arguments a host ceiling may constrain, and (needed by the policy
   evaluator) the safety-critical facts the capability depends on.
2. **Built-in contracts derived from the existing `ReadTool` catalogue.** The ten non-skill read tools
   each get one contract. `ReadTool` itself is **not** changed or removed in this prompt.
3. The data-defined family **`skill.<name>`** and its adapter: a skill's effect class comes from its
   manifest `risk`, and the hub may keep a **floor**: the owner may raise the class, never lower it,
   and an unknown declared class counts as `Destructive`.
4. A validated set of contracts (`ContractIndex`) that rejects duplicate ids and a category that spans
   two effect classes, because the policy specificity order depends on "a category belongs to exactly
   one effect class".
5. The host-ceiling summary **types** (`CeilingSummary`), so the registry (prompt 25) and the policy
   evaluator (prompt 39) share one definition. This prompt only defines them; it does not parse a
   ceiling or put anything on the wire.

Nothing in this prompt changes behaviour: no code path consults a contract yet. Prompt 25 makes the
registry and execution boundary use them, and prompt 39 makes the evaluator use them.

## 2. Architectural context

Decisions this prompt implements, in plain terms:

- **RW-07 (hub-owned contracts; owner decision D-02).** Today the hub takes a skill's `risk` from the
  agent's summary. A skill that declares `risk = "read"` therefore runs whenever the model asks, with
  no human step. The fix is that **the hub owns the contract**: built-in contracts are typed in
  `domovoi-core`; skills are a data-defined family `skill.<name>`; agents advertise only
  implementation facts (contract id, versions, implementation hash, availability, local ceiling
  summary) and never an effect class. For skills, the effect class still originates in the owner's
  local manifest (the trust boundary is unchanged), but the hub applies a **floor**: the owner may
  raise a skill's class in the hub, never lower it; an unknown class is `Destructive`.
- **Effect classes.** `Read` (observes only), `Mutate` (changes state reversibly or idempotently),
  `Contain` (reduces blast radius, for example a timed firewall block; always with kernel-enforced
  expiry, M9), `Destructive` (can lose data). The policy evaluator (04 §7.3) uses the class for its
  defaults and hard limits: `Destructive` is never `Allow` (D-06), `Contain` has its own attestation
  rules (RW-37).
- **Specificity needs a stable taxonomy (RW-10, 04 §7.2).** Rules can name a capability as
  `Contract(id)`, `Category(cat)`, `Effect(class)` or `Any`, ordered `Contract ⊂ Category ⊂ Effect ⊂
  Any`. The order only works if every category belongs to exactly one effect class, so a contract set
  that violates this is invalid (`IndexError::CategorySpansEffects`).
- **Constrainable arguments (D-18).** A host's unattended ceiling may constrain only the arguments the
  contract declares (for example `min_prefix_len_v4` of a future `firewall.block_source`). This prompt
  adds the declaration; no built-in read contract declares any.
- **Safety-critical facts (04 §7.3 step 7).** A contract may declare facts that must be known, fresh
  and undisputed before the evaluator may return `Allow`. Reads declare none.
- **Trust boundary (RW-39, D-29).** Contracts say *where* a capability executes (`ExecutionPath`):
  in the hub, in the agent, through the skill runner, or through the root helper. The helper path is
  used from M8 on; this prompt only names it so later prompts need no type change.
- **Why `TargetName` and `TargetIdentity` live here.** Policy rules name targets by *name* and
  permits and plan hashes bind a canonical *identity* (RW-34). Both the registry (25) and the
  evaluator (39) need those two types and run in parallel, so they are defined once here as opaque
  validated strings. The resolver that maps a name to an identity is prompt 25's.
- **What is not decided here.** Which agent offers which contract on which host (availability,
  prompt 25); which rule matches (prompt 39); `skill.<name>` enable/disable storage (prompt 25
  keeps the existing setting); the model-facing meta-tools (prompt 31).

## 3. Repository context

All paths are relative to the repository root. Verify each before relying on it.

- `crates/domovoi-core/src/triage/tools.rs:1-92`: the closed `ReadTool` enum (11 variants),
  `ReadTool::ALL`, `name()`, `description()`, `from_name()`. Gate tests pin its contents.
  ```rust
  pub enum ReadTool { DockerPs, DockerLogs, DockerInspect, DockerEvents, HostOverview, Journal,
      ListeningPorts, SmartReport, IncidentHistory, NotesSearch, RunReadSkill }
  ```
- `crates/domovoi-core/src/triage/tool_schema.rs:1-114`: `ReadTool::json_schema(&self) -> Value`
  (`object(properties, required)` helper; `additionalProperties: false`). For `RunReadSkill` the
  schema is the generic `{skill, args}` wrapper, which a skill contract does not reuse.
- `crates/domovoi-core/src/triage/tool_call.rs:1-120`: the argument limits (`MAX_LINES = 500`,
  `MAX_SINCE_MINUTES = 1440`, `MAX_NAME_CHARS = 128`, `MAX_HISTORY = 50`, `MAX_NOTES = 20`, ...)
  and `ReadToolCall`. Tool results come from two places: `IncidentHistory` and `NotesSearch` are
  answered **inside the hub** from the store (`crates/domovoi-engine/src/triage/tools.rs:105-145`);
  the other eight go to the agent through `AgentLink::run_tool`
  (`crates/domovoi-engine/src/triage/tools.rs:165-180`). `RunReadSkill` is a skill call.
- `crates/domovoi-engine/src/triage/tools.rs:30-36`: `MAX_RESULT_BYTES = 8 * 1024` (hub cap on a tool
  result) and `TOOL_DEADLINE = SignedDuration::from_secs(15)` (per-call deadline). The built-in read
  contracts take exactly these values (see section 6, and test `builtin_read_limits_match_triage`).
- `crates/domovoi-core/src/skill/manifest.rs:28-38`: `Risk { Read, Write, Destructive }`
  (`serde(rename_all = "snake_case")`). `manifest.rs:170-197`: `SkillSummary { name, description,
  risk, params: Vec<ParamSpec>, applies_to, manifest_hash, timeout: SignedDuration, verify:
  Option<VerifySpec>, readme }`. `crates/domovoi-core/src/skill/params.rs:18-59`: `ParamKind {
  String{pattern,max_len}, Integer{min,max}, Boolean, Enum{values} }` and `ParamSpec { name,
  description, required, kind }`.
- `crates/domovoi-core/src/skill/mod.rs:1-40`: the private `ident_newtype!` macro and `check_ident`
  (used for `SkillName`: `^[a-z][a-z0-9_]{0,63}$`). `crates/domovoi-core/src/error.rs:1-22`:
  `ValidationError { field: &'static str, reason: String }` with `ValidationError::new`.
- `crates/domovoi-core/src/lib.rs:1-80`: module list and flat `pub use` re-exports. Note
  `ReadTool`, `Risk`, `SkillSummary` are re-exported at the root.
- `crates/domovoi-core/tests/gates.rs:1-60`: `read_tool_catalogue_is_read_only` pins the eleven names
  and forbids names containing `approve`, `exec`, `write`, `delete`, `restart`, `run_skill`, `shell`,
  `command`. **Never weaken it.**
- `crates/domovoi-core/tests/serde_snapshots.rs:191-198` (`read_tool_schemas_snapshot`) shows the
  `insta` JSON snapshot convention; snapshot files live in `crates/domovoi-core/tests/snapshots/`.
- `crates/domovoi-core/Cargo.toml`: dependencies already include `serde`, `serde_json`, `thiserror`,
  `jiff`, `sha2`, `hex`, `regex`; dev-dependencies `proptest`, `insta`. Add nothing.
- Note for later prompts: `docs/rework/04-target-architecture.md` §7.1 writes the rule's capability
  scope as `Contract(ContractId)`; the id strings used by policy examples are `container.restart`,
  `unit.restart`, `firewall.block_source`. Those are M8/M9 contracts and are **not** created here.

## 4. Exact scope

Add a module `domovoi_core::capability` with these files (each at most 500 lines):

| File | Contents |
|---|---|
| `crates/domovoi-core/src/capability/mod.rs` **(new)** | module doc; `pub use` of everything in section 6 |
| `.../capability/ids.rs` **(new)** | `ContractId`, `ContractVersion`, `ContractCategory`, `FactId`, `TargetName`, `TargetIdentity` and their validation |
| `.../capability/effect.rs` **(new)** | `EffectClass`, `TargetKind`, `Reversibility`, `Idempotency`, `ExecutionPath`, `VerificationHint` |
| `.../capability/contract.rs` **(new)** | `CapabilityContract`, `ConstrainableArg`, `ConstraintKind`, `ContractSource`, `ContractError`, `CapabilityContract::validate` |
| `.../capability/index.rs` **(new)** | `ContractIndex`, `ContractMeta`, `IndexError` |
| `.../capability/ceiling.rs` **(new)** | `CeilingSummary`, `CeilingEntry`, `CeilingTargets` (data only) |
| `.../capability/builtin.rs` **(new)** | `builtin_contracts()`, `contract_id_of(ReadTool)` |
| `.../capability/skill.rs` **(new)** | `skill_contract`, `skill_input_schema`, `effective_effect`, `SkillFloor`, `FloorError` |
| `.../capability/tests.rs` **(new)** | unit tests (compiled with `#[cfg(test)]`) |
| `crates/domovoi-core/src/lib.rs` | add `pub mod capability;` and re-export the main types at the crate root: `CapabilityContract, ContractId, EffectClass, TargetKind` only; everything else is reached through `domovoi_core::capability::` |
| `crates/domovoi-core/tests/gates.rs` | **extend** with `read_derived_contracts_are_read_only` (section 12) |
| `crates/domovoi-core/tests/serde_snapshots.rs` | add `builtin_contracts_snapshot` and `skill_contract_snapshot`; snapshot files are generated by `cargo insta` and reviewed |
| `crates/domovoi-engine/tests/` | one test pinning the built-in read limits to the engine constants (section 12) |
| `docs/` | see section 15 |

## 5. Explicit non-goals

- **No change to `ReadTool`, `ReadToolCall`, `tool_schema.rs`, the engine, the hub, the agent, the
  store, the proto crate or the web UI.** The model keeps calling `docker_ps` etc. The contract id
  `container.list` is a registry and policy name, not a model-facing name. The model-facing meta-tools
  are prompt 31.
- No contract for `run_read_skill`: it is the entry point to the skill family, not a capability.
- No built-in `Mutate`, `Contain` or `Destructive` contracts. `container.restart`, `unit.restart` and
  `firewall.*` belong to prompts 45 and 46. Do not create placeholder contracts for them.
- No availability, enable state, registry, catalogue, endpoint, database table or migration (prompt
  25).
- No rule, evaluator or decision types (prompt 39).
- No target resolver and no `TargetRef` (prompt 25). Only the opaque `TargetName` and
  `TargetIdentity` strings.
- No storage or API for the skill floor. This prompt defines the pure rule and the value type only.
- No parsing of `agent.toml`, no wire type for the ceiling summary (prompt 20 owns the wire; prompt 25
  maps the wire type into `CeilingSummary`).
- Do not rename or move `Risk`, `SkillSummary`, `ParamSpec`.

## 6. Required interfaces and contracts

Mark: **fixed** means names, variants and semantics must match; **convention** means follow
repository style (derives, doc comments, error message wording).

### 6.1 Identifier types (`ids.rs`) **fixed**

All are newtypes over `String` (except `ContractVersion`), `#[serde(try_from = "String", into =
"String")]` (or `transparent` for the numeric one), `Debug, Clone, PartialEq, Eq, PartialOrd, Ord,
Hash`, with `parse(&str) -> Result<Self, ValidationError>`, `as_str()`, `Display`, `TryFrom<String>`
and `From<Self> for String`. Never panic on input.

| Type | Grammar | Max length | Notes |
|---|---|---|---|
| `ContractId` | one or more `.`-separated segments after the first, each segment `[a-z][a-z0-9_]*`; at least two segments | 80 | `container.list`, `skill.backup_db`. Error field name `"contract id"`. |
| `ContractCategory` | same grammar as `ContractId` | 64 | `container.observe`. Error field `"contract category"`. |
| `FactId` | `[a-z][a-z0-9_.]*` (dots allowed, single segment allowed) | 64 | names a fact the policy evaluator reads, for example `host.not_in_maintenance`. Error field `"fact id"`. |
| `TargetName` | any text with no control characters and no leading or trailing whitespace; 1 to 128 chars (counted in `char`s) | 128 | what an owner types in a rule, for example `plex`. Matching is exact and case-sensitive. Error field `"target name"`. |
| `TargetIdentity` | `<scheme>:<rest>` where `scheme` is one of `host`, `container`, `unit`, `ipnet` and `rest` is 1 to 300 chars of printable ASCII without spaces | 310 | canonical identity bound by permits and plan hashes (RW-34), for example `container:2b0c.../plex-id`. This prompt only validates the shape. Error field `"target identity"`. |

`ContractId` extra constructors:

```rust
impl ContractId {
    /// The id of the skill family member `name`: `skill.<name>`.
    pub fn skill(name: &SkillName) -> ContractId;       // cannot fail: skill names fit the grammar
    /// `Some(name)` when this id is in the `skill.` family.
    pub fn skill_name(&self) -> Option<SkillName>;
    pub fn is_skill(&self) -> bool;
}
```

`ContractVersion(u32)`: `new(u32) -> Result<Self, ValidationError>` rejects 0; `get()`; serde
transparent; `Display` prints the number. Built-in contracts are version 1.

### 6.2 Effect and descriptive enums (`effect.rs`) **fixed**

```rust
/// What a capability can do to a host. Ordered by severity for the skill floor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectClass { Read, Mutate, Contain, Destructive }   // declaration order IS the severity order

impl EffectClass {
    pub const ALL: [EffectClass; 4];
    pub const fn as_str(&self) -> &'static str;          // "read" | "mutate" | "contain" | "destructive"
    /// Parses the declared class of a skill or catalogue entry. Anything that is not exactly one of
    /// "read", "write", "destructive" is `Destructive` (RW-07: an unknown class is the most
    /// dangerous one). "write" is the manifest word for `Mutate`. "mutate" and "contain" are
    /// NOT accepted from a declaration: a skill cannot declare itself `Contain`.
    pub fn from_declared_risk(text: &str) -> EffectClass;
}
impl From<Risk> for EffectClass { /* Read->Read, Write->Mutate, Destructive->Destructive */ }

pub enum TargetKind { None, Host, Container, Unit }            // serde snake_case
pub enum Reversibility { NotApplicable, Reversible, Irreversible }
pub enum Idempotency { Idempotent, NotIdempotent }
/// Where a contract executes (RW-39).
pub enum ExecutionPath { Hub, Agent, SkillRunner, Helper }
pub enum VerificationHint {
    /// Nothing to verify (reads).
    None,
    /// Re-run the originating check after a delay.
    Recheck,
    /// The capability's own postcondition, described for the approval card.
    Postcondition { description: String },
}
```
All derive `Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize` with `snake_case` renames;
`VerificationHint` is `#[serde(tag = "kind", rename_all = "snake_case")]`. `TargetKind` has no
`NetworkSource` variant yet: prompt 46 adds it together with its resolver.

Meaning of `ExecutionPath`: `Hub` = answered by the hub from its own store; `Agent` = run by the agent
process itself; `SkillRunner` = run through the skill runner identity (skills); `Helper` = a typed
privileged operation through the root helper (M8 and later). Built-ins in this prompt are `Hub` or
`Agent`; skill contracts are `SkillRunner`; nothing uses `Helper` yet.

### 6.3 The contract (`contract.rs`) **fixed**

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CapabilityContract {
    pub id: ContractId,
    pub version: ContractVersion,
    pub title: String,                       // short, owner-facing, for the inventory
    pub description: String,                 // one or two sentences
    pub input_schema: serde_json::Value,     // JSON Schema object of the arguments
    pub effect: EffectClass,
    pub category: ContractCategory,
    pub target_kind: TargetKind,
    /// The argument that names the target, when `target_kind` is not `None` or `Host`
    /// (for example "container"). `None` for host-level capabilities.
    pub target_arg: Option<String>,
    pub reversibility: Reversibility,
    pub idempotency: Idempotency,
    #[serde(serialize_with = "jiff::fmt::serde::duration::friendly::compact::required")]
    pub timeout: SignedDuration,
    pub output_limit_bytes: u32,
    pub verification: VerificationHint,
    /// The arguments a host ceiling may constrain (D-18); empty for every contract in this prompt.
    pub constrainable_args: Vec<ConstrainableArg>,
    /// Facts that must be Known before the evaluator may allow it (04 §7.3 step 7); empty here.
    pub safety_facts: Vec<FactId>,
    pub execution: ExecutionPath,
    pub source: ContractSource,
}

pub struct ConstrainableArg { pub key: String, pub kind: ConstraintKind, pub description: String }
/// How a ceiling bound on an integer argument is read.
pub enum ConstraintKind { IntAtLeast, IntAtMost }        // snake_case; the bound is the ceiling's value

pub enum ContractSource {
    BuiltIn,
    /// A skill. `declared` is what the manifest says; `floor` is the hub's raised minimum, if any.
    /// The contract's `effect` is `effective_effect(declared, floor)`.
    Skill { declared: EffectClass, floor: Option<EffectClass> },
}
```
The `Serialize` output is the shape the inventory API (prompt 25) will expose: keep field names as
written. `Deserialize` is not derived (contracts are built in code, never read from a provider).

`CapabilityContract::validate(&self) -> Result<(), ContractError>` enforces, in this order:

1. `output_limit_bytes` is in `1..=1_048_576`; `timeout` is positive and at most one hour.
2. `target_arg.is_some()` implies `target_kind` is `Container` or `Unit`; `target_kind` `Container` or
   `Unit` implies `target_arg.is_some()`.
3. `effect == Read` implies `reversibility == NotApplicable`, `idempotency == Idempotent`,
   `verification == VerificationHint::None`, `constrainable_args.is_empty()` and `safety_facts.is_empty()`.
4. `effect != Read` implies `reversibility != NotApplicable` and `verification != VerificationHint::None`.
5. `id.is_skill()` if and only if `source` is `ContractSource::Skill`; a skill contract has
   `execution == SkillRunner` and `target_kind == Host`; a non-skill contract never has
   `ExecutionPath::SkillRunner`.
6. A skill contract's `effect` equals `effective_effect(declared, floor)`, and its `declared` is never
   `Contain`.
7. `constrainable_args` keys are unique, match `[a-z][a-z0-9_]{0,63}`.
8. `input_schema` is a JSON object whose `"type"` is `"object"`.

```rust
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContractError {
    #[error("{id}: output limit {0} is outside 1..=1048576 bytes", ...)] OutputLimit { id: ContractId, bytes: u32 },
    #[error(...)] Timeout { id: ContractId },
    #[error(...)] TargetArgMismatch { id: ContractId },
    #[error(...)] ReadHasSideEffectFields { id: ContractId, field: &'static str },
    #[error(...)] MutatingNeedsReversibilityAndVerification { id: ContractId },
    #[error(...)] SkillSourceMismatch { id: ContractId },
    #[error(...)] SkillEffectMismatch { id: ContractId, expected: EffectClass, found: EffectClass },
    #[error(...)] DuplicateConstrainableArg { id: ContractId, key: String },
    #[error(...)] BadConstrainableKey { id: ContractId, key: String },
    #[error(...)] SchemaNotObject { id: ContractId },
}
```
(The `...` stands for a short, specific message; write them.)

### 6.4 Contract index (`index.rs`) **fixed**

```rust
/// What the policy evaluator needs to know about a contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractMeta {
    pub id: ContractId,
    pub category: ContractCategory,
    pub effect: EffectClass,
    pub target_kind: TargetKind,
    pub safety_facts: Vec<FactId>,
    pub constrainable_args: Vec<ConstrainableArg>,
}

/// A validated set of contracts.
#[derive(Debug, Clone, Default)]
pub struct ContractIndex { /* BTreeMap<ContractId, CapabilityContract>, BTreeMap<ContractCategory, EffectClass> */ }

impl ContractIndex {
    /// Validates every contract (`validate`), rejects duplicate ids and any category used by two
    /// effect classes.
    pub fn new(contracts: Vec<CapabilityContract>) -> Result<Self, IndexError>;
    pub fn get(&self, id: &ContractId) -> Option<&CapabilityContract>;
    pub fn meta(&self, id: &ContractId) -> Option<ContractMeta>;
    pub fn effect_of_category(&self, category: &ContractCategory) -> Option<EffectClass>;
    pub fn iter(&self) -> impl Iterator<Item = &CapabilityContract>;   // ordered by id
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IndexError {
    Invalid(ContractError),                                  // #[from]
    DuplicateId(ContractId),
    CategorySpansEffects { category: ContractCategory, first: EffectClass, second: EffectClass },
}
```
(`iter` returning `impl Iterator` is allowed: it is not a port signature.)

### 6.5 Ceiling summary types (`ceiling.rs`) **fixed** (data only)

These mirror `agent.toml [unattended]` (04 §8.3, D-18) after the agent has summarised it.

```rust
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CeilingSummary {
    /// `[unattended].enabled`.
    pub enabled: bool,
    pub entries: Vec<CeilingEntry>,
    /// Opaque hash of the agent's ceiling configuration, as reported (hex). The hub compares it for
    /// change detection and never interprets it.
    pub hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CeilingEntry {
    pub contract: ContractId,                                  // exact id: no wildcards (D-18)
    pub targets: CeilingTargets,
    pub max_per_hour: u32,
    pub max_active: Option<u32>,                               // stricter-only
    #[serde(with = "jiff::fmt::serde::duration::friendly::compact::optional")]
    pub max_ttl: Option<SignedDuration>,                       // stricter-only
    /// Bounds on the contract's declared constrainable args: key -> integer bound. How the bound is
    /// read (minimum or maximum) comes from the contract's `ConstraintKind`.
    pub args: BTreeMap<String, i64>,
}

#[serde(tag = "kind", content = "names", rename_all = "snake_case")]
pub enum CeilingTargets { Any, Names(BTreeSet<TargetName>) }
```
If the `friendly::compact::optional` serde helper is not available in the pinned `jiff` version, use
`#[serde(default, with = ...)]` as the existing code does for `SignedDuration` in
`crates/domovoi-core/src/skill/manifest.rs` or store `max_ttl_secs: Option<u64>` instead; say which in
the report. **Convention** for that choice only.

### 6.6 Built-in contracts (`builtin.rs`) **fixed**

```rust
/// The contracts of the ten non-skill read tools, in `ReadTool::ALL` order. Version 1.
pub fn builtin_contracts() -> Vec<CapabilityContract>;
/// The built-in contract id a read tool is the model-facing name of; `None` for `RunReadSkill`.
pub fn contract_id_of(tool: ReadTool) -> Option<ContractId>;
```

The exact table (every row: `effect = Read`, `reversibility = NotApplicable`, `idempotency =
Idempotent`, `verification = None`, `version = 1`, `timeout = 15 s`, `output_limit_bytes = 8192`,
`constrainable_args = []`, `safety_facts = []`, `source = BuiltIn`):

| `ReadTool` | Contract id | Category | Target kind / `target_arg` | Execution | `input_schema` |
|---|---|---|---|---|---|
| `DockerPs` | `container.list` | `container.observe` | `Host` / none | `Agent` | `ReadTool::json_schema()` |
| `DockerLogs` | `container.logs` | `container.observe` | `Container` / `container` | `Agent` | same |
| `DockerInspect` | `container.inspect` | `container.observe` | `Container` / `container` | `Agent` | same |
| `DockerEvents` | `container.events` | `container.observe` | `Host` / none (the `container` arg is an optional filter) | `Agent` | same |
| `HostOverview` | `host.overview` | `host.observe` | `Host` / none | `Agent` | same |
| `Journal` | `system.journal` | `host.observe` | `Host` / none (the `unit` arg is an optional filter) | `Agent` | same |
| `ListeningPorts` | `network.listening_ports` | `network.observe` | `Host` / none | `Agent` | same |
| `SmartReport` | `disk.smart_report` | `disk.observe` | `Host` / none (the `device` arg is a parameter, not a bound target) | `Agent` | same |
| `IncidentHistory` | `incident.history` | `knowledge.observe` | `Host` / none | `Hub` | same |
| `NotesSearch` | `notes.search` | `knowledge.observe` | `Host` / none | `Hub` | same |

Title and description: take the `ReadTool::description()` text as `description`; `title` is the
`name()` with underscores replaced by spaces and the first letter upper-cased ("Docker ps" is fine;
do not invent prose). `input_schema` is taken from `ReadTool::json_schema()` at call time, so the
schema can never drift from parsing.

The `timeout` and `output_limit_bytes` come from two named constants defined in `builtin.rs`:

```rust
/// Equals `domovoi_engine::triage::tools::TOOL_DEADLINE`; an engine test pins the equality.
pub const READ_TIMEOUT: SignedDuration = SignedDuration::from_secs(15);
/// Equals `domovoi_engine::triage::tools::MAX_RESULT_BYTES`; an engine test pins the equality.
pub const READ_OUTPUT_LIMIT: u32 = 8 * 1024;
```

### 6.7 The skill family and the floor (`skill.rs`) **fixed**

```rust
/// A hub-set minimum effect class for one skill. Only `Read`, `Mutate` and `Destructive` are valid:
/// `Contain` is reserved for typed helper capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "EffectClass", into = "EffectClass")]
pub struct SkillFloor(EffectClass);
impl SkillFloor {
    pub fn new(class: EffectClass) -> Result<Self, FloorError>;
    pub fn class(&self) -> EffectClass;
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FloorError { #[error("a skill floor cannot be `contain`")] ContainNotAllowed }

/// RW-07: the effective class is the stricter of the declared class and the floor. A floor can only
/// raise. With no floor the declared class stands.
pub fn effective_effect(declared: EffectClass, floor: Option<SkillFloor>) -> EffectClass;

/// The contract of one skill as the hub sees it.
pub fn skill_contract(summary: &SkillSummary, floor: Option<SkillFloor>) -> CapabilityContract;

/// The JSON Schema of a skill's parameters.
pub fn skill_input_schema(params: &[ParamSpec]) -> serde_json::Value;
```

`skill_contract` builds:

| Field | Value |
|---|---|
| `id` | `ContractId::skill(&summary.name)` |
| `version` | 1 |
| `title` | `summary.name` as written |
| `description` | `summary.description` |
| `input_schema` | `skill_input_schema(&summary.params)` |
| `effect` | `effective_effect(declared, floor)`, where `declared = EffectClass::from(summary.risk)`; if `summary.risk` is the open/forward-compatible type prompt 20 may have introduced, `declared = EffectClass::from_declared_risk(<its raw text>)` so an unknown value is `Destructive` |
| `category` | `skill.read`, `skill.mutate` or `skill.destructive` by the **effective** class (`skill.contain` never occurs) |
| `target_kind` | `Host`, `target_arg` none |
| `reversibility` | Read: `NotApplicable`. Otherwise `Reversible` when `summary.verify` is `Some`, else `Irreversible` for `Destructive` and `Reversible` for `Mutate`. (A skill's revert spec is not in `SkillSummary`; this field is informational for the inventory and approval card.) |
| `idempotency` | Read: `Idempotent`; otherwise `NotIdempotent` (the hub cannot know) |
| `timeout` | `summary.timeout` (clamped to at most one hour so `validate` holds) |
| `output_limit_bytes` | `READ_OUTPUT_LIMIT` for Read; 65536 for others (the agent caps skill stdout and stderr; the hub redacts and truncates) |
| `verification` | Read: `None`. Otherwise `Recheck` when `summary.verify` is `Some(v)` with `v.recheck`, else `Postcondition { description: "none declared by the skill" }` |
| `constrainable_args` | empty |
| `safety_facts` | empty |
| `execution` | `SkillRunner` |
| `source` | `Skill { declared, floor: floor.map(SkillFloor::class) }` |

`skill_input_schema`: object with one property per parameter and `required` listing required ones,
`additionalProperties: false`. By kind: `String { pattern, max_len }` -> `{"type":"string",
"pattern": pattern, "maxLength": max_len}`; `Integer { min, max }` -> `{"type":"integer","minimum":
min,"maximum": max}`; `Boolean` -> `{"type":"boolean"}`; `Enum { values }` -> `{"type":"string",
"enum": values}`. Every property also gets `"description"`. (The wire carries all skill arguments as
strings today; the schema describes the value's meaning, not its transport.)

A skill named so that `skill.<name>` would equal a built-in id cannot exist: built-in ids never start
with `skill.` and `ContractId::skill` always prefixes it. `validate` item 5 enforces the converse.

## 7. Ordered implementation steps

1. **Inspect and verify the repository context; report discrepancies.** Open every file and symbol
   named in section 3. Confirm `ReadTool` still has exactly eleven variants and the names in
   `gates.rs`; confirm `SkillSummary.risk` is `Risk` (or whatever type prompt 20 gave it) and note
   which; confirm `jiff` serde helpers used in `manifest.rs:48,186`; confirm no `capability` module
   already exists (`grep -rn "mod capability" crates/`). Confirm prompts 20 and 22 did not already
   introduce a ceiling-summary type in `domovoi-core`; if one exists, reuse it and drop section 6.5
   (report this). Write discrepancies down before editing.
2. `ids.rs`: the six identifier types, grammar functions, unit tests for each accept and reject case
   (section 12).
3. `effect.rs`: the enums, `from_declared_risk`, `From<Risk>`, serde names.
4. `contract.rs`: struct, `ContractError`, `validate`.
5. `index.rs`: `ContractIndex`, `ContractMeta`, `IndexError`.
6. `ceiling.rs`: the data types and serde round-trip tests.
7. `skill.rs`: `SkillFloor`, `effective_effect`, `skill_input_schema`, `skill_contract`.
8. `builtin.rs`: the table in 6.6, `contract_id_of` as an exhaustive `match` over `ReadTool` (no
   wildcard arm, so adding a tool without a decision fails to compile).
9. `mod.rs` and `lib.rs` re-exports. Keep every file under 500 lines (split `tests.rs` per area if
   needed).
10. Tests: unit tests in `capability/tests.rs`; gate test; snapshot tests; the engine pin test.
11. Documentation (section 15).
12. Run the checks listed in section 16 and write the completion report.

## 8. Database and persistence

Not applicable: this prompt adds no table, column, migration or stored value. (Prompt 25 stores the
skill floor in the existing `settings` table; prompt 40 stores policy versions.)

## 9. Protocol and compatibility

Not applicable: no wire message changes, the protocol version is untouched. `CeilingSummary` and
`CapabilityContract` are hub-internal types here. Prompt 20 owns the wire shapes; prompt 25 converts.
Because `CapabilityContract` does not derive `Deserialize`, nothing an agent sends can construct one.

## 10. Error handling and recovery

| Situation | Behaviour |
|---|---|
| `ContractId::parse` gets uppercase, one segment, empty, over 80 chars, or a leading digit | `Err(ValidationError { field: "contract id", .. })`; never panics |
| `TargetName::parse` gets text with a control character or surrounding whitespace, or 0 or over 128 chars | `Err(ValidationError { field: "target name", .. })` |
| `TargetIdentity::parse` gets an unknown scheme, empty rest or whitespace in the rest | `Err(ValidationError { field: "target identity", .. })` |
| `EffectClass::from_declared_risk("")`, `"READ"`, `"mutate"`, `"contain"`, `"nonsense"` | `Destructive` for each (only the exact lowercase words `read`, `write`, `destructive` are known) |
| `SkillFloor::new(Contain)` | `Err(FloorError::ContainNotAllowed)` |
| `ContractIndex::new` with two contracts of one id | `Err(IndexError::DuplicateId)`; the whole index is refused, not partially built |
| `ContractIndex::new` with category `x.y` used by a `Read` and a `Mutate` contract | `Err(IndexError::CategorySpansEffects)` |
| `ContractIndex::new` with a contract that fails `validate` | `Err(IndexError::Invalid(..))` naming the id |
| `skill_contract` for a skill whose timeout exceeds one hour | the contract's timeout is clamped to one hour; no error |

## 11. Security requirements

- **Provider cannot define its own authority (RW-07).** `CapabilityContract` has no `Deserialize`
  impl and no constructor that accepts an effect class from a wire value except through
  `EffectClass::from_declared_risk` (unknown becomes `Destructive`) and `effective_effect` (floor
  only raises). Test `provider_cannot_lower_effect_class`.
- **Fail closed to the most dangerous class.** Every unrecognised declaration is `Destructive`; a
  declaration of `contain` or `mutate` text is also unrecognised and becomes `Destructive`. Test
  `unknown_effect_is_destructive`.
- **No lowering by construction.** `proptest`: for any `declared` and any floor,
  `effective_effect(declared, Some(floor)) >= declared` and `>= floor.class()`.
- **A skill can never be `Contain`.** `SkillFloor::new(Contain)` fails; `validate` rejects a skill
  contract with `declared == Contain`.
- **Closed built-in read set stays closed.** `contract_id_of` is an exhaustive match. The gate test
  `read_derived_contracts_are_read_only` fails if any `ReadTool`-derived contract is not `Read`, has
  an id in the `skill.` family, or if `RunReadSkill` maps to a built-in.
- **Nothing here can approve or execute.** No type in this module constructs `HumanApproval`,
  `PolicyPermit` or `Authorization`. `domovoi-core` stays pure: no I/O, no new dependency.
- **No secrets.** Contracts and ceiling summaries hold no secret values.

## 12. Testing requirements

Unit tests live in `crates/domovoi-core/src/capability/tests.rs` unless a path is given. Use plain
values; no clock, no fakes are needed (the module is pure).

| Test | Setup, action, expected |
|---|---|
| `contract_id_grammar_accepts_and_rejects` | Accept `container.list`, `skill.backup_db`, `a.b.c`. Reject `list`, `Container.list`, `container..list`, `container.`, `.x`, `1a.b`, a 81-char id, `container.list ` (trailing space), empty. |
| `target_name_rejects_control_and_padding` | Accept `plex`, `my container`; reject `\tplex`, `plex `, `a\u{0}b`, empty, 129 chars. |
| `target_identity_requires_known_scheme` | Accept `host:0190...`, `container:abc/def`, `unit:media/plex.service`, `ipnet:10.0.0.0/24`; reject `file:/etc`, `container:`, `container:a b`. |
| `contract_version_rejects_zero` | `ContractVersion::new(0)` fails; `1` ok. |
| `unknown_effect_is_destructive` | The table in section 10 for `from_declared_risk`. |
| `provider_cannot_lower_effect_class` | A `SkillSummary` with `risk = Destructive` and floor `Some(Read)` gives `effect == Destructive`; `risk = Read` and floor `Some(Mutate)` gives `Mutate`; `risk = Read` with an unknown declared text (through `from_declared_risk("pwn")`) gives `Destructive` even with floor `None`. A summary named `docker_ps` yields id `skill.docker_ps`, never `container.list`. |
| `effective_effect_never_below_declared_or_floor` | `proptest` over all `EffectClass` pairs. |
| `skill_floor_rejects_contain` | `SkillFloor::new(EffectClass::Contain)` is `Err`; the other three are `Ok`. |
| `skill_contract_validates_for_every_risk` | Build a `SkillSummary` for each `Risk` (with and without `verify`) and call `validate()`; all pass. |
| `skill_contract_category_follows_effective_class` | declared Read floor Destructive gives category `skill.destructive`. |
| `skill_input_schema_covers_every_param_kind` | One param of each `ParamKind`; assert exact schema JSON. |
| `builtin_contracts_cover_every_read_tool` | For each `ReadTool::ALL`: `contract_id_of` is `Some` except for `RunReadSkill`; the ids of `builtin_contracts()` equal the `Some` ids in order. |
| `builtin_contracts_validate_and_index` | `ContractIndex::new(builtin_contracts())` is `Ok` with `len() == 10`; every contract passes `validate`. |
| `builtin_input_schema_matches_read_tool_schema` | For each mapped tool, `contract.input_schema == tool.json_schema()`. |
| `builtin_container_target_args_exist_in_schema` | For `container.logs` and `container.inspect`: `target_arg == Some("container")` and the schema has a `container` property. |
| `category_belongs_to_exactly_one_effect` | Two crafted contracts sharing a category with different effects: `IndexError::CategorySpansEffects`. |
| `duplicate_contract_id_rejected` | Same contract twice: `IndexError::DuplicateId`. |
| `validate_rejects_read_with_side_effect_fields` | A Read contract with `reversibility = Reversible`: `ReadHasSideEffectFields`. |
| `validate_requires_verification_for_mutating` | A Mutate contract with `VerificationHint::None`: error. |
| `validate_ties_skill_id_to_skill_source` | A `skill.x` id with `BuiltIn` source, and a non-skill id with `Skill` source: `SkillSourceMismatch`. |
| `ceiling_summary_serde_round_trip` | A summary with `Any` and `Names`, `max_ttl`, `args`: serialize then deserialize, equal. |
| `index_meta_exposes_category_effect_and_facts` | `meta(container.logs)` has category `container.observe`, effect `Read`. |

Other test locations:

- `crates/domovoi-core/tests/gates.rs`: **extend** with `read_derived_contracts_are_read_only`: every
  contract from `builtin_contracts()` has `effect == Read` and `source == BuiltIn` and a non-`skill.`
  id; `contract_id_of(ReadTool::RunReadSkill)` is `None`. Do not edit the existing tests.
- `crates/domovoi-core/tests/serde_snapshots.rs`: `builtin_contracts_snapshot` (the ten contracts as
  JSON, keyed by id) and `skill_contract_snapshot` (one write skill with a verify spec and floor
  `Destructive`). Review the generated `.snap` files line by line before accepting.
- `crates/domovoi-engine/tests/` (use the existing aggregate test target layout; find where
  `triage` tests live, for example `tests/services/` or `tests/engine/`; add the file next to them
  and register it in that target's `main.rs`/`mod.rs` the way the neighbours are): test
  `builtin_read_limits_match_triage` asserts `domovoi_core::capability::builtin::READ_TIMEOUT ==
  domovoi_engine::triage::tools::TOOL_DEADLINE` and `READ_OUTPUT_LIMIT as usize ==
  domovoi_engine::triage::tools::MAX_RESULT_BYTES`. If those two engine items are not `pub` from
  outside the crate, make the test a `#[cfg(test)]` unit test inside `crates/domovoi-engine/src/triage/tools.rs`'s
  test module instead; do not widen visibility.

Fakes: none needed. No `domovoi-testkit` change.

## 13. Acceptance criteria

1. `domovoi_core::capability` exists with every item in section 6 and nothing in `domovoi-engine`,
   `domovoi-hub`, `domovoi-agent`, `domovoi-store`, `domovoi-proto` or `web/` changed except the one
   engine pin test.
2. `builtin_contracts()` returns exactly the ten contracts in the 6.6 table, and
   `ContractIndex::new(builtin_contracts())` succeeds.
3. For every `ReadTool`, `contract_id_of` returns the id in the table, and `None` for `RunReadSkill`.
4. `EffectClass::from_declared_risk` returns `Destructive` for every input other than the exact
   words `read`, `write`, `destructive`.
5. `effective_effect` never returns a class below either argument (property test passes).
6. `SkillFloor::new(Contain)` fails, and `validate` rejects a skill contract declared `Contain`.
7. A skill contract's id is `skill.<name>`, its category is `skill.<effective class>`, and its
   `ContractSource::Skill` carries `declared` and `floor`.
8. `ContractIndex::new` rejects duplicate ids and a category spanning two effect classes.
9. The existing gate test `read_tool_catalogue_is_read_only` and `read_tool_schemas_snapshot` pass
   unchanged.
10. `cargo xtask arch-check` passes (no new dependency edge, files at most 500 lines, every public
    item documented).
11. The new tests exist under the names in section 12 and pass.

## 14. Migration and compatibility

Existing installs, agents, data and config are unaffected: no behaviour, schema or wire changes.
Prompt 25 will start using the contracts; until then they are unreferenced by runtime code (the
compiler may warn about nothing because the items are `pub`). Agents of any version keep working.

## 15. Documentation requirements

- `ARCHITECTURE.md`: do **not** edit in this prompt; decision D-15 says the architecture file is
  updated once per milestone by the last M2 prompt (25), which names the sections. Note the new
  module in the completion report so that prompt can include it.
- Book: no page change. Add module-level `//!` documentation to `capability/mod.rs` that explains in
  five to ten lines: contracts are hub-owned, built-ins come from the read catalogue, skills are the
  `skill.<name>` family with a raise-only floor, and unknown means `Destructive`.
- No OpenAPI or config reference regeneration (no API or config change).
- Keep the doc comments on every public item stating purpose, not the name restated.
