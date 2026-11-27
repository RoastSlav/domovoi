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
        status: None,
    };
    let verdict = CardDto::from(&card(CardBadge::Verdict(Verdict::Monitor)));
    assert_eq!(verdict.badge, "MONITOR");
    let severity = CardDto::from(&card(CardBadge::Severity(domovoi_core::Severity::Critical)));
    assert_eq!(severity.badge, "critical");
}

#[test]
fn host_package_rows_carry_the_agents_skill_state() {
    use domovoi_proto::messages::PackageSkillState;
    let row = crate::api::dto_packages::HostPackageDto {
        name: "web-tools".to_owned(),
        version: "1.0.0".to_owned(),
        key_id: String::new(),
        archive_sha256: String::new(),
        installed_at: String::new(),
        status: "active".to_owned(),
        key_state: "trusted".to_owned(),
        skills: vec!["a".to_owned()],
        privilege_boundary: "in_effect".to_owned(),
        observed_at: String::new(),
        observed_by: "job".to_owned(),
        skills_enabled: None,
        skills_off_reason: None,
        skills_live: None,
    };
    let report = |skills: &[PackageSkillState]| crate::agent_server::PackageReport {
        skills: skills.to_vec(),
        live: false,
    };
    let off = PackageSkillState {
        package: "web-tools".to_owned(),
        version: "1.0.0".to_owned(),
        skill: "a".to_owned(),
        enabled: false,
        reason: Some("stored, not enabled".to_owned()),
    };
    let shown = row
        .clone()
        .with_skill_states(&report(std::slice::from_ref(&off)));
    assert_eq!(shown.skills_live, Some(false));
    assert_eq!(shown.skills_enabled, Some(false));
    assert_eq!(
        shown.skills_off_reason.as_deref(),
        Some("stored, not enabled")
    );
    let on = PackageSkillState {
        enabled: true,
        reason: None,
        ..off.clone()
    };
    assert_eq!(
        row.clone().with_skill_states(&report(&[on])).skills_enabled,
        Some(true)
    );
    let other = PackageSkillState {
        package: "other".to_owned(),
        ..off
    };
    assert_eq!(
        row.with_skill_states(&report(&[other])).skills_enabled,
        None,
        "unknown to the agent"
    );
}
