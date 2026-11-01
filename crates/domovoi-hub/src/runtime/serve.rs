//! Listener ownership and phased shutdown.

use std::net::SocketAddr;

use jiff::{SignedDuration, Timestamp};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::{HubRuntime, RuntimeError};

pub(super) async fn run(
    runtime: HubRuntime,
    shutdown: CancellationToken,
) -> Result<(), RuntimeError> {
    let HubRuntime {
        lock,
        config,
        clock,
        store,
        bus,
        manager,
        agent_server,
        app,
        dispatcher,
        events,
        schedulers,
        gateways,
        manager_task,
        triage_task,
        actions_task,
        intake_shutdown,
        worker_shutdown,
        manager_shutdown,
        delivery_shutdown,
        ..
    } = runtime;
    let web = TcpListener::bind(config.web.bind)
        .await
        .map_err(|source| RuntimeError::Bind {
            name: "web",
            source,
        })?;
    let agents = agent_server.bind().await?;
    let (stopped_tx, mut stopped_rx) = mpsc::channel(4);
    let mut intake_tasks = Vec::new();
    intake_tasks.extend(schedulers.spawn(intake_shutdown.clone()));

    let serving = agent_server.clone();
    let report = stopped_tx.clone();
    intake_tasks.push(tokio::spawn(async move {
        let result = serving.serve(agents).await;
        let outcome = result.map_err(|err| RuntimeError::Serve(err.to_string()));
        let _ = report.send(outcome).await;
    }));
    let presence = agent_server.clone();
    let report = stopped_tx.clone();
    intake_tasks.push(tokio::spawn(async move {
        let result = presence.run_presence().await;
        let outcome = result.map_err(|err| RuntimeError::Serve(err.to_string()));
        let _ = report.send(outcome).await;
    }));
    let report = stopped_tx.clone();
    let stop_http = intake_shutdown.clone();
    intake_tasks.push(tokio::spawn(async move {
        let result = axum::serve(web, app.into_make_service_with_connect_info::<SocketAddr>())
            .with_graceful_shutdown(stop_http.cancelled_owned())
            .await
            .map_err(|err| RuntimeError::Serve(format!("web server failed: {err}")));
        let _ = report.send(result).await;
    }));
    for (gateway, sink) in gateways {
        let stop = intake_shutdown.clone();
        intake_tasks.push(tokio::spawn(async move {
            tokio::select! {
                () = stop.cancelled() => {}
                result = gateway.run(sink) => {
                    if let Err(err) = result {
                        tracing::warn!(%err, gateway = %gateway.platform(), "chat gateway stopped");
                    }
                }
            }
        }));
    }
    drop(stopped_tx);
    let delivery_task = tokio::spawn(dispatcher.run(events, delivery_shutdown.clone()));
    let outcome = tokio::select! {
        () = shutdown.cancelled() => Ok(()),
        stopped = stopped_rx.recv() => stopped.unwrap_or_else(|| {
            Err(RuntimeError::Serve("every hub listener stopped".to_owned()))
        }),
    };

    let total = deadline(clock.now(), 15);
    intake_shutdown.cancel();
    agent_server.shutdown();
    let intake_deadline = deadline(clock.now(), 2).min(total);
    let mut clean = true;
    for mut task in intake_tasks {
        clean &= finish(&mut task, &clock, intake_deadline).await;
    }
    drop(manager);

    worker_shutdown.cancel();
    if let Some(mut task) = triage_task {
        let grace = deadline(clock.now(), 10).min(total);
        clean &= finish(&mut task, &clock, grace).await;
    }
    let mut actions_task = actions_task;
    clean &= finish(&mut actions_task, &clock, total).await;

    manager_shutdown.cancel();
    let mut manager_task = manager_task;
    clean &= finish(&mut manager_task, &clock, total).await;
    drop(bus);
    delivery_shutdown.cancel();
    let mut delivery_task = delivery_task;
    clean &= finish(&mut delivery_task, &clock, total).await;

    let closing = store.close();
    tokio::pin!(closing);
    tokio::select! {
        () = &mut closing => {}
        () = clock.sleep_until(total) => clean = false,
    }
    // Released only once the store is closed, so a CLI admin command never sees a
    // half-closed database.
    drop(lock);
    if clean {
        outcome
    } else {
        Err(RuntimeError::ShutdownTimeout)
    }
}

fn deadline(now: Timestamp, seconds: i64) -> Timestamp {
    now.checked_add(SignedDuration::from_secs(seconds))
        .unwrap_or(now)
}

async fn finish(
    task: &mut JoinHandle<()>,
    clock: &std::sync::Arc<dyn domovoi_core::ports::Clock>,
    until: Timestamp,
) -> bool {
    tokio::select! {
        result = &mut *task => {
            if let Err(err) = result
                && !err.is_cancelled()
            {
                tracing::warn!(%err, "a hub task failed");
            }
            true
        }
        () = clock.sleep_until(until) => {
            tracing::warn!(deadline = %until, "hub shutdown phase timed out");
            task.abort();
            let _ = task.await;
            false
        }
    }
}
