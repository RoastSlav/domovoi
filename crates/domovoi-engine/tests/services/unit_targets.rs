//! Resolving a unit: the identity comes only from the helper's answer, an alias binds the
//! canonical id, and nothing is guessed when the helper cannot say.

use std::sync::Arc;

use domovoi_core::capability::{
    TargetError, TargetName, TargetQuery, TargetUnresolved, canonical_host_identity,
};
use domovoi_core::host::KeyFingerprint;
use domovoi_core::ports::{HostRepo, LinkError, Presence, TargetResolver};
use domovoi_core::unit::{UnitOutputs, UnitRefusal};
use domovoi_core::{AgentId, AgentIdentity, HostId};
use domovoi_engine::Evidence;
use domovoi_engine::targets::TargetService;
use domovoi_testkit::build::{T0, host};
use domovoi_testkit::link::{evidence_caps, unit_caps};
use domovoi_testkit::{
    FakeAgentLink, FakeCapabilityRegistry, FakeClock, FakeUnitRelay, InMemoryStore,
    identity_outputs,
};

const KEY: [u8; 32] = [7; 32];

#[cfg(test)]
struct World {
    host: HostId,
    relay: Arc<FakeUnitRelay>,
    with_units: TargetService,
    without_units: TargetService,
}

#[cfg(test)]
async fn world() -> World {
    let nas = host("nas").expect("host");
    let store = Arc::new(InMemoryStore::new());
    let hosts: Arc<dyn HostRepo> = store.clone();
    hosts.upsert_host(&nas).await.expect("host stored");
    hosts
        .put_agent(&AgentIdentity {
            agent: AgentId::new(),
            host: nas.id,
            public_key: KEY,
            version: "0.1.0".to_owned(),
            enrolled_at: T0,
            revoked_at: None,
        })
        .await
        .expect("agent stored");
    let link = Arc::new(FakeAgentLink::new());
    link.set_presence(
        nas.id,
        Presence::Connected {
            since: T0,
            version: "0.1.0".to_owned(),
        },
    );
    let relay = Arc::new(FakeUnitRelay::new());
    let evidence = || {
        Arc::new(Evidence::new(
            evidence_caps(link.clone()),
            Arc::new(FakeClock::new(T0)),
            Arc::new(FakeCapabilityRegistry::following(link.clone())),
        ))
    };
    World {
        host: nas.id,
        relay: relay.clone(),
        with_units: TargetService::new(evidence(), hosts.clone())
            .with_units(unit_caps(link.clone(), relay)),
        without_units: TargetService::new(evidence(), hosts),
    }
}

#[cfg(test)]
fn name(text: &str) -> TargetName {
    TargetName::parse(text).expect("name")
}

#[tokio::test]
async fn alias_typed_name_binds_the_canonical_id() {
    let w = world().await;
    // The owner typed an alias; the helper answers with the canonical id and its hash.
    w.relay.set_identity(
        "alias.service",
        identity_outputs("example.service", 'c', "untested", true),
    );
    let unit = w
        .with_units
        .resolve_unit(w.host, &name("alias.service"))
        .await
        .expect("resolves");
    assert_eq!(unit.id.as_str(), "example.service");
    assert_eq!(unit.identity_hash.to_string(), "c".repeat(64));
    let key = KeyFingerprint::of_public_key(&KEY);
    assert_eq!(
        unit.target.identity.to_string(),
        format!("unit:{}/{key}/example.service", w.host)
    );
    assert_eq!(
        unit.target.name.as_str(),
        "alias.service",
        "the typed name is display only"
    );
    // The same query through `resolve` gives the same identity.
    let via_resolve = w
        .with_units
        .resolve(TargetQuery::Unit {
            host: w.host,
            name: name("alias.service"),
        })
        .await
        .expect("resolves");
    assert_eq!(via_resolve, unit.target);
    // The card carries what the approval shows, and the tier and opt-in.
    assert_eq!(unit.card.platform_tier, "untested");
    assert!(unit.card.write_opt_in);
    let text = unit.card.render("nas");
    assert!(text.starts_with("Restart example.service on nas."));
    assert!(text.contains("This platform is not release-tested"));
    // The host identity this was bound under is unchanged by a unit being resolved.
    assert!(canonical_host_identity(w.host, &key).is_ok());
}

#[tokio::test]
async fn a_helper_refusal_is_a_unit_refusal_with_its_sentence() {
    let w = world().await;
    w.relay.fail_identity(
        "example.service",
        LinkError::Rejected("target_stale".to_owned()),
    );
    let err = w
        .with_units
        .resolve_unit(w.host, &name("example.service"))
        .await
        .expect_err("refused");
    let TargetError::UnitRefused { refusal, .. } = err else {
        panic!("expected a unit refusal: {err:?}");
    };
    assert_eq!(refusal, UnitRefusal::TargetStale);
    assert!(refusal.describe().contains("systemctl daemon-reload"));

    w.relay.fail_identity(
        "other.service",
        LinkError::Rejected(
            "target_unsupported: location: /run/systemd/system/other.service".to_owned(),
        ),
    );
    let err = w
        .with_units
        .resolve_unit(w.host, &name("other.service"))
        .await
        .expect_err("refused");
    assert!(matches!(err, TargetError::UnitRefused { .. }), "{err:?}");
}

#[tokio::test]
async fn an_unreadable_or_unreachable_answer_proves_nothing() {
    let w = world().await;
    // Transport failure.
    w.relay.fail_identity("example.service", LinkError::Timeout);
    let err = w
        .with_units
        .resolve_unit(w.host, &name("example.service"))
        .await
        .expect_err("unresolved");
    assert!(matches!(
        err,
        TargetError::Unresolved {
            reason: TargetUnresolved::UnitIdentityUnproven,
            ..
        }
    ));
    // A reply that is missing the hash is `read_failed`, never a partial binding.
    let mut broken = identity_outputs("example.service", 'c', "supported", false).0;
    broken.remove("identity_hash");
    w.relay.set_identity("example.service", UnitOutputs(broken));
    let err = w
        .with_units
        .resolve_unit(w.host, &name("example.service"))
        .await
        .expect_err("refused");
    assert!(matches!(
        err,
        TargetError::UnitRefused {
            refusal: UnitRefusal::ReadFailed,
            ..
        }
    ));
}

#[tokio::test]
async fn without_a_helper_path_a_unit_never_resolves() {
    let w = world().await;
    w.relay.set_identity(
        "example.service",
        identity_outputs("example.service", 'c', "supported", false),
    );
    let err = w
        .without_units
        .resolve_unit(w.host, &name("example.service"))
        .await
        .expect_err("unresolved");
    assert!(matches!(
        err,
        TargetError::Unresolved {
            reason: TargetUnresolved::UnitIdentityUnproven,
            ..
        }
    ));
    assert!(w.relay.reads().is_empty(), "nothing was asked");
}

#[tokio::test]
async fn a_name_outside_the_unit_grammar_is_invalid_before_any_read() {
    let w = world().await;
    for bad in ["example.socket", "-rf.service", "a*.service"] {
        let err = w
            .with_units
            .resolve_unit(w.host, &name(bad))
            .await
            .expect_err("invalid");
        assert!(
            matches!(err, TargetError::InvalidName { .. }),
            "{bad}: {err:?}"
        );
    }
    assert!(w.relay.reads().is_empty());
}
