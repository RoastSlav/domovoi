//! Wire shapes for the audit log routes and the mappers that build them.

use domovoi_core::audit_chain::{
    AuditAnchor, BreakReason, ChainRow, ChainStatus, LegacyStatus, VerifyReport,
};
use domovoi_core::ports::Redactor;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

/// The most bytes of an entry's detail text one response carries.
pub const MAX_DETAIL_BYTES: usize = 4096;

/// Filters and paging for the audit entry list.
#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AuditQuery {
    /// `legacy` or `chained`: only that part of the history.
    pub history: Option<String>,
    /// Only entries of this exact kind, at most 100 characters.
    pub kind: Option<String>,
    /// Where the previous page ended.
    pub cursor: Option<String>,
    /// Entries per page, 1 to 200; default 50.
    pub limit: Option<u16>,
}

/// Which check to run.
#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct VerificationQuery {
    /// `recent` (default) checks the newest chained entries and counts the older ones;
    /// `full` checks everything.
    pub scope: Option<String>,
}

/// One audit entry with its chain position.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct AuditRowDto {
    /// The entry's sequence number.
    pub seq: u64,
    /// When it was recorded, RFC 3339.
    pub at: String,
    /// Who did it.
    pub actor: String,
    /// What kind of entry it is.
    pub kind: String,
    /// What it is about.
    pub subject: String,
    /// The entry's detail as compact JSON text, secrets removed, cut at 4096 bytes.
    pub detail: String,
    /// Whether `detail` was cut.
    pub detail_truncated: bool,
    /// `legacy` (written before the chain) or `chained`.
    pub history: String,
    /// The entry's hash as 64 lower-case hex characters; `null` on a legacy entry.
    pub hash: Option<String>,
}

/// One page of audit entries, newest first.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct AuditPageDto {
    /// The highest sequence number written before the chain began; 0 on a fresh install.
    pub legacy_last_id: u64,
    /// Pass as `cursor` for the next page; `null` on the last.
    pub next_cursor: Option<String>,
    /// The entries.
    pub items: Vec<AuditRowDto>,
}

/// The legacy part of a verification.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct LegacyHistoryDto {
    /// `verified`, `count_only`, `mismatch` or `not_verifiable`.
    pub status: String,
    /// Entries counted (`verified`, `count_only`).
    pub count: Option<u64>,
    /// Entries the anchor recorded (`mismatch`).
    pub expected_count: Option<u64>,
    /// Entries found (`mismatch`).
    pub actual_count: Option<u64>,
    /// Whether the digest still matched (`mismatch`).
    pub digest_matches: Option<bool>,
}

/// The chained part of a verification.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct ChainedHistoryDto {
    /// `empty`, `intact` or `broken`.
    pub status: String,
    /// Entries checked (`intact`, `broken`).
    pub checked: Option<u64>,
    /// The newest checked entry (`intact`).
    pub head_seq: Option<u64>,
    /// Its hash (`intact`).
    pub head_hash: Option<String>,
    /// The first entry that does not match (`broken`).
    pub first_broken_seq: Option<u64>,
    /// A stable code for why (`broken`).
    pub break_reason: Option<String>,
    /// The reason in words (`broken`).
    pub break_text: Option<String>,
}

/// The anchor over the history written before the chain.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct AuditAnchorDto {
    /// The install this log belongs to.
    pub install_id: String,
    /// The highest legacy sequence number.
    pub legacy_last_id: u64,
    /// How many legacy entries there were.
    pub legacy_count: u64,
    /// The digest of the legacy entries, hex.
    pub legacy_digest: String,
    /// The anchor's own hash, hex.
    pub anchor_hash: String,
}

/// The newest stored entry's position.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct HeadDto {
    /// Its sequence number.
    pub seq: u64,
    /// Its hash, hex.
    pub hash: String,
}

/// The result of checking the audit log.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct AuditVerificationDto {
    /// `recent` or `full`.
    pub scope: String,
    /// How many of the newest chained entries a `recent` check covers.
    pub recent_limit: u64,
    /// When the check finished, RFC 3339.
    pub checked_at: String,
    /// The legacy part.
    pub legacy: LegacyHistoryDto,
    /// The chained part.
    pub chained: ChainedHistoryDto,
    /// The anchor both parts are judged against.
    pub anchor: AuditAnchorDto,
    /// The newest stored entry; `null` before the first chained entry.
    pub head: Option<HeadDto>,
}

/// Text cut to at most `max` bytes on a character boundary; whether it was cut.
fn cut(text: String, max: usize) -> (String, bool) {
    if text.len() <= max {
        return (text, false);
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_owned(), true)
}

/// Maps a stored row to its wire shape. History is decided by `legacy_last_id`, never by
/// whether the row has a hash.
pub fn row_dto(row: &ChainRow, legacy_last_id: u64, redactor: &dyn Redactor) -> AuditRowDto {
    let text = if row.event.detail.is_null() {
        String::new()
    } else {
        redactor.redact(&row.event.detail.to_string())
    };
    let (detail, detail_truncated) = cut(text, MAX_DETAIL_BYTES);
    AuditRowDto {
        seq: row.seq,
        at: row.event.at.to_string(),
        actor: row.event.actor.clone(),
        kind: row.event.kind.clone(),
        subject: row.event.subject.clone(),
        detail,
        detail_truncated,
        history: if row.seq <= legacy_last_id {
            "legacy"
        } else {
            "chained"
        }
        .to_owned(),
        hash: row.hash.as_ref().map(hex::encode),
    }
}

/// The stable code of a break reason.
fn reason_code(reason: BreakReason) -> &'static str {
    match reason {
        BreakReason::MissingChainFields => "missing_chain_fields",
        BreakReason::HashMismatch => "hash_mismatch",
        BreakReason::LinkMismatch => "link_mismatch",
        BreakReason::SequenceGap => "sequence_gap",
        BreakReason::CheckpointMismatch => "checkpoint_mismatch",
    }
}

fn legacy_dto(status: &LegacyStatus) -> LegacyHistoryDto {
    let empty = LegacyHistoryDto {
        status: String::new(),
        count: None,
        expected_count: None,
        actual_count: None,
        digest_matches: None,
    };
    match status {
        LegacyStatus::Verified { count } => LegacyHistoryDto {
            status: "verified".to_owned(),
            count: Some(*count),
            ..empty
        },
        LegacyStatus::CountOnly { count } => LegacyHistoryDto {
            status: "count_only".to_owned(),
            count: Some(*count),
            ..empty
        },
        LegacyStatus::Mismatch {
            expected_count,
            actual_count,
            digest_matches,
        } => LegacyHistoryDto {
            status: "mismatch".to_owned(),
            expected_count: Some(*expected_count),
            actual_count: Some(*actual_count),
            digest_matches: Some(*digest_matches),
            ..empty
        },
        LegacyStatus::NotVerifiable => LegacyHistoryDto {
            status: "not_verifiable".to_owned(),
            ..empty
        },
    }
}

fn chained_dto(status: &ChainStatus) -> ChainedHistoryDto {
    let empty = ChainedHistoryDto {
        status: String::new(),
        checked: None,
        head_seq: None,
        head_hash: None,
        first_broken_seq: None,
        break_reason: None,
        break_text: None,
    };
    match status {
        ChainStatus::Empty => ChainedHistoryDto {
            status: "empty".to_owned(),
            ..empty
        },
        ChainStatus::Intact {
            checked,
            head_seq,
            head_hash,
        } => ChainedHistoryDto {
            status: "intact".to_owned(),
            checked: Some(*checked),
            head_seq: Some(*head_seq),
            head_hash: Some(hex::encode(head_hash)),
            ..empty
        },
        ChainStatus::Broken {
            checked,
            first_broken_seq,
            reason,
        } => ChainedHistoryDto {
            status: "broken".to_owned(),
            checked: Some(*checked),
            first_broken_seq: Some(*first_broken_seq),
            break_reason: Some(reason_code(*reason).to_owned()),
            break_text: Some(reason.to_string()),
            ..empty
        },
    }
}

fn anchor_dto(anchor: &AuditAnchor) -> AuditAnchorDto {
    AuditAnchorDto {
        install_id: anchor.install_id.clone(),
        legacy_last_id: anchor.legacy_last_id,
        legacy_count: anchor.legacy_count,
        legacy_digest: hex::encode(anchor.legacy_digest),
        anchor_hash: hex::encode(anchor.hash()),
    }
}

/// Maps a verification report, the check's scope and the newest stored entry to the response.
pub fn verification_dto(
    report: &VerifyReport,
    scope: &str,
    recent_limit: u64,
    checked_at: String,
    head: Option<(u64, [u8; 32])>,
) -> AuditVerificationDto {
    AuditVerificationDto {
        scope: scope.to_owned(),
        recent_limit,
        checked_at,
        legacy: legacy_dto(&report.legacy),
        chained: chained_dto(&report.chained),
        anchor: anchor_dto(&report.anchor),
        head: head.map(|(seq, hash)| HeadDto {
            seq,
            hash: hex::encode(hash),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domovoi_core::audit_chain::AuditAnchor;

    fn report(legacy: LegacyStatus, chained: ChainStatus) -> VerifyReport {
        VerifyReport {
            legacy,
            chained,
            anchor: AuditAnchor {
                install_id: "i".to_owned(),
                legacy_last_id: 5,
                legacy_count: 5,
                legacy_digest: [2; 32],
            },
        }
    }

    #[test]
    fn verify_report_mapping_covers_every_core_variant() {
        let legacy = [
            (LegacyStatus::Verified { count: 3 }, "verified"),
            (LegacyStatus::CountOnly { count: 3 }, "count_only"),
            (
                LegacyStatus::Mismatch {
                    expected_count: 5,
                    actual_count: 4,
                    digest_matches: false,
                },
                "mismatch",
            ),
            (LegacyStatus::NotVerifiable, "not_verifiable"),
        ];
        for (status, name) in legacy {
            let dto = verification_dto(
                &report(status, ChainStatus::Empty),
                "full",
                10,
                "t".to_owned(),
                None,
            );
            assert_eq!(dto.legacy.status, name);
            assert_eq!(dto.chained.status, "empty");
            assert!(dto.head.is_none());
        }
        let mismatch = legacy_dto(&LegacyStatus::Mismatch {
            expected_count: 5,
            actual_count: 4,
            digest_matches: false,
        });
        assert_eq!(
            (
                mismatch.expected_count,
                mismatch.actual_count,
                mismatch.digest_matches
            ),
            (Some(5), Some(4), Some(false))
        );
        assert_eq!(mismatch.count, None);
        let intact = chained_dto(&ChainStatus::Intact {
            checked: 2,
            head_seq: 7,
            head_hash: [0xab; 32],
        });
        assert_eq!(intact.head_hash, Some("ab".repeat(32)));
        assert_eq!(intact.first_broken_seq, None);
        let codes = [
            (BreakReason::MissingChainFields, "missing_chain_fields"),
            (BreakReason::HashMismatch, "hash_mismatch"),
            (BreakReason::LinkMismatch, "link_mismatch"),
            (BreakReason::SequenceGap, "sequence_gap"),
            (BreakReason::CheckpointMismatch, "checkpoint_mismatch"),
        ];
        for (reason, code) in codes {
            let dto = chained_dto(&ChainStatus::Broken {
                checked: 1,
                first_broken_seq: 4,
                reason,
            });
            assert_eq!(dto.status, "broken");
            assert_eq!(dto.break_reason.as_deref(), Some(code));
            assert_eq!(dto.break_text, Some(reason.to_string()));
            assert_eq!(dto.head_seq, None);
        }
        let dto = verification_dto(
            &report(LegacyStatus::Verified { count: 5 }, ChainStatus::Empty),
            "recent",
            10,
            "t".to_owned(),
            Some((9, [0xcd; 32])),
        );
        assert_eq!(dto.anchor.anchor_hash.len(), 64);
        assert_eq!(dto.head.map(|h| h.seq), Some(9));
    }
}
