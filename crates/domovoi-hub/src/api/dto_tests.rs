use super::*;

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
