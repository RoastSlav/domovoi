//! Running a world's incidents as investigation operations, with the effect runner driven
//! by the fake clock.

use domovoi_core::ports::{CapabilityRegistry, OperationFilter, OperationRepo, Page};
use domovoi_core::{Incident, Operation, OperationSnapshot, TriageOutcome};
use jiff::SignedDuration;
use tokio::task::{JoinHandle, yield_now};

use crate::harness::{World, host_id};
use crate::proposal_view::{ProposalView, from_snapshot};

impl World {
    /// Investigates the world's incident and returns the outcome.
    pub async fn run(&self) -> TriageOutcome {
        self.run_on(&self.incident).await
    }

    /// Cancels every open investigation of `incident`, as a person closing a step that waits
    /// for approval, so a second investigation may start.
    pub async fn withdraw(&self, incident: &Incident) {
        for snapshot in self.snapshots(incident).await {
            if !snapshot.operation.status.is_terminal() {
                let by = domovoi_core::Actor::User(domovoi_core::UserId::new());
                self.ops
                    .cancel(snapshot.operation.id, by, "withdrawn".to_owned())
                    .await
                    .expect("the open investigation is cancelled");
            }
        }
    }

    /// Investigates `incident` and returns the outcome.
    pub async fn run_on(&self, incident: &Incident) -> TriageOutcome {
        let run = self.spawn_run(incident);
        self.finish(run).await
    }

    /// Investigates `incident` again from scratch, skipping the repeat cache.
    pub async fn run_fresh_on(&self, incident: &Incident) -> TriageOutcome {
        let incident = incident.clone();
        let ops = self.ops.clone();
        let run = tokio::spawn(async move {
            match ops.investigate_and_wait(&incident, true).await {
                Ok(outcome) => outcome,
                Err(err) => panic!("the investigation gave no outcome: {err}"),
            }
        });
        self.finish(run).await
    }

    /// Starts investigating `incident` on a task, without driving it.
    pub fn spawn_run(&self, incident: &Incident) -> JoinHandle<TriageOutcome> {
        let incident = incident.clone();
        let ops = self.ops.clone();
        tokio::spawn(async move {
            match ops.investigate_and_wait(&incident, false).await {
                Ok(outcome) => outcome,
                Err(err) => panic!("the investigation gave no outcome: {err}"),
            }
        })
    }

    /// Drives the services, moving the fake clock a second after every tenth round of yields
    /// that did not finish `run`, and returns its result.
    pub async fn finish<T>(&self, run: JoinHandle<T>) -> T {
        for round in 0..30_000 {
            for _ in 0..100 {
                yield_now().await;
            }
            if run.is_finished() {
                return run.await.expect("the run task finished");
            }
            if round % 10 == 9 {
                self.clock.advance(SignedDuration::from_secs(1));
            }
        }
        panic!("the run never finished");
    }

    /// Investigates the world's incident under paused tokio time, moving both that time and
    /// the fake clock a second after every round of yields that did not finish the run.
    pub async fn run_paused(&self) -> TriageOutcome {
        let run = self.spawn_run(&self.incident);
        for _ in 0..30_000 {
            for _ in 0..100 {
                yield_now().await;
            }
            if run.is_finished() {
                return run.await.expect("the run task finished");
            }
            tokio::time::advance(std::time::Duration::from_secs(1)).await;
            self.clock.advance(SignedDuration::from_secs(1));
        }
        panic!("the run never finished");
    }

    /// Lets the services work for `rounds` rounds of yields, moving the clock `step`
    /// seconds after each.
    pub async fn drive(&self, rounds: u32, step: i64) {
        for _ in 0..rounds {
            for _ in 0..100 {
                yield_now().await;
            }
            self.clock.advance(SignedDuration::from_secs(step));
        }
    }

    /// Every operation in the store, newest first.
    pub async fn operations(&self) -> Vec<Operation> {
        OperationRepo::list(&*self.store, &OperationFilter::default(), &Page::first(100))
            .await
            .unwrap()
            .items
    }

    /// The stored snapshot of every operation about `incident`.
    pub async fn snapshots(&self, incident: &Incident) -> Vec<OperationSnapshot> {
        let mut found = Vec::new();
        for operation in OperationRepo::for_incident(&*self.store, incident.id())
            .await
            .unwrap()
        {
            found.push(
                OperationRepo::load(&*self.store, operation.id)
                    .await
                    .unwrap()
                    .unwrap(),
            );
        }
        found
    }

    /// What was proposed for `incident`, as Execution steps.
    pub async fn proposals(&self, incident: &Incident) -> Vec<ProposalView> {
        let skills = self.capabilities.enabled_skills(host_id());
        self.snapshots(incident)
            .await
            .iter()
            .flat_map(|snapshot| from_snapshot(snapshot, &skills))
            .collect()
    }

    /// Stores a legacy `Proposed` action restarting `container` for the world's incident, as
    /// one left from before investigations. Only tests seed these; nothing creates them.
    pub async fn seed_legacy_action(&self, container: &str) -> domovoi_core::ActionRequest {
        let skill = crate::harness::restart_skill();
        let args =
            std::collections::BTreeMap::from([("container".to_owned(), container.to_owned())]);
        let validated = skill.validate_args(&args).expect("valid arguments");
        let key = domovoi_core::KeyFingerprint::of_public_key(&[7; 32]);
        let action = domovoi_core::ActionRequest::propose(
            &self.incident,
            &skill,
            &validated,
            Some(key),
            domovoi_core::Principal::System("seed".into()),
            SignedDuration::from_hours(1),
            self.now(),
        )
        .expect("a proposal");
        domovoi_core::ports::ActionRepo::insert(&*self.store, &action)
            .await
            .expect("the action is stored");
        action
    }
}
