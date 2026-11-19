//! How a step becomes evaluator inputs, and what is recorded when an input is unavailable.

use domovoi_core::HostId;
use domovoi_core::capability::CeilingSummary;
use domovoi_engine::policy::{Condition, FactState, Outcome, Rule};
use domovoi_engine::policy_shadow::{
    CeilingReading, GapInput, GapReason, InputGap, InputsEnvelope, Verdict, build, decode_inputs,
    request_from_inputs,
};
use uuid::Uuid;

use crate::policy_fixtures::fact;
use crate::policy_shadow_support::{
    candidate, container_pin, env, host, named_rule, rule, skill_candidate, t,
};

fn gap(input: GapInput, reason: GapReason) -> InputGap {
    InputGap { input, reason }
}

#[tokio::test]
async fn build_skips_package_contracts() {
    let built = build(&candidate("agent.package.install", 0), &[], &env(), t(1)).await;
    assert_eq!(built.verdict, Verdict::Skip("package_contract"));
    assert!(built.request.is_none() && built.inputs.is_none());
}

#[tokio::test]
async fn build_marks_unknown_contract_unevaluable() {
    let built = build(&candidate("skill.never_announced", 0), &[], &env(), t(1)).await;
    assert_eq!(built.verdict, Verdict::Unevaluable("contract_unknown"));
    assert!(built.gaps.is_empty());
}

#[tokio::test]
async fn build_marks_unresolved_host_unevaluable() {
    let env = env();
    env.fail_host(host());
    let built = build(&skill_candidate(), &[], &env, t(1)).await;
    assert_eq!(built.verdict, Verdict::Unevaluable("target_unresolved"));
}

#[tokio::test]
async fn build_records_every_unavailable_input_as_a_gap() {
    let requires = Rule {
        conditions: vec![Condition::RequiresFact {
            fact: fact("host.not_in_maintenance"),
            expected: true,
        }],
        ..rule("needs-fact", Outcome::Allow)
    };
    let built = build(&skill_candidate(), &[requires], &env(), t(1)).await;
    assert_eq!(built.verdict, Verdict::Evaluate);
    assert_eq!(
        built.gaps,
        vec![
            gap(GapInput::Ceiling, GapReason::NotProducedYet),
            gap(GapInput::Facts, GapReason::NotProducedYet),
            gap(GapInput::Autonomy, GapReason::NotProducedYet),
        ]
    );
    let request = built.request.unwrap();
    assert!(request.ceiling.is_none());
    assert!(request.facts.is_empty(), "an absent fact stays absent");
    assert!(!request.autonomy.paused && !request.autonomy.circuit_broken);
    insta::assert_snapshot!(
        "input_gaps",
        serde_json::to_string_pretty(&built.gaps).unwrap()
    );
}

#[tokio::test]
async fn unknown_root_state_is_recorded_as_root() {
    let env = env();
    env.set_runs_as_root(host(), None);
    let built = build(&skill_candidate(), &[], &env, t(1)).await;
    assert!(built.request.unwrap().runs_as_root);
    assert!(
        built
            .gaps
            .contains(&gap(GapInput::RunsAsRoot, GapReason::HostNotConnected))
    );
}

#[tokio::test]
async fn pinned_identities_come_from_resolvable_hosts_only() {
    let env = env();
    let other = HostId::from_uuid(Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_00b2));
    env.fail_host(other);
    let mut step = skill_candidate();
    step.pinned.push(domovoi_core::PinnedSubject {
        host: other,
        ..step.pinned[0].clone()
    });
    let built = build(&step, &[], &env, t(1)).await;
    assert_eq!(built.request.as_ref().unwrap().pinned.len(), 1);
    let unresolvable = gap(GapInput::PinnedTargets, GapReason::Unresolvable);
    assert_eq!(built.gaps.iter().filter(|g| **g == unresolvable).count(), 1);
    assert!(
        !built
            .gaps
            .contains(&gap(GapInput::PinnedTargets, GapReason::NotProducedYet))
    );

    step.pinned.push(container_pin());
    step.pinned.push(container_pin());
    let built = build(&step, &[], &env, t(1)).await;
    let not_yet = gap(GapInput::PinnedTargets, GapReason::NotProducedYet);
    assert_eq!(built.gaps.iter().filter(|g| **g == not_yet).count(), 1);
}

#[tokio::test]
async fn host_name_resolves_for_rules_that_name_it() {
    let env = env();
    env.set_host_name(host(), "nas");
    let named = build(
        &skill_candidate(),
        &[named_rule("by-name", "nas", Outcome::Allow)],
        &env,
        t(1),
    )
    .await;
    let request = named.request.unwrap();
    assert_eq!(request.rule_targets.len(), 1);
    assert_eq!(
        request.rule_targets.values().next(),
        Some(&request.target_identity)
    );
    let unnamed = build(&skill_candidate(), &[], &env, t(1)).await;
    assert!(unnamed.request.unwrap().rule_targets.is_empty());
}

#[tokio::test]
async fn recorded_inputs_round_trip_and_rebuild_the_same_request() {
    let built = build(&skill_candidate(), &[], &env(), t(0)).await;
    let (request, inputs) = (built.request.unwrap(), built.inputs.unwrap());
    let text = serde_json::to_string(&InputsEnvelope::new(inputs.clone())).unwrap();
    let decoded = decode_inputs(&text).unwrap();
    assert_eq!(decoded, inputs);
    assert_eq!(request_from_inputs(&decoded, None).unwrap(), request);
    assert!(decode_inputs(&text.replace("\"v\":1", "\"v\":2")).is_err());
    insta::assert_snapshot!(
        "recorded_inputs",
        serde_json::to_string_pretty(&InputsEnvelope::new(inputs)).unwrap()
    );
}

#[tokio::test]
async fn reported_ceiling_and_known_facts_are_used_as_given() {
    let env = env();
    let summary = CeilingSummary {
        enabled: true,
        entries: Vec::new(),
        hash: Some("h1".to_owned()),
    };
    env.set_ceiling(host(), CeilingReading::Reported(summary.clone()));
    env.set_fact(host(), "host.not_in_maintenance", FactState::Known(true));
    let requires = Rule {
        conditions: vec![Condition::RequiresFact {
            fact: fact("host.not_in_maintenance"),
            expected: true,
        }],
        ..rule("needs-fact", Outcome::Allow)
    };
    let built = build(&skill_candidate(), &[requires], &env, t(1)).await;
    assert_eq!(
        built.gaps,
        vec![gap(GapInput::Autonomy, GapReason::NotProducedYet)]
    );
    let mut request = built.request.unwrap();
    assert_eq!(request.ceiling, Some(summary));
    assert_eq!(request.facts.len(), 1);
    // A replay expects the ceiling hash it recorded, which changes no result.
    let replayed = request_from_inputs(&built.inputs.unwrap(), None).unwrap();
    request.expected_ceiling_hash = Some("h1".to_owned());
    assert_eq!(replayed, request);
}
