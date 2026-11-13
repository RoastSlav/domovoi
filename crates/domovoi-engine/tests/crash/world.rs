//! A running incident manager and action service over a crashing store, plus the restart.
//!
//! Every commit the services make goes through [`CrashingStore`]; every read goes straight
//! to the store underneath it, so a restart sees exactly what was durable. The world is
//! built twice per sweep case: once over a fresh store to run the scenario, and once over
//! the surviving store after a simulated crash.

use std::collections::BTreeMap;
use std::sync::Arc;

use domovoi_core::ports::{
    ActionFilter, ActionRepo, CheckRecheck, Clock, IncidentFilter, IncidentRepo, Page, Presence,
    SkillOutput,
};
use domovoi_core::{
    ActionId, ActionRequest, ActionStatus, CheckKey, Evidence, HostId, Incident, IncidentId,
    ParamKind, ParamName, ParamSpec, Principal, Risk, Severity, Signal, SignalClass, SignalStatus,
    SkillManifest, SkillSummary, SourceKind, TargetKey, UserId, Verdict, VerifySpec,
};
use domovoi_engine::triage::{mint_token, token_hash};
use domovoi_engine::{
    ActionDeps, ActionHandle, ActionService, ActionSettings, EnginePolicy, IncidentManager,
    ManagerDeps, ManagerHandle, PatternRedactor,
};
use domovoi_testkit::build::{T0, manifest, open_incident};
use domovoi_testkit::crash::{CrashingStore, SweepStore, check_invariants};
use domovoi_testkit::{FakeAgentLink, FakeClock};
use jiff::{SignedDuration, Timestamp};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// The host whose agent offers the skills and whose container incident the world seeds.
#[must_use]
pub fn host_id() -> HostId {
    "01900000-0000-7000-8000-000000000001"
        .parse()
        .expect("a valid host id")
}

/// A second host, used to prove the manager still opens incidents after a restart.
#[must_use]
pub fn new_host_id() -> HostId {
    "01900000-0000-7000-8000-000000000002"
        .parse()
        .expect("a valid host id")
}

/// How long a quiet incident stays open: long enough for a scenario, short enough that a
/// tick after the clock moves resolves the seeded one.
const TTL: SignedDuration = SignedDuration::from_mins(10);

/// The manager policy: two runs confirm a normal signal, and the ticker never fires on its
/// own, so every tick in these tests is one the scenario asked for.
fn policy() -> EnginePolicy {
    let mut policy = EnginePolicy::default();
    policy.incident.ttl = Some(TTL);
    policy.tick_every = SignedDuration::from_hours(24);
    policy
}

/// The action settings: two runs per skill and hour (so the third is blocked), no wait for
/// an offline agent, and a short verification delay.
fn settings() -> ActionSettings {
    ActionSettings {
        loop_guard_per_hour: 2,
        offline_wait: SignedDuration::ZERO,
        presence_poll: SignedDuration::from_secs(1),
        verify_default_delay: SignedDuration::from_secs(1),
        dispatch_grace: SignedDuration::from_secs(30),
    }
}

/// `restart_container` with one `container` parameter.
#[must_use]
pub fn restart_skill() -> SkillSummary {
    let mut manifest = manifest("restart_container", Risk::Write).expect("a valid manifest");
    manifest.params = vec![ParamSpec {
        name: ParamName::parse("container").expect("a valid parameter name"),
        description: "The container to restart.".to_owned(),
        required: true,
        kind: ParamKind::String {
            pattern: "^[a-z0-9_-]+$".to_owned(),
            max_len: 64,
        },
    }];
    manifest.summary()
}

/// `restart_container` that re-checks after 10 s, so a successful run is verified.
#[must_use]
pub fn verified_restart() -> SkillSummary {
    let mut skill = restart_skill();
    skill.verify = Some(VerifySpec {
        recheck: true,
        delay: SignedDuration::from_secs(10),
    });
    skill
}

/// A restart-shaped skill that is never verified: it stays `succeeded` after its run.
fn runner(name: &str) -> SkillSummary {
    let mut manifest: SkillManifest = manifest(name, Risk::Write).expect("a valid manifest");
    manifest.params = restart_skill().params.clone();
    manifest.summary()
}

/// The skill of the loop-guard step, which has used no slot when the step starts.
#[must_use]
pub fn loop_skill() -> SkillSummary {
    runner("restart_loop")
}

/// A skill whose run exits non-zero, so the action reaches `failed`.
#[must_use]
pub fn fail_skill() -> SkillSummary {
    runner("restart_failing")
}

/// A skill that re-checks and finds the check still firing, so it reaches `verify_failed`.
#[must_use]
pub fn verify_fail_skill() -> SkillSummary {
    let mut skill = runner("restart_unverified");
    skill.verify = Some(VerifySpec {
        recheck: true,
        delay: SignedDuration::from_secs(5),
    });
    skill
}

/// A skill whose manifest changes between approval and dispatch, so dispatch rejects it.
#[must_use]
pub fn mutable_skill() -> SkillSummary {
    runner("restart_mutable")
}

/// The skill approved after its agent goes offline.
#[must_use]
pub fn offline_skill() -> SkillSummary {
    runner("restart_offline")
}

/// Every skill the host's agent announces.
#[must_use]
pub fn all_skills() -> Vec<SkillSummary> {
    vec![
        verified_restart(),
        loop_skill(),
        fail_skill(),
        verify_fail_skill(),
        mutable_skill(),
        offline_skill(),
    ]
}

/// A successful `run_skill` output.
#[must_use]
pub fn ok_output(stdout: &str) -> SkillOutput {
    SkillOutput {
        exit_code: Some(0),
        stdout: stdout.to_owned(),
        stderr: String::new(),
        truncated: false,
        duration_ms: 1200,
        argv_hash: [0xab; 32],
    }
}

/// A re-check answer for the given status.
#[must_use]
pub fn check(status: SignalStatus) -> CheckRecheck {
    CheckRecheck {
        status,
        summary: "nginx is running".to_owned(),
        evidence: Evidence::new(),
        observed_at: T0,
    }
}

/// A firing signal observed now, from the host's agent.
#[must_use]
pub fn firing(
    host: HostId,
    check: &str,
    target: Option<&str>,
    class: SignalClass,
    at: Timestamp,
) -> Signal {
    Signal::firing(
        SourceKind::Agent,
        host,
        CheckKey::parse(check).expect("a valid check key"),
        target.map(|target| TargetKey::parse(target).expect("a valid target key")),
        Severity::Warning,
        format!("{check} firing"),
        at,
    )
    .with_class(class)
}

/// A resolved signal observed at `at`.
#[must_use]
pub fn resolved(host: HostId, check: &str, target: Option<&str>, at: Timestamp) -> Signal {
    Signal::resolved(
        SourceKind::Agent,
        host,
        CheckKey::parse(check).expect("a valid check key"),
        target.map(|target| TargetKey::parse(target).expect("a valid target key")),
        format!("{check} ok"),
        at,
    )
}

/// A manager, an action service and their fakes, all over one crashing store.
pub struct World<S: SweepStore> {
    /// The wrapper every commit passes through.
    pub crashing: Arc<CrashingStore<S>>,
    /// The store underneath it: what the services read and what survives a restart.
    pub store: Arc<S>,
    /// Time, moved only by the scenario.
    pub clock: Arc<FakeClock>,
    /// The agents' presence, skills and answers.
    pub link: Arc<FakeAgentLink>,
    /// The manager.
    pub manager: ManagerHandle,
    /// The action service.
    pub service: ActionHandle,
    shutdown: CancellationToken,
    manager_task: Option<JoinHandle<()>>,
    action_task: Option<JoinHandle<()>>,
}

impl<S: SweepStore> Drop for World<S> {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}

impl<S: SweepStore> World<S> {
    /// A world over a fresh store, with one open container incident already stored.
    pub async fn new(store: Arc<S>) -> Self {
        let incident = open_incident(host_id(), "docker.container", Some("nginx"))
            .expect("a valid open incident");
        IncidentRepo::insert(&*store, &incident)
            .await
            .expect("the seeded incident is stored");
        Self::build(store)
    }

    /// A world over the state a crashed process left behind: no seeding, fresh services and
    /// a fresh [`CrashingStore`] that only records.
    pub fn restarted(store: Arc<S>) -> Self {
        Self::build(store)
    }

    fn build(store: Arc<S>) -> Self {
        let clock = Arc::new(FakeClock::new(T0));
        let link = Arc::new(FakeAgentLink::new());
        script(&link);
        let crashing = Arc::new(CrashingStore::new(store.clone()));
        let (bus, _events) = broadcast::channel(256);
        let shutdown = CancellationToken::new();
        let (manager, manager_task) = IncidentManager::spawn(ManagerDeps {
            incidents: store.clone(),
            uow: crashing.clone(),
            versions: crashing.clone(),
            acks: store.clone(),
            clock: clock.clone(),
            redactor: Arc::new(PatternRedactor::default()),
            policy: policy(),
            bus: bus.clone(),
            shutdown: shutdown.clone(),
        });
        let (service, action_task) = ActionService::spawn(
            ActionDeps {
                actions: store.clone(),
                incidents: store.clone(),
                audit: store.clone(),
                uow: crashing.clone(),
                link: link.clone(),
                redactor: Arc::new(PatternRedactor::default()),
                clock: clock.clone(),
                manager: manager.clone(),
                bus,
                shutdown: shutdown.clone(),
            },
            settings(),
        );
        Self {
            crashing,
            store,
            clock,
            link,
            manager,
            service,
            shutdown,
            manager_task: Some(manager_task),
            action_task: Some(action_task),
        }
    }

    /// The fake clock's reading.
    pub fn now(&self) -> Timestamp {
        Clock::now(&*self.clock)
    }

    /// Moves the fake clock forward by `secs`.
    pub fn advance(&self, secs: i64) {
        self.clock.advance(SignedDuration::from_secs(secs));
    }

    /// Lets every queued task run without moving time.
    pub async fn settle(&self) {
        for _ in 0..200 {
            tokio::task::yield_now().await;
        }
    }

    /// Waits until no action is in a status the services will still change and both service
    /// queues have drained, so a read of the store is not torn by a commit landing between
    /// two of its queries.
    ///
    /// Each round yields, drains the manager and the writer, and looks for a run in flight.
    /// Fifteen quiet rounds, a second of time apart, also cover a pending verification,
    /// whose wait is at most ten seconds here.
    pub async fn quiesce(&self) {
        let mut quiet = 0;
        for _ in 0..600 {
            for _ in 0..20 {
                tokio::task::yield_now().await;
            }
            assert!(
                self.manager.tick().await.is_ok(),
                "the manager stopped while waiting for it to go quiet"
            );
            assert!(
                self.service.expire_due().await.is_ok(),
                "the writer stopped while waiting for it to go quiet"
            );
            let moving = self.all_actions().await.iter().any(|action| {
                matches!(
                    action.status(),
                    ActionStatus::Approved | ActionStatus::Dispatched
                )
            });
            quiet = if moving { 0 } else { quiet + 1 };
            if quiet >= 15 {
                return;
            }
            self.advance(1);
        }
        panic!("the services never went quiet");
    }

    /// Every stored action.
    pub async fn all_actions(&self) -> Vec<ActionRequest> {
        let mut items = Vec::new();
        let mut page = Page::first(Page::MAX_LIMIT);
        loop {
            let got = ActionRepo::list(&*self.store, &ActionFilter::default(), &page)
                .await
                .expect("the actions are readable");
            items.extend(got.items);
            match got.next_cursor {
                Some(cursor) => page.cursor = Some(cursor),
                None => return items,
            }
        }
    }

    /// Every stored incident, newest first.
    pub async fn incidents(&self) -> Vec<Incident> {
        let mut items = Vec::new();
        let mut page = Page::first(Page::MAX_LIMIT);
        loop {
            let got = IncidentRepo::list(&*self.store, &IncidentFilter::default(), &page)
                .await
                .expect("the incidents are readable");
            items.extend(got.items);
            match got.next_cursor {
                Some(cursor) => page.cursor = Some(cursor),
                None => return items,
            }
        }
    }

    /// The stored action with `id`.
    pub async fn action(&self, id: ActionId) -> Option<ActionRequest> {
        ActionRepo::get(&*self.store, id)
            .await
            .expect("the action is readable")
    }

    /// Builds a proposal as triage does, with a one-use chat token.
    pub fn proposal(
        &self,
        incident: IncidentId,
        skill: &SkillSummary,
        args: &[(&str, &str)],
        ttl: SignedDuration,
    ) -> (ActionRequest, String) {
        let args: BTreeMap<String, String> = args
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        let validated = skill
            .validate_args(&args)
            .expect("the args match the manifest");
        let token = mint_token();
        let action = ActionRequest::propose(
            incident,
            host_id(),
            skill,
            &validated,
            Principal::System("triage".into()),
            ttl,
            self.now(),
        )
        .with_chat_token_hash(token_hash(&token));
        (action, token)
    }

    /// Proposes `skill` through the writer and returns the stored action and its token.
    pub async fn propose(
        &self,
        incident: IncidentId,
        skill: &SkillSummary,
        args: &[(&str, &str)],
        ttl: SignedDuration,
    ) -> Result<(ActionRequest, String), domovoi_engine::ApprovalError> {
        let (action, token) = self.proposal(incident, skill, args, ttl);
        let stored = self.service.propose(action).await?;
        Ok((stored, token))
    }

    /// Waits, moving the clock a second at a time, until the stored action satisfies `done`.
    /// Fails at once when the process is down: nothing will change any more.
    pub async fn wait_for(
        &self,
        id: ActionId,
        done: impl Fn(&ActionStatus) -> bool,
    ) -> Result<ActionRequest, String> {
        for _ in 0..2000 {
            if self.crashing.crashed() {
                return Err(format!("the process is down before action {id} finished"));
            }
            for _ in 0..20 {
                tokio::task::yield_now().await;
            }
            if let Some(action) = self.action(id).await
                && done(action.status())
            {
                // The writer may still be recording the outcome of a verification.
                self.settle().await;
                return Ok(action);
            }
            self.advance(1);
        }
        Err(format!("action {id} never reached the expected status"))
    }

    /// Waits until `incident`'s verdict is `verdict`, so the write a successful run makes
    /// after its verification has landed before the scenario moves on.
    pub async fn wait_for_verdict(
        &self,
        incident: IncidentId,
        verdict: Verdict,
    ) -> Result<(), String> {
        for _ in 0..2000 {
            if self.crashing.crashed() {
                return Err("the process is down before the verdict was stored".to_owned());
            }
            for _ in 0..20 {
                tokio::task::yield_now().await;
            }
            let found = IncidentRepo::get(&*self.store, incident)
                .await
                .expect("the incident is readable");
            if found.is_some_and(|incident| incident.verdict() == Some(verdict)) {
                return Ok(());
            }
            self.advance(1);
        }
        Err("the verdict was never stored".to_owned())
    }

    /// Stops the services and waits for their tasks with the fake clock only: no real
    /// sleeps, at most 10 000 yields.
    pub async fn shutdown(mut self) {
        self.shutdown.cancel();
        join(self.manager_task.take()).await;
        join(self.action_task.take()).await;
    }

    /// Lets the restarted services do their start-up work: one tick, the writer's start-up
    /// scan of actions left running, and every run the start-up scan starts, until nothing
    /// is in flight.
    pub async fn recover(&self) -> Result<(), String> {
        self.advance(1);
        self.manager
            .tick()
            .await
            .map_err(|err| format!("the restarted manager did not tick: {err}"))?;
        self.service
            .expire_due()
            .await
            .map_err(|err| format!("the restarted writer did not start: {err}"))?;
        self.quiesce().await;
        Ok(())
    }

    /// Proves the restarted services still do their work: one new incident, one approved
    /// action and the invariants again.
    pub async fn assert_services_work(&self) {
        let before = self.incidents().await;
        let known: Vec<IncidentId> = before.iter().map(Incident::id).collect();
        let signal = firing(
            new_host_id(),
            "net.link",
            Some("eth0"),
            SignalClass::Immediate,
            self.now(),
        );
        self.manager
            .ingest(vec![signal])
            .await
            .expect("the manager stores a signal for a new host");

        let after = self.incidents().await;
        assert_eq!(
            after.len(),
            before.len() + 1,
            "the new host's immediate signal opens exactly one incident"
        );
        let incident = after
            .iter()
            .find(|incident| !known.contains(&incident.id()))
            .expect("the new incident is there");

        let (action, _) = self
            .propose(
                incident.id(),
                &verified_restart(),
                &[("container", "nginx")],
                SignedDuration::from_hours(1),
            )
            .await
            .expect("the writer stores a proposal");
        let approved = self
            .service
            .approve(
                action.id(),
                domovoi_testkit::approvals::human(UserId::new()),
                None,
            )
            .await
            .expect("the writer approves the action");
        assert_eq!(approved.status(), &ActionStatus::Approved);
        let _ = self
            .wait_for(action.id(), |status| {
                !matches!(
                    status,
                    ActionStatus::Proposed | ActionStatus::Approved | ActionStatus::Dispatched
                )
            })
            .await;
        self.quiesce().await;
        if let Err(violations) = check_invariants(&*self.store).await {
            panic!("invariants after recovery: {violations:#?}");
        }
    }
}

/// Waits for a service task without sleeping: a yield loop, at most 10 000 times.
async fn join(task: Option<JoinHandle<()>>) {
    let Some(task) = task else {
        return;
    };
    for _ in 0..10_000 {
        if task.is_finished() {
            let _ = task.await;
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("services did not stop");
}

/// Scripts the agent: connected, announcing every skill, restarting containers and resolving
/// the check.
fn script(link: &FakeAgentLink) {
    link.set_presence(
        host_id(),
        Presence::Connected {
            since: T0,
            version: "0.1.0".to_owned(),
        },
    );
    link.set_skills(host_id(), all_skills());
    link.on_skill(|_, dispatch| {
        if dispatch.skill.as_str() == "restart_failing" {
            Ok(SkillOutput {
                exit_code: Some(3),
                ..ok_output("boom")
            })
        } else {
            Ok(ok_output("restarted nginx"))
        }
    });
    link.on_check(|_, _, _| Ok(check(SignalStatus::Resolved)));
}
