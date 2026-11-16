use super::*;

#[test]
fn plan_binding_is_a_closed_two_value_type() {
    assert_eq!(serde_json::to_string(&PlanBinding::V1).unwrap(), "\"v1\"");
    assert_eq!(serde_json::to_string(&PlanBinding::V2).unwrap(), "\"v2\"");
    assert!(
        serde_json::from_str::<PlanBinding>("\"v3\"").is_err(),
        "an unknown binding is refused rather than treated as v1"
    );
}

#[test]
fn a_failed_action_reports_its_code_and_message() {
    use domovoi_core::{IncidentId, Risk, UserId};
    use domovoi_testkit::approvals::human;
    use domovoi_testkit::build::{T0, action, host};

    let mut request = action(IncidentId::new(), host("nas").unwrap().id, Risk::Write).unwrap();
    domovoi_testkit::approvals::approve(&mut request, human(UserId::new()), T0).unwrap();
    let plan = request.plan_hash_for_approval();
    request.dispatch(plan, None, T0).unwrap();
    request
        .complete(
            Err("helper_unavailable: the helper is not reachable".to_owned()),
            T0,
        )
        .unwrap();

    let dto = ActionDto::from(&request);
    assert_eq!(dto.status, "failed");
    assert_eq!(
        dto.reason.as_deref(),
        Some("helper_unavailable: the helper is not reachable"),
        "the person sees the failure class and its message",
    );
}

#[test]
fn badges_and_verdicts_use_wire_names() {
    assert_eq!(verdict_name(Verdict::ActionNeeded), "ACTION_NEEDED");
    let card = |badge| Card {
        incident: domovoi_core::IncidentId::new(),
        host: domovoi_core::HostName::parse("nas").unwrap(),
        badge,
        headline: String::new(),
        bullets: Vec::new(),
        next_step: None,
        details: None,
        buttons: Vec::new(),
        plans: Vec::new(),
        resolved: false,
    };
    let verdict = CardDto::from(&card(CardBadge::Verdict(Verdict::Monitor)));
    assert_eq!(verdict.badge, "MONITOR");
    let severity = CardDto::from(&card(CardBadge::Severity(domovoi_core::Severity::Critical)));
    assert_eq!(severity.badge, "critical");
}
