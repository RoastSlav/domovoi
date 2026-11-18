//! A validated set of contracts the policy evaluator and the inventory can share.

use std::collections::BTreeMap;

use super::contract::{CapabilityContract, ConstrainableArg, ContractError};
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
    /// The whole set is refused when any contract is invalid, so a partially built index
    /// never exists.
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
}
