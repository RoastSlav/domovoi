//! Contracts for [`TriageCacheRepo`] and [`UsageRepo`].

use domovoi_core::ports::{TriageCacheRepo, UsageRepo, UsageTotal};
use domovoi_core::{HostId, TriageReport, Verdict};
use jiff::Timestamp;
use jiff::civil::date;

use super::{ContractError, ContractResult};
use crate::build::{at, signal};

fn report(headline: &str) -> TriageReport {
    TriageReport {
        headline: headline.to_owned(),
        summary: "summary".to_owned(),
        cause: "cause".to_owned(),
        evidence: Vec::new(),
        verdict: Verdict::Monitor,
        next_step: "watch it".to_owned(),
        proposals: Vec::new(),
        details: String::new(),
    }
}

/// Exercises cache keys, freshness and replacement on an empty `repo`.
pub async fn triage_cache_repo_contract(repo: &dyn TriageCacheRepo) -> ContractResult {
    let host = HostId::new();
    let fp = signal(host, "disk.smart", Some("/dev/sda"))?.fingerprint();
    let other_fp = signal(host, "disk.smart", Some("/dev/sdb"))?.fingerprint();
    assert_eq!(repo.get(&fp, &[1; 32], at(0)).await?, None);

    repo.put(&fp, &[1; 32], &report("first"), at(10)).await?;
    assert_eq!(
        repo.get(&fp, &[1; 32], at(10)).await?,
        Some(report("first"))
    );
    assert_eq!(
        repo.get(&fp, &[1; 32], at(11)).await?,
        None,
        "older than `since`"
    );
    assert_eq!(
        repo.get(&fp, &[2; 32], at(0)).await?,
        None,
        "digest is part of the key"
    );
    assert_eq!(repo.get(&other_fp, &[1; 32], at(0)).await?, None);

    repo.put(&fp, &[1; 32], &report("second"), at(20)).await?;
    assert_eq!(
        repo.get(&fp, &[1; 32], at(15)).await?,
        Some(report("second")),
        "put replaces"
    );
    Ok(())
}

fn utc(y: i16, m: i8, d: i8, h: i8) -> Result<Timestamp, ContractError> {
    date(y, m, d)
        .at(h, 0, 0, 0)
        .in_tz("UTC")
        .map(|z| z.timestamp())
        .map_err(|e| ContractError::Setup(e.to_string()))
}

/// Exercises monthly totals on an empty `repo`.
pub async fn usage_repo_contract(repo: &dyn UsageRepo) -> ContractResult {
    assert_eq!(
        repo.month_total(date(2026, 3, 1)).await?,
        UsageTotal::default()
    );
    let rows = [
        ("anthropic", 100, 10, 1_000, utc(2026, 3, 1, 0)?),
        ("openai", 200, 20, 2_000, utc(2026, 3, 31, 23)?),
        ("anthropic", 400, 40, 4_000, utc(2026, 2, 28, 23)?),
        ("anthropic", 800, 80, 8_000, utc(2026, 4, 1, 0)?),
    ];
    for (provider, tokens_in, tokens_out, cost, when) in rows {
        repo.record(provider, tokens_in, tokens_out, cost, when)
            .await?;
    }
    let march = UsageTotal {
        tokens_in: 300,
        tokens_out: 30,
        cost_micros: 3_000,
    };
    assert_eq!(
        repo.month_total(date(2026, 3, 1)).await?,
        march,
        "UTC calendar month"
    );
    assert_eq!(
        repo.month_total(date(2026, 3, 17)).await?,
        march,
        "any day of the month"
    );
    assert_eq!(
        repo.month_total(date(2025, 3, 17)).await?,
        UsageTotal::default(),
        "year matters"
    );
    Ok(())
}
