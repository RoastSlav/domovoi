//! `domovoi restore`: install a backup as the hub's database, offline.

use std::path::PathBuf;

use clap::Args;
use domovoi_core::ports::Clock;
use domovoi_hub::admin::{self, BackupReport, Depth, RestoreReport, verify_backup};

use crate::ExitCode;
use crate::cmd::HubConfigArg;
use crate::init::SystemClock;

/// What was decided, which the process turns into an exit code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The backup was installed.
    Restored(RestoreReport),
    /// The operator has not confirmed; nothing changed.
    NotConfirmed,
}

/// Arguments of `domovoi restore`.
#[derive(Debug, Args)]
pub struct RestoreArgs {
    /// The backup file to install.
    pub backup: PathBuf,
    /// Where the hub's configuration is; only its data directory is used.
    #[command(flatten)]
    pub config: HubConfigArg,
    /// Restore without asking. Without it, the command only describes what it would do.
    #[arg(long)]
    pub yes: bool,
}

pub async fn run(args: RestoreArgs) -> anyhow::Result<ExitCode> {
    let outcome = execute(args, &SystemClock).await?;
    Ok(match outcome {
        Outcome::Restored(_) => ExitCode::SUCCESS,
        Outcome::NotConfirmed => ExitCode::from(2),
    })
}

/// Runs the command and returns what it decided.
pub(crate) async fn execute(args: RestoreArgs, clock: &dyn Clock) -> anyhow::Result<Outcome> {
    let config = args.config.effective().await?;
    let data_dir = config.hub.data_dir.clone();
    let database = data_dir.join("hub.db");
    let report = verify_backup(&args.backup, Depth::Full)
        .await
        .map_err(|err| anyhow::anyhow!("{err}"))?;
    if !args.yes {
        print_plan(&args.backup, &database, &report);
        println!("Nothing changed. Run again with --yes to restore.");
        return Ok(Outcome::NotConfirmed);
    }
    // `admin::restore` holds the data directory's lock throughout: no hub may be running.
    let report = admin::restore(&data_dir, &args.backup, clock.now())
        .await
        .map_err(|err| match err {
            domovoi_hub::admin::AdminError::HubRunning(_) => anyhow::anyhow!(
                "a hub is running on {}; stop it before restoring",
                data_dir.display()
            ),
            other => anyhow::anyhow!("{other}"),
        })?;
    print_report(&report);
    Ok(Outcome::Restored(report))
}

fn print_plan(backup: &std::path::Path, database: &std::path::Path, report: &BackupReport) {
    println!(
        "Would restore {} into {}.",
        backup.display(),
        database.display()
    );
    println!(
        "  the backup holds {} migrations (newest {}), {} bytes",
        report.migrations, report.newest_migration, report.size_bytes
    );
    if database.exists() {
        println!(
            "  the current database would be moved to {}.before-restore-<time>",
            database.display()
        );
    } else {
        println!("  there is no current database; it would be created");
    }
    reminders();
}

fn print_report(report: &RestoreReport) {
    println!(
        "Restored the backup: {} migrations, {} bytes.",
        report.backup.migrations, report.backup.size_bytes
    );
    match &report.previous {
        Some(previous) => println!("The previous database is at {}.", previous.display()),
        None => println!("There was no previous database."),
    }
    println!(
        "{category} migration(s) ran to bring the backup up to date.",
        category = report.migrations_applied_after
    );
    reminders();
    println!("Start the hub again.");
    if audit_verify_exists() {
        println!("Then check the audit log with `domovoi audit verify`.");
    }
}

fn reminders() {
    println!("Restore secret.key and tls/ from the same time if they were lost.");
    println!(
        "Data deleted since this backup was taken is back; retention will remove it again at the next purge."
    );
}

/// Whether `domovoi audit verify` is part of this binary.
fn audit_verify_exists() -> bool {
    use clap::CommandFactory;
    crate::Cli::command()
        .get_subcommands()
        .find(|command| command.get_name() == "audit")
        .and_then(|audit| {
            audit
                .get_subcommands()
                .find(|sub| sub.get_name() == "verify")
        })
        .is_some()
}
