//! One task applies every action decision: two decisions at once leave exactly one
//! outcome, and the expiry sweep only reads proposals whose approval window closed.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use domovoi_core::ports::{ActionFilter, ActionRepo, Clock, Page, Paged, StoreError};
use domovoi_core::{
    ActionError, ActionId, ActionRequest, ActionStatus, HostId, IncidentId, Principal, SkillName,
    UserId,
};
use domovoi_engine::actions::audit;
use domovoi_engine::{
    ActionDeps, ActionHandle, ActionService, ActionSettings, ApprovalError, EnginePolicy,
    IncidentManager, ManagerDeps, PatternRedactor,
};
use domovoi_testkit::approvals::human;
use domovoi_testkit::build::T0;
use domovoi_testkit::{FakeAgentLink, FakeClock, InMemoryStore};
use jiff::{SignedDuration, Timestamp};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::actions_harness::{actions, verified_restart};

/// `DUE` is above the store page limit (500) so the sweep has to keep paging.
const DUE: usize = 501;
const DECIDED: usize = 300;
const OPEN: usize = 300;

struct SpyStore {
    store: Arc<InMemoryStore>,
    lists: Arc<Mutex<Vec<ActionFilter>>>,
}

#[async_trait]
impl ActionRepo for SpyStore {
    async fn insert(&self, action: &ActionRequest) -> Result<(), StoreError> {
        ActionRepo::insert(&*self.store, action).await
    }

    async fn update_from(&self, action: &ActionRequest, expected: &str) -> Result<(), StoreError> {
        ActionRepo::update_from(&*self.store, action, expected).await
    }

    async fn get(&self, id: ActionId) -> Result<Option<ActionRequest>, StoreError> {
        ActionRepo::get(&*self.store, id).await
    }

    async fn list(
        &self,
        filter: &ActionFilter,
        page: &Page,
    ) -> Result<Paged<ActionRequest>, StoreError> {
        {
            let mut lists = self.lists.lock().unwrap_or_else(|err| err.into_inner());
            lists.push(filter.clone());
        }
        ActionRepo::list(&*self.store, filter, page).await
    }

    async fn find_by_chat_token_hash(
        &self,
        hash: &[u8; 32],
    ) -> Result<Option<ActionRequest>, StoreError> {
        ActionRepo::find_by_chat_token_hash(&*self.store, hash).await
    }

    async fn last_run_by_skill(
        &self,
        host: HostId,
    ) -> Result<BTreeMap<SkillName, Timestamp>, StoreError> {
        ActionRepo::last_run_by_skill(&*self.store, host).await
    }
}

struct Swept {
    pub store: Arc<InMemoryStore>,
    pub clock: Arc<FakeClock>,
    pub service: ActionHandle,
    pub shutdown: CancellationToken,
}

impl Drop for Swept {
    fn drop(&mut self) {
        self.shutdown.cancel()
    }
}

async fn swept(settings: ActionSettings, lists: Arc<Mutex<Vec<ActionFilter>>>) -> Swept {
    let store = Arc::new(InMemoryStore::new());
    let clock = Arc::new(FakeClock::new(T0));
    let (bus, _events) = broadcast::channel(64);
    let shutdown = CancellationToken::new();
    let (manager, _manager_task) = IncidentManager::spawn(ManagerDeps {
        incidents: store.clone(),
        uow: store.clone(),
        versions: store.clone(),
        acks: store.clone(),
        clock: clock.clone(),
        redactor: Arc::new(PatternRedactor::default()),
        policy: EnginePolicy::default(),
        bus: bus.clone(),
        shutdown: shutdown.clone(),
    });
    let actions: Arc<dyn ActionRepo> = Arc::new(SpyStore {
        store: store.clone(),
        lists,
    });
    let (service, _task) = ActionService::spawn(
        ActionDeps {
            actions,
            incidents: store.clone(),
            audit: store.clone(),
            uow: store.clone(),
            link: Arc::new(FakeAgentLink::new()),
            redactor: Arc::new(PatternRedactor::default()),
            clock: clock.clone(),
            manager,
            bus: bus.clone(),
            shutdown: shutdown.clone(),
        },
        settings,
    );
    Swept {
        store,
        clock,
        service,
        shutdown,
    }
}

impl Swept {
    pub fn now(&self) -> Timestamp {
        Clock::now(&*self.clock)
    }

    pub async fn get(&self, id: ActionId) -> ActionRequest {
        ActionRepo::get(&*self.store, id).await.unwrap().unwrap()
    }

    pub async fn save(&self, action: &ActionRequest) {
        ActionRepo::insert(&*self.store, action).await.unwrap()
    }
}

#[tokio::test]
async fn approve_and_deny_at_once_leave_exactly_one_outcome() {
    let a = actions(ActionSettings::default()).await;
    let (action, _) = a.propose().await;
    let id = action.id();

    let approved = tokio::spawn(approve(a.service.clone(), id));
    let denied = tokio::spawn(deny(a.service.clone(), id));
    let approved = approved.await.unwrap();
    let denied = denied.await.unwrap();

    assert_ne!(
        approved.is_ok(),
        denied.is_ok(),
        "exactly one decision is applied"
    );

    let (applied, refused) = if approved.is_ok() {
        (approved.ok(), denied.err())
    } else {
        (denied.ok(), approved.err())
    };
    assert!(
        matches!(
            refused,
            Some(ApprovalError::Refused(ActionError::NotAllowed { .. }))
        ),
        "the other decision is refused because the action already has an outcome",
    );

    let won = matches!(applied, Some(ActionStatus::Approved));
    let trail = a.trail(id).await;
    let audited: Vec<String> = trail
        .iter()
        .filter(|kind| *kind == audit::APPROVED || *kind == audit::DENIED)
        .map(|kind| kind.to_owned())
        .collect();
    assert_eq!(
        audited,
        [(if won { audit::APPROVED } else { audit::DENIED }).to_owned()],
        "only the winning decision is audited",
    );
}

#[tokio::test]
async fn expiry_never_overwrites_an_approval() {
    let a = actions(ActionSettings::default()).await;
    let (action, _) = a.propose().await;
    let id = action.id();

    let approved = a
        .service
        .approve(id, human(UserId::new()), None)
        .await
        .unwrap();
    assert!(matches!(approved.status(), ActionStatus::Approved));

    a.clock.advance(SignedDuration::from_hours(2));
    let swept = a.service.expire_due().await.unwrap();
    assert_eq!(swept, 0, "a decided action is never swept");

    let trail = a.trail(id).await;
    assert!(
        trail.iter().all(|kind| *kind != audit::EXPIRED),
        "expiry never rewrites an approval",
    );
    assert_ne!(
        a.get(id).await.status(),
        &ActionStatus::Expired,
        "the approval is still stored"
    );
}

/// Where a spy store keeps the listings it was asked for.
fn new_lists() -> Arc<Mutex<Vec<ActionFilter>>> {
    Arc::new(Mutex::new(Vec::new()))
}

/// What `restart_container` needs to restart `nginx`.
fn restart_params() -> BTreeMap<String, String> {
    let pairs: [(&str, &str); 1] = [("container", "nginx")];
    let params: BTreeMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    params
}

#[tokio::test]
async fn expire_due_reads_only_due_proposals() {
    let lists = new_lists();
    let a = swept(ActionSettings::default(), lists.clone()).await;
    let (incident, host) = (IncidentId::new(), HostId::new());
    let skill = verified_restart();
    let args = skill.validate_args(&restart_params()).unwrap();
    let open_for = |ttl: SignedDuration, at: Timestamp| {
        ActionRequest::propose(
            incident,
            host,
            &skill,
            &args,
            Principal::System("triage".into()),
            ttl,
            at,
        )
    };

    for _ in 0..DUE {
        a.save(&open_for(SignedDuration::from_mins(1), T0)).await
    }

    let mut decided = open_for(SignedDuration::from_mins(1), T0);
    decided.deny(T0).unwrap();
    let decided_id = decided.id();
    a.save(&decided).await;
    for _ in 1..DECIDED {
        let mut other = open_for(SignedDuration::from_mins(1), T0);
        other.deny(T0).unwrap();
        a.save(&other).await
    }

    a.clock.advance(SignedDuration::from_mins(2));

    let opened = a.now();
    let still_open = open_for(SignedDuration::from_hours(1), opened);
    let open_id = still_open.id();
    a.save(&still_open).await;
    for _ in 1..OPEN {
        a.save(&open_for(SignedDuration::from_hours(1), opened))
            .await
    }

    let expired = a.service.expire_due().await.unwrap();
    assert_eq!(
        expired, DUE,
        "only proposals whose window closed are decided"
    );

    let due_at = a.now();
    let expected = ActionFilter {
        status: Some("proposed".to_owned()),
        expires_before: Some(
            due_at
                .checked_add(SignedDuration::from_millis(1))
                .unwrap_or(due_at),
        ),
        ..ActionFilter::default()
    };
    {
        let asked = lists.lock().unwrap_or_else(|err| err.into_inner());
        let sweeps = asked.iter().filter(|got| *got == &expected).count();
        assert_eq!(sweeps, 2, "the sweep pages over due proposals, then stops",);
        let approved_only = ActionFilter {
            status: Some("approved".to_owned()),
            ..ActionFilter::default()
        };
        let dispatched_only = ActionFilter {
            status: Some("dispatched".to_owned()),
            ..ActionFilter::default()
        };
        assert_eq!(
            asked.iter().filter(|got| *got == &approved_only).count(),
            1,
            "the service start looks for what a previous session left approved, once",
        );
        assert_eq!(
            asked.iter().filter(|got| *got == &dispatched_only).count(),
            1,
            "the service start looks for what a previous session left running, once",
        );
        assert_eq!(
            asked.len(),
            sweeps + 2,
            "the only listings are the sweep and the two the service start makes to catch up on what a previous session left",
        );
        assert!(
            asked
                .iter()
                .all(|got| got == &expected || got == &approved_only || got == &dispatched_only),
            "a listing asks for open proposals whose window closed, or for one of the two statuses a restart leaves behind; none reads every action",
        );
    }

    let again = a.service.expire_due().await.unwrap();
    assert_eq!(again, 0, "each due proposal is decided once");
    assert_eq!(a.get(open_id).await.status(), &ActionStatus::Proposed);
    assert_eq!(a.get(decided_id).await.status(), &ActionStatus::Denied);
}

async fn approve(handle: ActionHandle, id: ActionId) -> Result<ActionStatus, ApprovalError> {
    handle
        .approve(id, human(UserId::new()), None)
        .await
        .map(|action| action.status().clone())
}

async fn deny(handle: ActionHandle, id: ActionId) -> Result<ActionStatus, ApprovalError> {
    handle
        .deny(id, human(UserId::new()))
        .await
        .map(|action| action.status().clone())
}
