//! `domovoi hub run`: the supervisor around the hub runtime (ADR 0017).
//!
//! It starts the runtime from the settings layers and restarts it in this process when
//! settings saved in the web UI ask for it: it checks the new settings first and answers
//! the waiting request, then stops the runtime (within its 15 s budget) and starts it
//! again on the same data directory. If the new runtime will not start, it goes back to
//! the previous settings. The hub crate never restarts itself.

use std::path::PathBuf;
use std::sync::Arc;

use domovoi_config::AgentConfig;
use domovoi_core::ports::{Clock, ProcessExecutor};
use domovoi_hub::runtime::{HubDeps, HubRuntime, LocalAgents};
use domovoi_hub::settings::{ApplyRequest, ConfigSources};
use domovoi_runner::SandboxExecutor;
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

use super::{HubConfigArg, redactor, warn};
use crate::ExitCode;
use crate::init::{self, SystemClock};
use crate::local_link;

/// `hub run`.
#[derive(Debug, clap::Args)]
pub struct RunArgs {
    #[command(flatten)]
    config: HubConfigArg,
    /// Also run an agent for this machine in the same process (single-box mode).
    #[arg(long, value_name = "AGENT_TOML")]
    with_local_agent: Option<PathBuf>,
}

/// How one run of the runtime ended.
enum Ended {
    /// Shutdown was requested, or the runtime stopped by itself.
    Stopped(Result<(), domovoi_hub::runtime::RuntimeError>),
    /// New settings passed preflight; start again.
    Restart,
}

pub async fn run(args: RunArgs) -> anyhow::Result<ExitCode> {
    let sources = args.config.sources()?;
    let local_agent: Option<AgentConfig> = args
        .with_local_agent
        .as_deref()
        .map(super::load_agent)
        .transpose()?;
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let metrics = init::metrics()?;
    let shutdown = init::shutdown_on_signals();
    let (apply_tx, mut apply_rx) = mpsc::channel::<ApplyRequest>(1);
    let (local_tx, local_rx) = watch::channel::<Option<LocalAgents>>(None);

    let mut agent = None;
    let mut restarting = false;
    loop {
        let executor = executor_for(&sources, &*clock).await;
        let deps = || HubDeps {
            clock: clock.clone(),
            process_executor: Some(executor.clone()),
            metrics: Some(metrics.clone()),
            apply: Some(apply_tx.clone()),
        };
        let runtime = match HubRuntime::start(sources.clone(), deps()).await {
            Ok(runtime) => runtime,
            Err(err) if restarting => {
                let why = format!("the hub did not start with the new settings: {err}");
                warn(&why);
                let data_dir =
                    domovoi_config::bootstrap_data_dir(sources.file.as_deref(), &*sources.env)
                        .map_err(|issues| anyhow::anyhow!("{issues:?}"))?;
                domovoi_hub::settings::rollback(&data_dir, &*clock, &why).await?;
                HubRuntime::start(sources.clone(), deps()).await?
            }
            Err(err) => return Err(err.into()),
        };
        if let Some(link) = runtime.setup_link().await? {
            println!("No account exists yet. Finish setup in the web UI within 24 hours:");
            println!("  {link}");
            println!("(or run `domovoi hub set-password` while the hub is stopped)");
        }
        let _ = local_tx.send(Some(runtime.local_agents()));
        if agent.is_none()
            && let Some(config) = local_agent.clone()
        {
            agent = Some(tokio::spawn(local_link::run_agent(
                local_rx.clone(),
                config,
                executor.clone(),
                clock.clone(),
                shutdown.clone(),
            )));
        }
        match supervise(runtime, &shutdown, &mut apply_rx, &clock, &executor).await {
            Ended::Restart => {
                tracing::info!("restarting the hub with the new settings");
                let _ = local_tx.send(None);
                restarting = true;
            }
            Ended::Stopped(result) => {
                shutdown.cancel();
                if let Some(agent) = agent {
                    match agent.await {
                        Ok(Ok(())) => {}
                        Ok(Err(err)) => warn(&format!("the local agent stopped: {err:#}")),
                        Err(err) => warn(&format!("the local agent task failed: {err}")),
                    }
                }
                result?;
                return Ok(ExitCode::SUCCESS);
            }
        }
    }
}

/// Runs `runtime` until shutdown, until it stops, or until new settings pass preflight.
async fn supervise(
    runtime: HubRuntime,
    shutdown: &CancellationToken,
    apply_rx: &mut mpsc::Receiver<ApplyRequest>,
    clock: &Arc<dyn Clock>,
    executor: &Arc<dyn ProcessExecutor>,
) -> Ended {
    let stop = shutdown.child_token();
    let mut running = tokio::spawn(runtime.run(stop.clone()));
    loop {
        tokio::select! {
            result = &mut running => {
                return Ended::Stopped(result.unwrap_or_else(|err| {
                    Err(domovoi_hub::runtime::RuntimeError::Serve(err.to_string()))
                }));
            }
            Some(request) = apply_rx.recv() => {
                match HubRuntime::preflight(&request.config, clock, Some(executor.clone())) {
                    Ok(()) => {
                        let _ = request.reply.send(Ok(()));
                        stop.cancel();
                        if let Ok(Err(err)) = (&mut running).await {
                            tracing::warn!(%err, "the hub stopped uncleanly before restarting");
                        }
                        if shutdown.is_cancelled() {
                            return Ended::Stopped(Ok(()));
                        }
                        return Ended::Restart;
                    }
                    Err(err) => {
                        tracing::warn!(%err, "new settings failed preflight; keeping the current ones");
                        let _ = request.reply.send(Err(err.to_string()));
                    }
                }
            }
        }
    }
}

/// The process executor for this start, redacting every secret the settings hold now.
async fn executor_for(sources: &ConfigSources, clock: &dyn Clock) -> Arc<dyn ProcessExecutor> {
    let redact = match domovoi_hub::settings::load_effective(sources, clock).await {
        Ok((config, _)) => redactor(&config),
        Err(_) => Arc::new(domovoi_hub::PatternRedactor::default()),
    };
    Arc::new(SandboxExecutor::new(redact))
}
