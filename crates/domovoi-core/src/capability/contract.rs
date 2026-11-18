//! The typed description of one capability.
//!
//! A contract is the hub's own description: what the capability is called, what its arguments
//! mean, what running it can do to a host and what a policy may bound. The fields are public so
//! the registry and the policy evaluator can read them, and the type has no `Deserialize`: only
//! the hub builds one, from its built-in table or through [`skill_contract`], so nothing an agent
//! or a skill author sends can become a contract. Every construction path runs the contract
//! through [`validate`](Self::validate), which refuses a contract whose fields contradict each
//! other; the rules themselves live in `rules.rs`.

use jiff::SignedDuration;
use serde::Serialize;

use super::effect::{
    EffectClass, ExecutionPath, Idempotency, Reversibility, TargetKind, VerificationHint,
};
use super::ids::{ContractCategory, ContractId, ContractVersion, FactId};
use super::schema::SchemaError;

/// The hub's own description of what one capability is and how dangerous it is.
///
/// Only the hub constructs these, from its built-in table or from a skill summary plus a floor.
/// There is no `Deserialize`, so nothing an agent or a skill author sends can build one, and the
/// effect class never comes from a wire value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CapabilityContract {
    /// Stable registry and policy id.
    pub id: ContractId,
    /// Version of this contract's shape and meaning.
    pub version: ContractVersion,
    /// Short owner-facing title for the capability inventory.
    pub title: String,
    /// One or two sentences saying what the capability does.
    pub description: String,
    /// JSON Schema of the capability's arguments.
    pub input_schema: serde_json::Value,
    /// What running the capability can do to a host.
    pub effect: EffectClass,
    /// The coarse family used for policy specificity.
    pub category: ContractCategory,
    /// What kind of thing the capability acts on.
    pub target_kind: TargetKind,
    /// The argument naming the target, when the target kind needs one.
    pub target_arg: Option<String>,
    /// Whether the effect can be undone.
    pub reversibility: Reversibility,
    /// Whether a repeated run changes anything further.
    pub idempotency: Idempotency,
    /// How long one run may take.
    #[serde(serialize_with = "jiff::fmt::serde::duration::friendly::compact::required")]
    pub timeout: SignedDuration,
    /// The most output the hub keeps and shows, in bytes.
    pub output_limit_bytes: u32,
    /// How to check that the capability worked.
    pub verification: VerificationHint,
    /// The arguments a host ceiling may constrain (D-18).
    pub constrainable_args: Vec<ConstrainableArg>,
    /// Facts that must be known before the policy evaluator may allow it.
    pub safety_facts: Vec<FactId>,
    /// Where the capability executes (RW-39).
    pub execution: ExecutionPath,
    /// Where this contract comes from.
    pub source: ContractSource,
    /// What a `contain` capability guarantees about how its containment ends (RW-39 §9.2).
    /// `None` for every other effect class, where there is no containment to end. A `contain`
    /// contract without it is refused, because the helper path alone says nothing about how the
    /// containment expires or is undone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub containment: Option<Containment>,
}

/// One argument an unattended host ceiling may constrain, and how its bound is read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConstrainableArg {
    /// The argument's key in the input schema.
    pub key: String,
    /// Whether the ceiling's bound is a minimum or a maximum.
    pub kind: ConstraintKind,
    /// What the bound means, for the approval card and the inventory.
    pub description: String,
}

/// How an integer ceiling bound on a constrainable argument is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintKind {
    /// The argument must be at least the ceiling's value.
    IntAtLeast,
    /// The argument must be at most the ceiling's value.
    IntAtMost,
}

/// What a `contain` capability promises about the containment it installs (§9.2).
///
/// A containment is not a run that ends when the agent ends: it holds a host down on its own
/// timer, and undoing it has to be possible after a crash. A contract that cannot say all of
/// this is refused by [`containment_rules`](../rules), so the helper path alone never makes an
/// incomplete containment look accepted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Containment {
    /// The schema argument carrying the TTL, in whole seconds, the containment must expire on.
    pub ttl_argument: String,
    /// What ends the containment.
    pub expiry: ExpiryOwner,
    /// The schema argument that arms a watchdog of its own, when the containment affects
    /// connectivity and must not leave the host unreachable; `None` when it does not.
    pub watchdog_argument: Option<String>,
    /// How the apply and its undo are recorded.
    pub rollback: RollbackDeclaration,
}

/// What ends a containment.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpiryOwner {
    /// A timer outside the agent ends it, which is the only shape that still holds when the agent
    /// is stopped, killed or crashed.
    KernelTimer,
    /// Ending the agent's own process is what would end it, so a stopped agent leaves the
    /// containment holding.
    AgentProcess,
}

/// How a containment's apply and its undo are recorded.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RollbackDeclaration {
    /// The journal holds both the apply and the undo, so the host can be brought back after a
    /// partial run.
    Journaled,
    /// Nothing records how to undo it, so a crash leaves a host that cannot be recovered from the
    /// hub's own records.
    Unrecorded,
}

/// Where a contract comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContractSource {
    /// Typed in `domovoi-core`; the hub owns it entirely.
    BuiltIn,
    /// A skill. `declared` is what the manifest says and `floor` is the hub's raised
    /// minimum, if any; the contract's `effect` is the stricter of the two.
    Skill {
        /// The class the skill's manifest declares.
        declared: EffectClass,
        /// The hub's allowed minimum, when the owner raised it.
        floor: Option<EffectClass>,
    },
}

/// Why a contract is not usable.
///
/// Every variant names one rule of [`validate`](CapabilityContract::validate), so a caller can tell which
/// field it has to fix. A schema problem is reported as [`SchemaInvalid`](ContractError::SchemaInvalid) carrying the
/// [`SchemaError`] that names the part of the argument schema at fault.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContractError {
    /// The output limit is outside the allowed range.
    #[error("{id}: output limit {bytes} is outside 1..=1048576 bytes")]
    OutputLimit {
        /// The contract that failed.
        id: ContractId,
        /// The rejected limit.
        bytes: u32,
    },
    /// The timeout is zero, negative or longer than an hour.
    #[error("{id}: timeout must be positive and at most one hour")]
    Timeout {
        /// The contract that failed.
        id: ContractId,
    },
    /// The title or the description is blank, so the inventory would show an unnamed
    /// capability.
    #[error("{id}: `{field}` must not be blank")]
    BlankText {
        /// The contract that failed.
        id: ContractId,
        /// The blank field.
        field: &'static str,
    },
    /// The target argument and the target kind disagree.
    #[error("{id}: the target argument and target kind disagree")]
    TargetArgMismatch {
        /// The contract that failed.
        id: ContractId,
    },
    /// The target argument is named outside the argument grammar.
    #[error("{id}: target argument `{key}` must match [a-z][a-z0-9_]{{0,63}}")]
    TargetArgBadName {
        /// The contract that failed.
        id: ContractId,
        /// The rejected name.
        key: String,
    },
    /// The schema does not declare the target argument, so nothing names what the capability
    /// acts on.
    #[error("{id}: the input schema does not declare target argument `{key}`")]
    TargetArgUndeclared {
        /// The contract that failed.
        id: ContractId,
        /// The missing argument.
        key: String,
    },
    /// The target argument is not text, so no target identity can be named by it.
    #[error("{id}: target argument `{key}` must be `string`")]
    TargetArgNotText {
        /// The contract that failed.
        id: ContractId,
        /// The argument.
        key: String,
    },
    /// The schema does not require the target argument, so a call could run with no target.
    #[error("{id}: the schema must require target argument `{key}`")]
    TargetArgNotRequired {
        /// The contract that failed.
        id: ContractId,
        /// The argument.
        key: String,
    },
    /// A read contract set a field only a capability that changes state may set.
    #[error("{id}: a read contract cannot set {field}")]
    ReadHasSideEffectFields {
        /// The contract that failed.
        id: ContractId,
        /// The field that does not apply to a read.
        field: &'static str,
    },
    /// A built-in capability that changes state did not say how it is undone and verified.
    #[error("{id}: a built-in that changes state needs a reversibility and a verification")]
    BuiltInNeedsReversibilityAndVerification {
        /// The contract that failed.
        id: ContractId,
    },
    /// A `mutate` capability is neither reversible nor idempotent, so repeating it can lose
    /// state and it is really `destructive`.
    #[error("{id}: a `mutate` capability must be reversible or idempotent")]
    MutateMustBeUndoable {
        /// The contract that failed.
        id: ContractId,
    },
    /// `contain` is a typed privileged operation, so only the root helper path may run it.
    #[error("{id}: a `contain` capability must run through the root helper")]
    ContainNeedsHelper {
        /// The contract that failed.
        id: ContractId,
    },
    /// A `contain` contract states no containment guarantee, so nothing says when the containment
    /// expires or how it is undone.
    #[error(
        "{id}: a `contain` contract must state the TTL, the independent expiry and the journalled undo of its containment"
    )]
    ContainmentUndeclared {
        /// The contract that failed.
        id: ContractId,
    },
    /// A contract that does not contain carries containment metadata, which would read as
    /// authoritative for a containment that is never installed.
    #[error("{id}: a containment guarantee belongs to a `contain` contract only")]
    ContainmentNotApplicable {
        /// The contract that failed.
        id: ContractId,
    },
    /// The named containment argument is outside the argument grammar.
    #[error("{id}: containment argument `{key}` must match [a-z][a-z0-9_]{{0,63}}")]
    ContainmentArgumentBadName {
        /// The contract that failed.
        id: ContractId,
        /// The rejected name.
        key: String,
    },
    /// The schema does not declare the containment argument, so the containment names a TTL that
    /// cannot be supplied.
    #[error("{id}: the input schema does not declare containment argument `{key}`")]
    ContainmentArgumentUndeclared {
        /// The contract that failed.
        id: ContractId,
        /// The missing argument.
        key: String,
    },
    /// The containment argument is not an integer, so its TTL cannot be compared with a ceiling.
    #[error("{id}: containment argument `{key}` must be an integer")]
    ContainmentArgumentNotInteger {
        /// The contract that failed.
        id: ContractId,
        /// The argument.
        key: String,
    },
    /// The containment argument is optional, so a run could install a containment with no TTL at
    /// all.
    #[error("{id}: containment argument `{key}` must be required")]
    ContainmentArgumentNotRequired {
        /// The contract that failed.
        id: ContractId,
        /// The argument.
        key: String,
    },
    /// The containment argument states no bounded positive range, so the TTL it carries could be
    /// zero, negative or unbounded.
    #[error(
        "{id}: containment argument `{key}` must state a range from at least one second up to a bound"
    )]
    ContainmentRangeUnbounded {
        /// The contract that failed.
        id: ContractId,
        /// The argument.
        key: String,
    },
    /// The containment expires only when the agent ends, which is not a containment the host can
    /// rely on.
    #[error("{id}: a containment must expire on a timer outside the agent")]
    ContainmentExpiryNotIndependent {
        /// The contract that failed.
        id: ContractId,
    },
    /// The containment's apply and undo are not journalled, so a partial run leaves a host the hub
    /// cannot bring back.
    #[error("{id}: a containment must journal its apply and its undo")]
    ContainmentRollbackNotJournalled {
        /// The contract that failed.
        id: ContractId,
    },
    /// The root helper path is only for capabilities that change state.
    #[error("{id}: the root helper path cannot run a read")]
    HelperPathForRead {
        /// The contract that failed.
        id: ContractId,
    },
    /// A recheck delay is negative or longer than an hour.
    #[error("{id}: a recheck delay must be between 0s and 1h")]
    RecheckDelayInvalid {
        /// The contract that failed.
        id: ContractId,
    },
    /// The id, the source, the execution path and the target kind disagree about whether this
    /// is a skill.
    #[error(
        "{id}: a skill contract needs a `skill.` id, a skill source, the skill runner and a host target"
    )]
    SkillSourceMismatch {
        /// The contract that failed.
        id: ContractId,
    },
    /// A built-in took the reserved `skill.` category namespace.
    #[error("{id}: a built-in may not take the `skill.` category namespace")]
    BuiltInSkillCategory {
        /// The contract that failed.
        id: ContractId,
    },
    /// The contract's effect is not what its declared class and floor imply.
    #[error("{id}: expected effect {expected:?}, found {found:?}")]
    SkillEffectMismatch {
        /// The contract that failed.
        id: ContractId,
        /// The effect the source implies.
        expected: EffectClass,
        /// The effect the contract states.
        found: EffectClass,
    },
    /// A skill contract is, or claims to be, `contain`.
    #[error("{id}: a skill contract cannot be `contain`")]
    SkillContain {
        /// The contract that failed.
        id: ContractId,
    },
    /// The skill's id is not exactly `skill.<skill name>`, so the runner would not know which
    /// manifest to load: the suffix is not one skill name.
    #[error("{id}: a skill contract's id must be exactly `skill.<skill name>`")]
    SkillIdNotCanonical {
        /// The contract that failed.
        id: ContractId,
    },
    /// A postcondition is blank, so the approval card would claim a verification that says
    /// nothing was expected to hold.
    #[error("{id}: a postcondition needs a description of what must hold after the run")]
    BlankPostcondition {
        /// The contract that failed.
        id: ContractId,
    },
    /// A skill's category does not name its effect class.
    #[error("{id}: expected category `{expected}`, found `{found}`")]
    SkillCategoryMismatch {
        /// The contract that failed.
        id: ContractId,
        /// The category the effect implies.
        expected: ContractCategory,
        /// The category the contract states.
        found: ContractCategory,
    },
    /// A constrainable argument is declared twice.
    #[error("{id}: constrainable argument `{key}` is declared twice")]
    DuplicateConstrainableArg {
        /// The contract that failed.
        id: ContractId,
        /// The repeated key.
        key: String,
    },
    /// A constrainable argument key is outside the argument grammar.
    #[error("{id}: constrainable argument `{key}` must match [a-z][a-z0-9_]{{0,63}}")]
    BadConstrainableKey {
        /// The contract that failed.
        id: ContractId,
        /// The rejected key.
        key: String,
    },
    /// The schema does not declare a constrainable argument, so a ceiling bound on it would be
    /// ignored at the call boundary.
    #[error("{id}: the input schema does not declare constrainable argument `{key}`")]
    ConstrainableUndeclared {
        /// The contract that failed.
        id: ContractId,
        /// The missing argument.
        key: String,
    },
    /// A constrainable argument is not an integer, so no ceiling bound applies to it.
    #[error("{id}: constrainable argument `{key}` must be `integer`")]
    ConstrainableNotInteger {
        /// The contract that failed.
        id: ContractId,
        /// The argument.
        key: String,
    },
    /// A constrainable argument states no bound, so the ceiling's bound has nothing to be
    /// compared against in the schema.
    #[error("{id}: constrainable argument `{key}` must state at least one integer bound")]
    ConstrainableUnbounded {
        /// The contract that failed.
        id: ContractId,
        /// The argument.
        key: String,
    },
    /// A constrainable argument has no description, so the approval card would show a bare key.
    #[error("{id}: constrainable argument `{key}` needs a description")]
    ConstrainableBlankDescription {
        /// The contract that failed.
        id: ContractId,
        /// The argument.
        key: String,
    },
    /// A safety fact is listed twice, which reads as one fact the evaluator waits for.
    #[error("{id}: safety fact `{fact}` is listed twice")]
    DuplicateSafetyFact {
        /// The contract that failed.
        id: ContractId,
        /// The repeated fact.
        fact: FactId,
    },
    /// The argument schema is refused by the structural validator, which says which part.
    #[error("{id}: the input schema is refused: {reason}")]
    SchemaInvalid {
        /// The contract that failed.
        id: ContractId,
        /// What is wrong with the schema.
        reason: SchemaError,
    },
}
