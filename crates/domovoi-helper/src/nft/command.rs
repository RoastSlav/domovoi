//! The bodies of `domovoi helper nft-selftest`, `nft-selftest-sender` and `containment-drill`.
//!
//! The `domovoi` binary parses the arguments, builds the executor and the clock and calls these
//! functions. They run as root, never inside the long-lived helper, and an `Err` means the run
//! could not complete, which is never a pass: the stored result stays as it was.

use std::path::Path;
use std::sync::Arc;

use domovoi_core::containment::templates::{TableName, delete_own_table, list_table_args};
use domovoi_core::containment::{DrillsState, JournalRecord, RecreateReason, Verdict};
use domovoi_core::identity::HostDirs;
use domovoi_core::ports::{Clock, HostInspect, ProcessExecutor};
use jiff::SignedDuration;

use super::drill::{self, DrillsFile};
use super::exec::Runner;
use super::journal_file::{Journal, fold};
use super::selftest::scenario::Host;
use super::selftest::{self, sender};
use super::view;
use crate::start_check::run_start_checks;
use crate::state::StateLock;

/// What a command needs from the host.
pub struct Env {
    /// Runs every process.
    pub exec: Arc<dyn ProcessExecutor>,
    /// Looks at the host.
    pub inspect: Arc<dyn HostInspect>,
    /// The clock.
    pub clock: Arc<dyn Clock>,
    /// The helper's directories.
    pub dirs: HostDirs,
    /// This binary's absolute path, re-invoked as the sender.
    pub exe: String,
}

fn ready(env: &Env) -> Result<(), String> {
    if env.inspect.euid() != 0 {
        return Err("this command needs root".to_owned());
    }
    run_start_checks(env.inspect.as_ref(), &env.dirs)
        .map_err(|refusal| format!("the helper's state is not sound: {}", refusal.token))
}

async fn key(env: &Env) -> Result<domovoi_core::containment::SelftestKey, String> {
    let runner = Runner {
        exec: env.exec.as_ref(),
        inspect: env.inspect.as_ref(),
        timeout: SignedDuration::from_secs(5),
        netns: None,
    };
    let version = runner.version().await.map_err(|e| e.to_string())?;
    if !version.ok() {
        return Err("nft --version failed".to_owned());
    }
    let kernel = env.inspect.kernel_release().map_err(|e| e.to_string())?;
    Ok(selftest::current_key(kernel.trim(), version.stdout.trim()))
}

fn host(env: &Env) -> Host<'_> {
    Host {
        exec: env.exec.as_ref(),
        inspect: env.inspect.as_ref(),
        clock: env.clock.as_ref(),
        exe: env.exe.clone(),
        id: std::process::id().to_string(),
    }
}

fn word(verdict: &Verdict) -> String {
    match verdict {
        Verdict::Pass => "pass".to_owned(),
        Verdict::Fail { leaked } => format!("fail ({leaked} packets leaked)"),
        Verdict::Inconclusive { reason } => format!("inconclusive ({reason})"),
    }
}

/// `domovoi helper nft-selftest [--if-changed] [--interval-only]`.
pub async fn selftest(
    env: &Env,
    if_changed: bool,
    interval_only: bool,
) -> Result<Vec<String>, String> {
    ready(env)?;
    let dir = env.dirs.helper_dir.as_path();
    let key = key(env).await?;
    let stored = selftest::read_result(dir);
    if if_changed && selftest::up_to_date(stored.as_ref(), &key, env.clock.now()) {
        return Ok(vec!["the stored self-test result is current".to_owned()]);
    }
    let run = selftest::run(&host(env), key, stored.as_ref(), interval_only)
        .await
        .map_err(|e| e.to_string())?;
    let lock = StateLock::open(dir).map_err(|e| e.to_string())?;
    let guard = lock.acquire().map_err(|e| e.to_string())?;
    selftest::write_result(&guard, dir, &run.result)?;
    let mut lines = vec![
        format!("hosts: {}", word(&run.result.hosts)),
        format!(
            "prefixes: {}",
            run.result
                .prefixes
                .as_ref()
                .map_or("not tested".to_owned(), word)
        ),
        format!(
            "replacements: {} (lowest of any scenario)",
            run.result.replacements
        ),
        format!("control leak ratio: {:.3}", run.result.control_leak_ratio),
    ];
    for c in &run.counts {
        lines.push(format!(
            "{}: leaked {} of {} sent, {} replacements, control leaked {} of {}",
            c.name, c.leaked, c.sent, c.replacements, c.control_leaked, c.control_sent
        ));
    }
    Ok(lines)
}

/// `domovoi helper nft-selftest-sender --src A --dst B --port P --seconds N`: prints `sent=<n>`.
pub fn sender_command(args: &[String]) -> Result<String, String> {
    let parsed = sender::parse(args).ok_or_else(|| "bad sender arguments".to_owned())?;
    let sent = sender::run(parsed).map_err(|e| e.to_string())?;
    Ok(format!("sent={sent}"))
}

/// `domovoi helper containment-drill`.
pub async fn containment_drill(env: &Env) -> Result<Vec<String>, String> {
    ready(env)?;
    let dir = env.dirs.helper_dir.as_path();
    let key = key(env).await?;
    let host = host(env);
    let scratch = dir.join(format!("drill-{}", host.id));
    prepare_scratch(&scratch)?;
    let built = host.build().await;
    let outcome = match built {
        Ok(()) => drill::run(&host, &scratch).await,
        Err(e) => Err(e),
    };
    host.teardown().await;
    let _ = std::fs::remove_dir_all(&scratch);
    let results = outcome.map_err(|e| e.to_string())?;
    let passed = results.values().all(|r| r == "ok");
    let at = env.clock.now();
    let file = DrillsFile {
        key,
        state: if passed {
            DrillsState::Passed { at }
        } else {
            DrillsState::Failed { at }
        },
        results: results.clone(),
    };
    let lock = StateLock::open(dir).map_err(|e| e.to_string())?;
    let guard = lock.acquire().map_err(|e| e.to_string())?;
    drill::write(&guard, dir, &file)?;
    Ok(results.iter().map(|(n, r)| format!("{n}: {r}")).collect())
}

fn prepare_scratch(scratch: &Path) -> Result<(), String> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(scratch)
        .map_err(|e| e.to_string())?;
    std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o600)
        .open(scratch.join(crate::state::LOCK_FILE))
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// `domovoi helper nft-recreate`: the maintenance operation behind a layout, `max_elements` or
/// default-timeout change. It deletes the helper's own table, and only when no block is live and
/// every block set is empty; the next request builds the table again from the template. Nothing
/// else ever deletes the table.
pub async fn recreate(env: &Env, table: &str) -> Result<Vec<String>, String> {
    ready(env)?;
    let table = TableName::parse(table).map_err(|e| e.to_string())?;
    let dir = env.dirs.helper_dir.as_path();
    let lock = StateLock::open(dir).map_err(|e| e.to_string())?;
    let guard = lock.acquire().map_err(|e| e.to_string())?;
    let journal = Journal::new(dir);
    let loaded = journal.load().map_err(|e| e.to_string())?;
    let now = env.clock.now();
    if fold(&loaded.records).iter().any(|b| b.live(now)) {
        return Err("a block is still live: release it first".to_owned());
    }
    let runner = Runner {
        exec: env.exec.as_ref(),
        inspect: env.inspect.as_ref(),
        timeout: SignedDuration::from_secs(5),
        netns: None,
    };
    let listed = runner
        .read(list_table_args(&table))
        .await
        .map_err(|e| e.to_string())?;
    if !listed.ok() {
        return if view::is_absent(listed.exit, &listed.stderr) {
            Ok(vec![
                "there is no table; the next request creates it".to_owned(),
            ])
        } else {
            Err("the table could not be read".to_owned())
        };
    }
    let current = view::parse(&listed.stdout).map_err(|e| e.to_string())?;
    let held = current
        .sets
        .iter()
        .filter(|(name, _)| name.starts_with("blocked"))
        .any(|(_, set)| !set.members.is_empty());
    if held {
        return Err("a block set still holds elements: release them first".to_owned());
    }
    let removed = runner
        .script(&delete_own_table(&table))
        .await
        .map_err(|e| e.to_string())?;
    if !removed.ok() {
        return Err("the table could not be deleted".to_owned());
    }
    journal
        .append(
            &guard,
            &JournalRecord::TableRecreated {
                reason: RecreateReason::Maintenance,
                at: now,
            },
        )
        .map_err(|e| e.to_string())?;
    Ok(vec![
        "the table was deleted; the next request creates it from the template".to_owned(),
    ])
}
