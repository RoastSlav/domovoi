//! A validated set of contracts the policy evaluator and the inventory can share.

use std::collections::BTreeMap;

use super::builtin::builtin_contracts;
use super::contract::{CapabilityContract, ConstrainableArg, ContractError, ContractSource};
use super::effect::{EffectClass, TargetKind};
use super::ids::{ContractCategory, ContractId, FactId};

/// What the policy evaluator needs to know about a contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractMeta {
    /// The contract's id.
    pub id: ContractId,
    /// The contract's category.
    pub category: ContractCategory,
    /// The contract's effect class.
    pub effect: EffectClass,
    /// What kind of thing the contract acts on.
    pub target_kind: TargetKind,
    /// Facts that must be known before it may be allowed.
    pub safety_facts: Vec<FactId>,
    /// Arguments a ceiling may constrain.
    pub constrainable_args: Vec<ConstrainableArg>,
}

/// A validated set of contracts, ordered by id.
#[derive(Debug, Clone, Default)]
pub struct ContractIndex {
    contracts: BTreeMap<ContractId, CapabilityContract>,
    categories: BTreeMap<ContractCategory, EffectClass>,
}

impl ContractIndex {
    /// Validates every contract, rejects duplicate ids and rejects a category used by two
    /// effect classes.
    ///
    /// The whole set is refused when any contract is invalid, so a partially built index never
    /// exists. Iteration is by id, so the same set always yields the same order.
    ///
    /// An empty set is accepted, which means "nothing composed yet": a stage that has read the
    /// skills but not the built-ins, or a unit test of one rule. Production code composes through
    /// [`with_builtins`](Self::with_builtins), which cannot be empty, and checks that boundary before it hands the
    /// index to the registry or the evaluator.
    pub fn new(contracts: Vec<CapabilityContract>) -> Result<Self, IndexError> {
        let mut by_id = BTreeMap::new();
        let mut categories = BTreeMap::new();
        for contract in contracts {
            contract.validate().map_err(IndexError::Invalid)?;
            if by_id.contains_key(&contract.id) {
                return Err(IndexError::DuplicateId(contract.id));
            }
            if let Some(first) = categories.get(&contract.category).copied()
                && first != contract.effect
            {
                return Err(IndexError::CategorySpansEffects {
                    category: contract.category.clone(),
                    first,
                    second: contract.effect,
                });
            }
            categories.insert(contract.category.clone(), contract.effect);
            by_id.insert(contract.id.clone(), contract);
        }
        Ok(Self {
            contracts: by_id,
            categories,
        })
    }

    /// The complete composition the registry and the evaluator consume: the built-in read table
    /// plus the contracts of the skills that resolved.
    ///
    /// This is the production boundary, so a later prompt builds one index instead of a second
    /// registry: the result always holds every built-in exactly once and is never empty. Every
    /// contract is validated first, so a skill cannot take a built-in id and a contract that is
    /// not a skill cannot hide in the skill list. A built-in read's category must follow the
    /// read convention, ending in the `observe` segment, which is what keeps the built-in
    /// families distinguishable from the skill family by category alone.
    pub fn with_builtins(skills: Vec<CapabilityContract>) -> Result<Self, IndexError> {
        let builtins = builtin_contracts();
        if builtins.is_empty() {
            return Err(IndexError::BuiltInsMissing);
        }
        for contract in &builtins {
            if !contract.category.is_observation() {
                return Err(IndexError::BuiltInCategoryNotObservation(
                    contract.category.clone(),
                ));
            }
        }
        for contract in &skills {
            match &contract.source {
                ContractSource::Skill { .. } => {}
                ContractSource::BuiltIn => {
                    return Err(IndexError::NotASkill {
                        id: contract.id.clone(),
                    });
                }
            }
        }
        let builtin_ids: Vec<ContractId> = builtins
            .iter()
            .map(|contract| contract.id.clone())
            .collect();
        let mut all = builtins;
        all.extend(skills);
        let index = Self::new(all)?;
        for id in builtin_ids {
            if index.get(&id).is_none() {
                return Err(IndexError::BuiltInMissing(id.clone()));
            }
        }
        Ok(index)
    }

    /// The contract with `id`.
    #[must_use]
    pub fn get(&self, id: &ContractId) -> Option<&CapabilityContract> {
        self.contracts.get(id)
    }

    /// The policy-facing summary of the contract with `id`.
    #[must_use]
    pub fn meta(&self, id: &ContractId) -> Option<ContractMeta> {
        let contract = self.contracts.get(id)?;
        Some(ContractMeta {
            id: contract.id.clone(),
            category: contract.category.clone(),
            effect: contract.effect,
            target_kind: contract.target_kind,
            safety_facts: contract.safety_facts.clone(),
            constrainable_args: contract.constrainable_args.clone(),
        })
    }

    /// The effect class every contract in `category` has, when the category is known.
    #[must_use]
    pub fn effect_of_category(&self, category: &ContractCategory) -> Option<EffectClass> {
        self.categories.get(category).copied()
    }

    /// Every contract, ordered by id.
    pub fn iter(&self) -> impl Iterator<Item = &CapabilityContract> {
        self.contracts.values()
    }

    /// How many contracts the index holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.contracts.len()
    }

    /// Whether the index holds no contracts.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.contracts.is_empty()
    }
}

/// Why a set of contracts is not a valid index.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IndexError {
    /// One contract failed validation.
    #[error(transparent)]
    Invalid(#[from] ContractError),
    /// The same id appeared twice.
    #[error("{0}: the contract id is used twice")]
    DuplicateId(ContractId),
    /// One category is used by two effect classes.
    #[error("{category}: category spans {first:?} and {second:?}")]
    CategorySpansEffects {
        /// The category that spans two classes.
        category: ContractCategory,
        /// The class of the first contract seen.
        first: EffectClass,
        /// The class of the conflicting contract.
        second: EffectClass,
    },
    /// The built-in read table composed empty, which cannot happen and would leave the
    /// evaluator with nothing it can name.
    #[error("the built-in read table is empty")]
    BuiltInsMissing,
    /// A built-in read's category does not end in the `observe` segment, so a policy rule
    /// cannot tell it apart from a family that changes state.
    #[error("{0}: a built-in read category must end in `observe`")]
    BuiltInCategoryNotObservation(ContractCategory),
    /// A contract in the skill list is not a skill, so the composition would hide a built-in
    /// among the skills.
    #[error("{id}: the skill list must hold skill contracts")]
    NotASkill {
        /// The contract that is not a skill.
        id: ContractId,
    },
    /// A built-in is not in the composed index, so the registry would not know a capability the
    /// model can call.
    #[error("{0}: a built-in contract is missing from the composition")]
    BuiltInMissing(ContractId),
}
