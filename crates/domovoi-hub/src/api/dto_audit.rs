//! Wire shapes for the audit log routes and the mappers that build them.

use domovoi_core::audit_chain::{AuditAnchor, BreakReason, ChainRow, ChainStatus, VerifyReport};
use domovoi_core::ports::Redactor;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

/// The most bytes of an entry's detail text one response carries.
pub const MAX_DETAIL_BYTES: usize = 4096;

/// Filters and paging for the audit entry list.
#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AuditQuery {
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
    /// `recent` (default) checks the newest entries;
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
    /// The entry's hash as 64 lower-case hex characters; `null` if the entry lost it.
    pub hash: Option<String>,
}

/// One page of audit entries, newest first.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct AuditPageDto {
    /// Pass as `cursor` for the next page; `null` on the last.
    pub next_cursor: Option<String>,
    /// The entries.
    pub items: Vec<AuditRowDto>,
}

/// The chain's part of a verification.
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

/// The anchor the chain starts from.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct AuditAnchorDto {
    /// The install this log belongs to.
    pub install_id: String,
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
    /// How many of the newest entries a `recent` check covers.
    pub recent_limit: u64,
    /// When the check finished, RFC 3339.
    pub checked_at: String,
    /// The chain's status.
    pub chained: ChainedHistoryDto,
    /// The anchor the chain is judged against.
    pub anchor: AuditAnchorDto,
    /// The newest stored entry; `null` before the first entry.
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

/// Maps a stored row to its wire shape.
pub fn row_dto(row: &ChainRow, redactor: &dyn Redactor) -> AuditRowDto {
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

    fn report(chained: ChainStatus) -> VerifyReport {
        VerifyReport {
            chained,
            anchor: AuditAnchor {
                install_id: "i".to_owned(),
            },
        }
    }

    #[test]
    fn verify_report_mapping_covers_every_core_variant() {
        let dto = verification_dto(
            &report(ChainStatus::Empty),
            "full",
            10,
            "t".to_owned(),
            None,
        );
        assert_eq!(dto.chained.status, "empty");
        assert!(dto.head.is_none());
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
            &report(ChainStatus::Empty),
            "recent",
            10,
            "t".to_owned(),
            Some((9, [0xcd; 32])),
        );
        assert_eq!(dto.anchor.anchor_hash.len(), 64);
        assert_eq!(dto.head.map(|h| h.seq), Some(9));
    }
}
