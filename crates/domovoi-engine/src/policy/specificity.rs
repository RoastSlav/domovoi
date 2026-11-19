//! The one definition of "more specific": a partial order over host, capability and target.

use domovoi_core::capability::{
    ContractCategory, ContractId, ContractIndex, ContractMeta, EffectClass,
};

use super::error::PolicyError;
use super::rule::{CapabilityScope, HostScope, Rule, TargetScope};

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
    Category {
        category: ContractCategory,
        effect: EffectClass,
    },
    Contract {
        id: ContractId,
        category: ContractCategory,
        effect: EffectClass,
    },
}

fn host_contains(outer: &HostScope, inner: &HostScope) -> bool {
    match (outer, inner) {
        (HostScope::All, _) => true,
        (HostScope::Host(a), HostScope::Host(b)) => a == b,
        (HostScope::Host(_), HostScope::All) => false,
    }
}

fn capability_contains(outer: &ResolvedCapability, inner: &ResolvedCapability) -> bool {
    use ResolvedCapability::{Any, Category, Contract, Effect};
    match (outer, inner) {
        (Any, _) => true,
        (Effect(e), Effect(i)) => e == i,
        (Effect(e), Category { effect, .. } | Contract { effect, .. }) => effect == e,
        (Category { category: c, .. }, Category { category, .. } | Contract { category, .. }) => {
            category == c
        }
        (Contract { id, .. }, Contract { id: other, .. }) => id == other,
        _ => false,
    }
}

fn targets_contain(outer: &TargetScope, inner: &TargetScope) -> bool {
    match (outer, inner) {
        (TargetScope::Any, _) => true,
        (TargetScope::Exact(a), TargetScope::Exact(b)) => b.is_subset(a),
        (TargetScope::Exact(_), TargetScope::Any) => false,
    }
}

/// Whether `outer` contains `inner` on every dimension, i.e. every request `inner` matches
/// `outer` matches too.
pub(crate) fn scope_contains(outer: &ResolvedScope, inner: &ResolvedScope) -> bool {
    host_contains(&outer.host, &inner.host)
        && capability_contains(&outer.capability, &inner.capability)
        && targets_contain(&outer.targets, &inner.targets)
}

/// `a` is strictly more specific than `b`: inside `b` on every dimension and properly inside
/// it on at least one. Conditions are never part of it.
pub(crate) fn is_strictly_more_specific(a: &ResolvedScope, b: &ResolvedScope) -> bool {
    scope_contains(b, a) && !scope_contains(a, b)
}

/// Two scopes intersect on every dimension.
///
/// Capabilities form a tree (contract inside category inside effect inside any), so two
/// capability scopes intersect exactly when one contains the other.
pub(crate) fn overlaps(a: &ResolvedScope, b: &ResolvedScope) -> bool {
    let hosts = host_contains(&a.host, &b.host) || host_contains(&b.host, &a.host);
    let caps = capability_contains(&a.capability, &b.capability)
        || capability_contains(&b.capability, &a.capability);
    let targets = match (&a.targets, &b.targets) {
        (TargetScope::Exact(x), TargetScope::Exact(y)) => x.intersection(y).next().is_some(),
        _ => true,
    };
    hosts && caps && targets
}

/// Normalises a rule against the contract of one request.
///
/// Returns `None` when the rule's capability scope does not contain the request's contract.
/// Host and target membership are checked by the matcher, which has the request.
pub(crate) fn resolve_against(rule: &Rule, meta: &ContractMeta) -> Option<ResolvedScope> {
    let capability = match &rule.capability {
        CapabilityScope::Any => ResolvedCapability::Any,
        CapabilityScope::Effect(e) if *e == meta.effect => ResolvedCapability::Effect(*e),
        CapabilityScope::Category(c) if *c == meta.category => ResolvedCapability::Category {
            category: c.clone(),
            effect: meta.effect,
        },
        CapabilityScope::Contract(id) if *id == meta.id => ResolvedCapability::Contract {
            id: id.clone(),
            category: meta.category.clone(),
            effect: meta.effect,
        },
        _ => return None,
    };
    Some(ResolvedScope {
        host: rule.host.clone(),
        capability,
        targets: rule.targets.clone(),
    })
}

/// Normalises a rule in the abstract, using the index for contract and category facts.
///
/// A rule naming a contract or category the index does not know is an error, never a guess.
pub(crate) fn resolve_in_index(
    rule: &Rule,
    index: &ContractIndex,
) -> Result<ResolvedScope, PolicyError> {
    let capability = match &rule.capability {
        CapabilityScope::Any => ResolvedCapability::Any,
        CapabilityScope::Effect(e) => ResolvedCapability::Effect(*e),
        CapabilityScope::Category(c) => ResolvedCapability::Category {
            category: c.clone(),
            effect: index
                .effect_of_category(c)
                .ok_or_else(|| PolicyError::CategoryNotInIndex(c.clone()))?,
        },
        CapabilityScope::Contract(id) => {
            let meta = index
                .meta(id)
                .ok_or_else(|| PolicyError::UnknownContract(id.clone()))?;
            ResolvedCapability::Contract {
                id: meta.id,
                category: meta.category,
                effect: meta.effect,
            }
        }
    };
    Ok(ResolvedScope {
        host: rule.host.clone(),
        capability,
        targets: rule.targets.clone(),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use domovoi_core::HostId;
    use domovoi_core::capability::TargetName;
    use proptest::prelude::*;

    use super::*;

    const HOST_A: &str = "018f0000-0000-7000-8000-000000000001";
    const HOST_B: &str = "018f0000-0000-7000-8000-000000000002";

    fn host(text: &str) -> HostScope {
        HostScope::Host(text.parse::<HostId>().unwrap())
    }

    fn category(text: &str) -> ContractCategory {
        ContractCategory::parse(text).unwrap()
    }

    fn contract(text: &str) -> ContractId {
        ContractId::parse(text).unwrap()
    }

    fn exact(names: &[&str]) -> TargetScope {
        TargetScope::Exact(
            names
                .iter()
                .map(|n| TargetName::parse(n).unwrap())
                .collect::<BTreeSet<_>>(),
        )
    }

    fn restart() -> ResolvedCapability {
        ResolvedCapability::Contract {
            id: contract("container.restart"),
            category: category("container.lifecycle"),
            effect: EffectClass::Mutate,
        }
    }

    fn destroy_category() -> ResolvedCapability {
        ResolvedCapability::Category {
            category: category("container.destroy"),
            effect: EffectClass::Destructive,
        }
    }

    /// Contract, category, effect, any: narrowest first, all containing `restart()`.
    fn chain() -> [ResolvedCapability; 4] {
        [
            restart(),
            ResolvedCapability::Category {
                category: category("container.lifecycle"),
                effect: EffectClass::Mutate,
            },
            ResolvedCapability::Effect(EffectClass::Mutate),
            ResolvedCapability::Any,
        ]
    }

    fn scope(
        host: HostScope,
        capability: ResolvedCapability,
        targets: TargetScope,
    ) -> ResolvedScope {
        ResolvedScope {
            host,
            capability,
            targets,
        }
    }

    #[test]
    fn host_scope_containment() {
        let (a, b) = (host(HOST_A), host(HOST_B));
        let cases = [
            (HostScope::All, HostScope::All, true),
            (HostScope::All, a.clone(), true),
            (a.clone(), a.clone(), true),
            (a.clone(), b.clone(), false),
            (a.clone(), HostScope::All, false),
        ];
        for (outer, inner, expected) in cases {
            assert_eq!(
                host_contains(&outer, &inner),
                expected,
                "{outer:?} / {inner:?}"
            );
        }
    }

    #[test]
    fn capability_rank_order_is_contract_category_effect_any() {
        let chain = chain();
        for (i, outer) in chain.iter().enumerate() {
            for (j, inner) in chain.iter().enumerate() {
                assert_eq!(
                    capability_contains(outer, inner),
                    i >= j,
                    "outer rank {i} over inner rank {j}"
                );
            }
        }
        let sibling = ResolvedCapability::Contract {
            id: contract("container.stop"),
            category: category("container.lifecycle"),
            effect: EffectClass::Mutate,
        };
        assert!(!capability_contains(&restart(), &sibling));
        assert!(!capability_contains(&sibling, &restart()));
        assert!(capability_contains(&chain[1], &sibling));
        assert!(!capability_contains(&chain[1], &destroy_category()));
        assert!(!capability_contains(&chain[2], &destroy_category()));
        assert!(capability_contains(
            &ResolvedCapability::Effect(EffectClass::Destructive),
            &destroy_category()
        ));
        assert!(!capability_contains(
            &ResolvedCapability::Effect(EffectClass::Read),
            &chain[1]
        ));
    }

    #[test]
    fn capability_rank_is_a_total_order_on_matches() {
        let chain = chain();
        for a in &chain {
            for b in &chain {
                assert!(
                    capability_contains(a, b) || capability_contains(b, a),
                    "{a:?} and {b:?} both match one contract, so one contains the other"
                );
            }
        }
    }

    #[test]
    fn target_exact_containment_is_strict_subset() {
        let one = exact(&["a"]);
        let two = exact(&["a", "b"]);
        assert!(targets_contain(&two, &one));
        assert!(!targets_contain(&one, &two));
        assert!(targets_contain(&one, &one), "equal sets contain each other");
        assert!(!targets_contain(&exact(&["a"]), &exact(&["b"])));
        assert!(targets_contain(&TargetScope::Any, &two));
        assert!(!targets_contain(&two, &TargetScope::Any));
        let narrow = scope(HostScope::All, restart(), one);
        let wide = scope(HostScope::All, restart(), two);
        assert!(is_strictly_more_specific(&narrow, &wide));
        assert!(!is_strictly_more_specific(&wide, &narrow));
        assert!(
            !is_strictly_more_specific(&narrow, &narrow),
            "never more specific than itself"
        );
    }

    #[test]
    fn incomparable_scopes_overlap_only_when_every_dimension_intersects() {
        let chain = chain();
        let base = scope(HostScope::All, chain[1].clone(), exact(&["a", "b"]));
        let shares_b = scope(HostScope::All, chain[0].clone(), exact(&["b", "c"]));
        let narrow_host = scope(host(HOST_A), chain[1].clone(), exact(&["a", "b"]));
        assert!(overlaps(&base, &shares_b), "targets share b");
        assert!(overlaps(&base, &narrow_host));

        let disjoint_targets = scope(HostScope::All, chain[0].clone(), exact(&["c"]));
        assert!(!overlaps(&base, &disjoint_targets));
        let on_a = scope(host(HOST_A), chain[1].clone(), TargetScope::Any);
        let on_b = scope(host(HOST_B), chain[1].clone(), TargetScope::Any);
        assert!(!overlaps(&on_a, &on_b));
        let lifecycle = scope(HostScope::All, chain[1].clone(), TargetScope::Any);
        let destroy = scope(HostScope::All, destroy_category(), TargetScope::Any);
        assert!(!overlaps(&lifecycle, &destroy));
        assert!(overlaps(
            &scope(HostScope::All, chain[3].clone(), exact(&["a"])),
            &scope(HostScope::All, chain[0].clone(), TargetScope::Any)
        ));
    }

    fn resolved_scope() -> impl Strategy<Value = ResolvedScope> {
        (0u8..3, 0usize..6, 0u8..4).prop_map(|(h, c, t)| {
            let hosts = match h {
                0 => HostScope::All,
                1 => host(HOST_A),
                _ => host(HOST_B),
            };
            let [contract, category, effect, any] = chain();
            let capability = match c {
                0 => contract,
                1 => category,
                2 => effect,
                3 => any,
                4 => destroy_category(),
                _ => ResolvedCapability::Effect(EffectClass::Read),
            };
            let targets = match t {
                0 => TargetScope::Any,
                1 => exact(&["a"]),
                2 => exact(&["b"]),
                _ => exact(&["a", "b"]),
            };
            scope(hosts, capability, targets)
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn specificity_is_antisymmetric(a in resolved_scope(), b in resolved_scope()) {
            prop_assert!(!(is_strictly_more_specific(&a, &b) && is_strictly_more_specific(&b, &a)));
            prop_assert!(!is_strictly_more_specific(&a, &a));
            if is_strictly_more_specific(&a, &b) {
                prop_assert!(scope_contains(&b, &a) && !scope_contains(&a, &b));
                prop_assert!(overlaps(&a, &b));
            }
        }

        #[test]
        fn specificity_is_transitive(a in resolved_scope(), b in resolved_scope(), c in resolved_scope()) {
            if is_strictly_more_specific(&a, &b) && is_strictly_more_specific(&b, &c) {
                prop_assert!(is_strictly_more_specific(&a, &c));
            }
        }
    }
}
