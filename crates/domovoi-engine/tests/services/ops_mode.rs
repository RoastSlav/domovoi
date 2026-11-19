//! Running a world's incidents by either path: the direct triage run or an investigation
//! operation, with the effect runner driven by the fake clock.

use domovoi_core::ports::{
    ActionFilter, ActionRepo, CapabilityRegistry, OperationFilter, OperationRepo, Page,
};
use domovoi_core::{Incident, Operation, OperationSnapshot, TriageOutcome};
use jiff::SignedDuration;
use tokio::task::{JoinHandle, yield_now};

use crate::harness::{Mode, World, host_id};
use crate::proposal_view::{ProposalView, from_actions, from_snapshot};

impl World {
    /// Triages the world's incident by `mode` and returns the outcome.
    pub async fn run(&self, mode: Mode) -> TriageOutcome {
        self.run_on(mode, &self.incident).await
    }

    /// Triages `incident` by `mode` and returns the outcome.
    pub async fn run_on(&self, mode: Mode, incident: &Incident) -> TriageOutcome {
        match mode {
            // The direct run is awaited in place, so a test can race it with a timer.
            Mode::Legacy => self.triage.triage(incident).await,
            Mode::Operations => {
                let run = self.spawn_run(mode, incident);
                self.finish(run).await
            }
        }
    }

    /// Starts triaging `incident` by `mode` on a task, without driving it.
    pub fn spawn_run(&self, mode: Mode, incident: &Incident) -> JoinHandle<TriageOutcome> {
        let incident = incident.clone();
        match mode {
            Mode::Legacy => {
                let triage = self.triage.clone();
                tokio::spawn(async move { triage.triage(&incident).await })
            }
            Mode::Operations => {
                let ops = self
                    .ops
                    .clone()
                    .expect("the world was built for Mode::Operations");
                tokio::spawn(async move {
                    match ops.investigate_and_wait(&incident, false).await {
                        Ok(outcome) => outcome,
                        Err(err) => panic!("the investigation gave no outcome: {err}"),
                    }
                })
            }
        }
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

    /// What was proposed for `incident`, as legacy actions or as Execution steps.
    pub async fn proposals(&self, mode: Mode, incident: &Incident) -> Vec<ProposalView> {
        match mode {
            Mode::Legacy => {
                let actions =
                    ActionRepo::list(&*self.store, &ActionFilter::default(), &Page::first(100))
                        .await
                        .unwrap()
                        .items;
                let own: Vec<_> = actions
                    .into_iter()
                    .filter(|action| action.incident() == incident.id())
                    .collect();
                from_actions(&own)
            }
            Mode::Operations => {
                let skills = self.capabilities.enabled_skills(host_id());
                self.snapshots(incident)
                    .await
                    .iter()
                    .flat_map(|snapshot| from_snapshot(snapshot, &skills))
                    .collect()
            }
        }
    }
}
