//! `domovoi audit verify` and `domovoi audit head`.

use std::sync::Arc;

use crate::ExitCode;
use clap::{Args, Subcommand};
use domovoi_core::audit_chain::{ChainStatus, VerifyReport, VerifyScope};
use domovoi_hub::admin::OfflineAdmin;

use super::HubConfigArg;
use crate::init::SystemClock;

/// Audit subcommands.
#[derive(Debug, Subcommand)]
pub enum AuditCommand {
    /// Verify the audit log's hash chain.
    Verify(VerifyArgs),
    /// Print the newest audit row's sequence number and hash.
    Head(HubConfigArg),
}

/// `audit verify`.
#[derive(Debug, Args)]
pub struct VerifyArgs {
    #[command(flatten)]
    config: HubConfigArg,
    /// Check only the newest N rows.
    #[arg(long)]
    last: Option<u64>,
}

pub async fn run(command: AuditCommand) -> anyhow::Result<ExitCode> {
    match command {
        AuditCommand::Verify(args) => verify(args).await,
        AuditCommand::Head(args) => head(args).await,
    }
}

async fn verify(args: VerifyArgs) -> anyhow::Result<ExitCode> {
    let config = args.config.effective().await?;
    let database = config.hub.data_dir.join("hub.db");
    let scope = args.last.map_or(VerifyScope::Full, VerifyScope::Last);
    let admin = OfflineAdmin::open(config, Arc::new(SystemClock)).await?;
    let report = admin.verify_audit(scope).await;
    admin.close().await;
    let report = report?;

    println!("Audit log of {}", database.display());
    let (text, ok) = render_report(&report);
    print!("{text}");
    Ok(if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

async fn head(args: HubConfigArg) -> anyhow::Result<ExitCode> {
    let config = args.effective().await?;
    let admin = OfflineAdmin::open(config, Arc::new(SystemClock)).await?;
    let head = admin.audit_head().await;
    admin.close().await;
    match head? {
        Some((seq, hash)) => println!("seq {seq} hash {}", hex::encode(hash)),
        None => println!("no audit rows yet"),
    }
    Ok(ExitCode::SUCCESS)
}

/// Renders the report as the lines the command prints, and whether verification held.
#[must_use]
pub fn render_report(report: &VerifyReport) -> (String, bool) {
    let mut out = String::new();
    let mut ok = true;
    match &report.chained {
        ChainStatus::Empty => out.push_str("Audit chain: empty\n"),
        ChainStatus::Intact {
            checked,
            head_seq,
            head_hash,
        } => {
            out.push_str(&format!(
                "Audit chain: intact, {checked} rows checked, head seq {head_seq} hash {}\n",
                hex::encode(head_hash)
            ));
        }
        ChainStatus::Broken {
            first_broken_seq,
            reason,
            ..
        } => {
            out.push_str(&format!(
                "Audit chain: BROKEN at seq {first_broken_seq}: {reason}\n"
            ));
            ok = false;
        }
    }
    (out, ok)
}

#[cfg(test)]
mod tests {
    use domovoi_core::audit_chain::{AuditAnchor, BreakReason};

    use super::*;

    fn anchor() -> AuditAnchor {
        AuditAnchor {
            install_id: "install-1".to_owned(),
        }
    }

    #[test]
    fn render_intact() {
        let report = VerifyReport {
            chained: ChainStatus::Intact {
                checked: 311,
                head_seq: 1515,
                head_hash: [0x9f; 32],
            },
            anchor: anchor(),
        };
        let (text, ok) = render_report(&report);
        assert!(ok);
        assert!(text.contains("Audit chain: intact, 311 rows checked, head seq 1515 hash 9f9f"));
    }

    #[test]
    fn render_broken_sets_failure() {
        let report = VerifyReport {
            chained: ChainStatus::Broken {
                checked: 2,
                first_broken_seq: 3,
                reason: BreakReason::HashMismatch,
            },
            anchor: anchor(),
        };
        let (text, ok) = render_report(&report);
        assert!(!ok);
        assert!(text.contains("BROKEN at seq 3: a row was edited"));
    }

    #[test]
    fn render_empty_does_not_fail() {
        let report = VerifyReport {
            chained: ChainStatus::Empty,
            anchor: anchor(),
        };
        let (text, ok) = render_report(&report);
        assert!(ok);
        assert!(text.contains("Audit chain: empty"));
    }
}
