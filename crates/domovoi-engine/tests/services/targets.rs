//! Target resolution: what identity a plan binds for a host, a container and a unit, and why
//! none of it is ever guessed.
//!
//! Every case drives a scripted agent and asserts that the only calls it saw were the two
//! Docker reads through the read boundary — never a tool call built by the test.

use std::sync::{Arc, Mutex};

use domovoi_core::capability::{
    TargetError, TargetName, TargetQuery, TargetUnresolved, validate_container_name,
};
use domovoi_core::host::KeyFingerprint;
use domovoi_core::ports::{HostRepo, Presence, TargetResolver, ToolOutput};
use domovoi_core::{AgentId, AgentIdentity, Host, HostId, ReadToolCall};
use domovoi_engine::Evidence;
use domovoi_engine::targets::TargetService;
use domovoi_testkit::build::{T0, host};
use domovoi_testkit::link::{LinkRequest, evidence_caps};
use domovoi_testkit::{FakeAgentLink, FakeCapabilityRegistry, FakeClock, InMemoryStore};

const ID_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ID_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// What the scripted agent lists and inspects.
#[derive(Clone)]
struct Containers {
    name: String,
    id: String,
    rows: usize,
}

/// A host with one enrolled agent, the scripted link and the resolver over them.
struct World {
    host: Host,
    store: Arc<InMemoryStore>,
    link: Arc<FakeAgentLink>,
    containers: Arc<Mutex<Containers>>,
    resolver: TargetService,
}

impl World {
    async fn new(public_key: [u8; 32]) -> Self {
        let nas = match host("nas") {
            Ok(host) => host,
            Err(err) => panic!("{err}"),
        };
        let store = Arc::new(InMemoryStore::new());
        let hosts: Arc<dyn HostRepo> = store.clone();
        hosts
            .upsert_host(&nas)
            .await
            .unwrap_or_else(|err| panic!("{err}"));
        hosts
            .put_agent(&AgentIdentity {
                agent: AgentId::new(),
                host: nas.id,
                public_key,
                version: "0.1.0".to_owned(),
                enrolled_at: T0,
                revoked_at: None,
            })
            .await
            .unwrap_or_else(|err| panic!("{err}"));
        let link = Arc::new(FakeAgentLink::new());
        link.set_presence(
            nas.id,
            Presence::Connected {
                since: T0,
                version: "0.1.0".to_owned(),
            },
        );
        let containers = Arc::new(Mutex::new(Containers {
            name: "plex".to_owned(),
            id: ID_A.to_owned(),
            rows: 1,
        }));
        let listed = containers.clone();
        link.on_tool(move |_host, call| match call {
            ReadToolCall::DockerPs { .. } => {
                let state = match listed.lock() {
                    Ok(state) => state.clone(),
                    Err(err) => panic!("{err}"),
                };
                let mut text = "name\timage\tstate\thealth\trestarts".to_owned();
                for _ in 0..state.rows {
                    text.push('\n');
                    text.push_str(&format!("{}\tplexinc/pms\tUp\thealthy\t0", state.name));
                }
                Ok(ToolOutput {
                    text,
                    truncated: false,
                })
            }
            ReadToolCall::DockerInspect { .. } => {
                let state = match listed.lock() {
                    Ok(state) => state.clone(),
                    Err(err) => panic!("{err}"),
                };
                Ok(ToolOutput {
                    text: format!("{{\"Id\": \"{}\"}}", state.id),
                    truncated: false,
                })
            }
            other => panic!("the resolver read {other:?}, which it must not"),
        });
        let clock = Arc::new(FakeClock::new(T0));
        let evidence = Arc::new(Evidence::new(
            evidence_caps(link.clone()),
            clock,
            Arc::new(FakeCapabilityRegistry::following(link.clone())),
        ));
        Self {
            resolver: TargetService::new(evidence, hosts),
            host: nas,
            store,
            link,
            containers,
        }
    }

    fn host_id(&self) -> HostId {
        self.host.id
    }

    fn container_query(&self, name: &str) -> TargetQuery {
        TargetQuery::Container {
            host: self.host_id(),
            name: TargetName::parse(name).unwrap_or_else(|err| panic!("{err}")),
        }
    }

    /// The Docker reads the resolver made, in order.
    fn reads(&self) -> Vec<String> {
        self.link
            .requests()
            .into_iter()
            .filter_map(|request| match request {
                LinkRequest::Tool { call, .. } => Some(format!("{call:?}")),
                _ => None,
            })
            .collect()
    }

    fn set(&self, name: &str, id: &str, rows: usize) {
        match self.containers.lock() {
            Ok(mut state) => {
                *state = Containers {
                    name: name.to_owned(),
                    id: id.to_owned(),
                    rows,
                }
            }
            Err(err) => panic!("{err}"),
        }
    }

    /// Revokes the current agent and enrols a new one with `public_key`, as a re-enrolment
    /// does.
    async fn reenrol(&self, public_key: [u8; 32]) {
        let hosts: Arc<dyn HostRepo> = self.store.clone();
        if let Some(current) = hosts
            .live_agent(self.host_id())
            .await
            .unwrap_or_else(|err| panic!("{err}"))
        {
            hosts
                .revoke_agent(current.agent, T0)
                .await
                .unwrap_or_else(|err| panic!("{err}"));
        }
        hosts
            .put_agent(&AgentIdentity {
                agent: AgentId::new(),
                host: self.host_id(),
                public_key,
                version: "0.1.0".to_owned(),
                enrolled_at: T0,
                revoked_at: None,
            })
            .await
            .unwrap_or_else(|err| panic!("{err}"));
    }
}

#[tokio::test]
async fn container_name_is_validated_before_any_agent_call() {
    let world = World::new([7; 32]).await;

    let refused = world
        .resolver
        .resolve(world.container_query("plex/1"))
        .await;

    assert!(
        matches!(refused, Err(TargetError::InvalidName { .. })),
        "{refused:?}"
    );
    assert!(
        world.reads().is_empty(),
        "an invalid name reached the agent: {:?}",
        world.reads()
    );
}

#[tokio::test]
async fn vanished_container_is_unresolved() {
    let world = World::new([7; 32]).await;
    world.set("jellyfin", ID_A, 1);

    let missing = world.resolver.resolve(world.container_query("plex")).await;
    assert!(
        matches!(
            missing,
            Err(TargetError::Unresolved {
                reason: TargetUnresolved::ContainerVanished,
                ..
            })
        ),
        "{missing:?}"
    );
}

#[tokio::test]
async fn ambiguous_container_name_is_not_chosen() {
    let world = World::new([7; 32]).await;
    world.set("plex", ID_A, 2);

    let ambiguous = world.resolver.resolve(world.container_query("plex")).await;

    assert!(
        matches!(ambiguous, Err(TargetError::Ambiguous { count: 2, .. })),
        "{ambiguous:?}"
    );
    assert!(
        world.reads().iter().all(|read| read.contains("DockerPs")),
        "an ambiguous name was inspected anyway: {:?}",
        world.reads()
    );
}

#[tokio::test]
async fn unit_target_is_unresolved() {
    let world = World::new([7; 32]).await;
    let query = TargetQuery::Unit {
        host: world.host_id(),
        name: TargetName::parse("nginx.service").unwrap_or_else(|err| panic!("{err}")),
    };

    let refused = world.resolver.resolve(query).await;

    assert!(
        matches!(
            refused,
            Err(TargetError::Unresolved {
                reason: TargetUnresolved::UnitIdentityUnproven,
                ..
            })
        ),
        "{refused:?}"
    );
    assert!(
        world.reads().is_empty(),
        "a unit resolution read the host: {:?}",
        world.reads()
    );
}

#[tokio::test]
async fn host_target_identity_includes_enrolled_key_fingerprint() {
    let world = World::new([7; 32]).await;

    let resolved = world
        .resolver
        .resolve(TargetQuery::Host {
            host: world.host_id(),
        })
        .await
        .unwrap_or_else(|err| panic!("{err}"));

    let key = KeyFingerprint::of_public_key(&[7; 32]);
    assert_eq!(resolved.host_key, key);
    assert_eq!(
        resolved.identity.as_str(),
        format!("host:{}/{key}", world.host_id())
    );
    assert!(
        world.reads().is_empty(),
        "a host identity was read from the agent"
    );
}

#[tokio::test]
async fn host_reenrollment_changes_target_identity() {
    let world = World::new([7; 32]).await;
    let query = TargetQuery::Host {
        host: world.host_id(),
    };
    let first = world
        .resolver
        .resolve(query.clone())
        .await
        .unwrap_or_else(|err| panic!("{err}"));

    world.reenrol([9; 32]).await;
    let second = world
        .resolver
        .resolve(query)
        .await
        .unwrap_or_else(|err| panic!("{err}"));

    assert_ne!(
        first.identity, second.identity,
        "re-enrolling the agent left the old identity valid"
    );
    assert_ne!(first.host_key, second.host_key);
}

#[tokio::test]
async fn container_target_identity_changes_on_host_reenrollment() {
    let world = World::new([7; 32]).await;
    let query = world.container_query("plex");
    let first = world
        .resolver
        .resolve(query.clone())
        .await
        .unwrap_or_else(|err| panic!("{err}"));

    world.reenrol([9; 32]).await;
    let second = world
        .resolver
        .resolve(query)
        .await
        .unwrap_or_else(|err| panic!("{err}"));

    assert_ne!(
        first.identity, second.identity,
        "a container identity survived a host re-enrolment"
    );
}

#[tokio::test]
async fn target_identity_change_refused() {
    let world = World::new([7; 32]).await;
    let bound = world
        .resolver
        .resolve(world.container_query("plex"))
        .await
        .unwrap_or_else(|err| panic!("{err}"));

    // The container is recreated: the name stays, the id does not.
    world.set("plex", ID_B, 1);
    let recreated = world
        .resolver
        .resolve(world.container_query("plex"))
        .await
        .unwrap_or_else(|err| panic!("{err}"));
    assert_ne!(
        bound.identity, recreated.identity,
        "a recreate kept the identity a plan was bound to"
    );

    // A rename keeps the id, so the identity is unchanged.
    world.set("renamed", ID_B, 1);
    let renamed = world
        .resolver
        .resolve(world.container_query("renamed"))
        .await
        .unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(
        recreated.identity, renamed.identity,
        "a rename changed the bound identity"
    );
}

#[tokio::test]
async fn host_target_without_enrolled_identity_is_unresolved() {
    let world = World::new([7; 32]).await;
    let hosts: Arc<dyn HostRepo> = world.store.clone();
    let agent = hosts
        .live_agent(world.host_id())
        .await
        .unwrap_or_else(|err| panic!("{err}"))
        .map(|agent| agent.agent)
        .unwrap_or_default();
    hosts
        .revoke_agent(agent, T0)
        .await
        .unwrap_or_else(|err| panic!("{err}"));

    let refused = world
        .resolver
        .resolve(TargetQuery::Host {
            host: world.host_id(),
        })
        .await;

    assert!(
        matches!(
            refused,
            Err(TargetError::Unresolved {
                reason: TargetUnresolved::HostIdentityUnproven,
                ..
            })
        ),
        "{refused:?}"
    );
}

#[tokio::test]
async fn host_target_resolves_while_the_agent_is_disconnected() {
    let world = World::new([7; 32]).await;
    world.link.set_presence(
        world.host_id(),
        Presence::Disconnected {
            last_seen: Some(T0),
        },
    );

    let resolved = world
        .resolver
        .resolve(TargetQuery::Host {
            host: world.host_id(),
        })
        .await;

    assert!(
        resolved.is_ok(),
        "the enrolment record needs no session: {resolved:?}"
    );
}

#[tokio::test]
async fn container_identity_is_within_the_target_identity_limit() {
    let world = World::new([7; 32]).await;

    let resolved = world
        .resolver
        .resolve(world.container_query("plex"))
        .await
        .unwrap_or_else(|err| panic!("{err}"));

    assert!(resolved.identity.as_str().starts_with("container:"));
    assert!(
        resolved.identity.as_str().len() <= 310,
        "the identity is longer than a target identity may be"
    );
    assert!(
        validate_container_name("plex").is_ok(),
        "the fixture name is outside the grammar"
    );
}
