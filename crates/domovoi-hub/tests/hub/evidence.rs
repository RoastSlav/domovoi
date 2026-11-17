//! The hub's read path through the evidence boundary: the link the engine gets is the one
//! with the per-host skill policy applied, and a skill a person switched off never reaches
//! the agent.

use std::collections::BTreeMap;
use std::sync::Arc;

use domovoi_core::ports::Presence;
use domovoi_core::{ReadToolCall, Risk, SkillName, SkillSummary};
use domovoi_engine::{Evidence, EvidenceError, EvidenceRequest};
use domovoi_hub::agent_server::{PolicyLink, SkillPolicy};
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
async fn a_switched_off_skill_is_refused_as_one_the_host_does_not_have() {
    let a = api().await;
    a.link.set_presence(
        a.nas.id,
        Presence::Connected {
            since: a.now(),
            version: "0.6.0".to_owned(),
        },
    );
    a.link.set_skills(a.nas.id, vec![read_skill()]);
    let policy = SkillPolicy::load(a.store.clone(), std::slice::from_ref(&a.nas))
        .await
        .unwrap();
    let skill = SkillName::parse("listening_ports").unwrap();
    policy.set(&a.nas, skill.clone(), false).await.unwrap();
    let link = Arc::new(PolicyLink::wrap(a.link.clone(), Arc::new(policy)));
    let evidence = Evidence::new(link);

    let call = ReadToolCall::RunReadSkill {
        skill,
        args: BTreeMap::new(),
    };
    let request =
        EvidenceRequest::for_host(a.nas.id, call, a.now() + SignedDuration::from_secs(15));
    let refused = evidence.collect_evidence(request).await.unwrap_err();
    assert_eq!(
        refused,
        EvidenceError::NotEnabled(
            "skill `listening_ports` is not installed on this host".to_owned()
        ),
        "the policy hides the skill, so the boundary answers as it always has"
    );
    assert!(
        a.link.requests().is_empty(),
        "a switched-off skill never reached the agent"
    );
}
