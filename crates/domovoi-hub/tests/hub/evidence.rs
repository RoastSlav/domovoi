//! The hub's read path through the evidence boundary: the capability registry the engine gets
//! carries the per-host skill switch, and a skill a person switched off never reaches the
//! agent.

use std::collections::BTreeMap;
use std::sync::Arc;

use domovoi_core::ports::Presence;
use domovoi_core::{ReadToolCall, Risk, SkillName, SkillSummary};
use domovoi_engine::{Evidence, EvidenceError, EvidenceRequest};
use domovoi_hub::runtime::caps::evidence_caps;
use domovoi_testkit::FakeCapabilityRegistry;
use domovoi_testkit::build::manifest;
use jiff::SignedDuration;

use crate::api_harness::api;

/// The skill a person switches off in the UI, as the agent announced it.
fn read_skill() -> SkillSummary {
    manifest("listening_ports", Risk::Read)
        .unwrap()
        .summary(domovoi_core::ScriptDigest::of_bytes(b"fixture entry"))
}

#[tokio::test]
async fn a_switched_off_skill_is_refused_with_the_hubs_words() {
    let a = api().await;
    a.link.set_presence(
        a.nas.id,
        Presence::Connected {
            since: a.now(),
            version: "0.6.0".to_owned(),
        },
    );
    a.link.set_skills(a.nas.id, vec![read_skill()]);
    let skill = SkillName::parse("listening_ports").unwrap();
    let capabilities = Arc::new(FakeCapabilityRegistry::following(a.link.clone()));
    capabilities.disable(a.nas.id, skill.clone());
    let evidence = Evidence::new(evidence_caps(a.link.clone()), a.clock.clone(), capabilities);

    let call = ReadToolCall::RunReadSkill {
        skill,
        args: BTreeMap::new(),
    };
    let request =
        EvidenceRequest::for_host(a.nas.id, call, a.now() + SignedDuration::from_secs(15));
    let refused = evidence.collect_evidence(request).await.unwrap_err();
    assert_eq!(
        refused,
        EvidenceError::NotEnabled("the skill is disabled on this host".to_owned()),
        "a switched-off skill is refused with the hub's own words"
    );
    assert!(
        a.link.requests().is_empty(),
        "a switched-off skill never reached the agent"
    );
}
