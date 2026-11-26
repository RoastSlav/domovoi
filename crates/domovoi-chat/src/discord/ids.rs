//! Button custom IDs and short incident IDs.
//!
//! A custom ID is `dv1:{kind}:{incident}[:{execution}:{token}[:{prefix}]]`, with UUIDs, the hex
//! chat token and the plan hash prefix in unpadded base64url. An approval that names its
//! plan is exactly 100 characters with a 32-byte token, which is the most Discord allows.
//! The hub never sees these strings: the gateway turns them back into a [`CardButton`]
//! before anything is delivered.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use domovoi_core::{CardButton, ExecutionId, IncidentId};
use uuid::Uuid;

/// The longest custom ID Discord accepts.
pub const MAX_CUSTOM_ID: usize = 100;

/// The version prefix of every custom ID.
const PREFIX: &str = "dv1";

/// How many hex characters of the plan hash a button carries.
const PREFIX_HEX_LEN: usize = 6;

/// How many bytes [`PREFIX_HEX_LEN`] hex characters decode to.
const PREFIX_BYTES: usize = PREFIX_HEX_LEN / 2;

fn uuid_text(uuid: Uuid) -> String {
    URL_SAFE_NO_PAD.encode(uuid.as_bytes())
}

fn uuid_of(text: &str) -> Option<Uuid> {
    let bytes = URL_SAFE_NO_PAD.decode(text).ok()?;
    Uuid::from_slice(&bytes).ok()
}

/// The hex chat token as base64url, or `None` when it is not hex.
fn token_text(token: &str) -> Option<String> {
    hex::decode(token).ok().map(|b| URL_SAFE_NO_PAD.encode(b))
}

fn token_of(text: &str) -> Option<String> {
    URL_SAFE_NO_PAD.decode(text).ok().map(hex::encode)
}

/// The plan hash prefix as base64url, or `None` unless it is exactly 6 lowercase hex
/// characters.
fn prefix_text(prefix: &str) -> Option<String> {
    let bytes = hex::decode(prefix).ok()?;
    (prefix.len() == PREFIX_HEX_LEN && bytes.len() == PREFIX_BYTES && hex::encode(&bytes) == prefix)
        .then(|| URL_SAFE_NO_PAD.encode(bytes))
}

/// The 6 lowercase hex characters a plan hash prefix segment stands for, or `None` unless
/// the segment is the canonical base64url encoding of 3 bytes.
fn prefix_of(text: &str) -> Option<String> {
    let bytes = URL_SAFE_NO_PAD.decode(text).ok()?;
    (bytes.len() == PREFIX_BYTES && URL_SAFE_NO_PAD.encode(&bytes) == text)
        .then(|| hex::encode(bytes))
}

/// The custom ID of `button` on `incident`'s card, or `None` for link buttons, for
/// approvals whose plan prefix is not 6 hex characters, and for buttons whose ID would not
/// fit.
#[must_use]
pub fn custom_id(incident: IncidentId, button: &CardButton) -> Option<String> {
    let inc = uuid_text(incident.as_uuid());
    let id = match button {
        CardButton::Ack => format!("{PREFIX}:k:{inc}"),
        CardButton::Mute1h => format!("{PREFIX}:m:{inc}"),
        CardButton::Details => format!("{PREFIX}:d:{inc}"),
        CardButton::Approve {
            execution,
            token,
            plan_prefix,
        } => {
            let base = format!(
                "{PREFIX}:a:{inc}:{}:{}",
                uuid_text(execution.as_uuid()),
                token_text(token)?
            );
            match plan_prefix {
                // A card from before structured approvals keeps the 5-segment form.
                None => base,
                Some(prefix) => format!("{base}:{}", prefix_text(prefix)?),
            }
        }
        CardButton::Deny { execution, token } => format!(
            "{PREFIX}:n:{inc}:{}:{}",
            uuid_text(execution.as_uuid()),
            token_text(token)?
        ),
        CardButton::OpenInWeb { .. } => return None,
    };
    (id.len() <= MAX_CUSTOM_ID).then_some(id)
}

/// The incident and button a custom ID stands for, or `None` for anything else.
#[must_use]
pub fn parse_custom_id(id: &str) -> Option<(IncidentId, CardButton)> {
    let mut parts = id.split(':');
    if parts.next()? != PREFIX {
        return None;
    }
    let kind = parts.next()?;
    let incident = IncidentId::from_uuid(uuid_of(parts.next()?)?);
    let execution_and_token = |parts: &mut std::str::Split<'_, char>| {
        let execution = ExecutionId::from_uuid(uuid_of(parts.next()?)?);
        let token = token_of(parts.next()?)?;
        Some((execution, token))
    };
    let button = match kind {
        "k" => CardButton::Ack,
        "m" => CardButton::Mute1h,
        "d" => CardButton::Details,
        "a" => {
            let (execution, token) = execution_and_token(&mut parts)?;
            let plan_prefix = match parts.next() {
                Some(text) => Some(prefix_of(text)?),
                None => None,
            };
            CardButton::Approve {
                execution,
                token,
                plan_prefix,
            }
        }
        "n" => {
            let (execution, token) = execution_and_token(&mut parts)?;
            CardButton::Deny { execution, token }
        }
        _ => return None,
    };
    parts.next().is_none().then_some((incident, button))
}

/// The short form of an incident ID people type: the last 8 hex digits, which come from
/// the UUID's random part (the leading digits are a timestamp and repeat).
#[must_use]
pub fn short_id(incident: IncidentId) -> String {
    let simple = incident.as_uuid().simple().to_string();
    simple[simple.len().saturating_sub(8)..].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token() -> String {
        "ab".repeat(32)
    }

    fn approve(execution: ExecutionId, plan_prefix: Option<&str>) -> CardButton {
        CardButton::Approve {
            execution,
            token: token(),
            plan_prefix: plan_prefix.map(str::to_owned),
        }
    }

    #[test]
    fn custom_id_round_trip_and_length_limit() {
        let incident = IncidentId::new();
        let execution = ExecutionId::new();
        let buttons = [
            CardButton::Ack,
            CardButton::Mute1h,
            CardButton::Details,
            approve(execution, None),
            approve(execution, Some("01ffab")),
            CardButton::Deny {
                execution,
                token: token(),
            },
        ];
        for button in buttons {
            let id = custom_id(incident, &button).unwrap();
            assert!(id.len() <= MAX_CUSTOM_ID, "{id} is {} long", id.len());
            assert!(id.starts_with("dv1:"));
            assert_eq!(parse_custom_id(&id), Some((incident, button)));
        }

        assert_eq!(
            custom_id(incident, &CardButton::OpenInWeb { execution }),
            None
        );
        let not_hex = CardButton::Approve {
            execution,
            token: "not hex".to_owned(),
            plan_prefix: None,
        };
        assert_eq!(custom_id(incident, &not_hex), None);
        let huge = CardButton::Approve {
            execution,
            token: "ab".repeat(64),
            plan_prefix: None,
        };
        assert_eq!(custom_id(incident, &huge), None, "too long for Discord");
    }

    #[test]
    fn approval_custom_id_carries_the_plan_prefix_and_fits_discord() {
        let incident = IncidentId::new();
        let execution = ExecutionId::new();
        let button = approve(execution, Some("0123ab"));
        let id = custom_id(incident, &button).unwrap();
        assert_eq!(id.len(), MAX_CUSTOM_ID, "a 32-byte token fills the budget");
        assert_eq!(id.split(':').count(), 6);
        assert_eq!(id.rsplit(':').next(), Some("ASOr"), "the 3 hash bytes");
        assert_eq!(parse_custom_id(&id), Some((incident, button)));

        // A card from before structured approvals still round-trips, without a prefix.
        let legacy = approve(execution, None);
        let old_id = custom_id(incident, &legacy).unwrap();
        assert_eq!(old_id.split(':').count(), 5);
        assert_eq!(parse_custom_id(&old_id), Some((incident, legacy)));

        for bad in ["0123abff", "0123a", "zzzzzz", "", "0123AB"] {
            let button = approve(execution, Some(bad));
            assert_eq!(custom_id(incident, &button), None, "{bad:?}");
        }

        let base = id.rsplit_once(':').unwrap().0;
        for bad in ["", "ASO", "ASOrr", "!!!!"] {
            assert_eq!(parse_custom_id(&format!("{base}:{bad}")), None, "{bad:?}");
        }
    }

    #[test]
    fn foreign_and_damaged_ids_are_ignored() {
        let incident = IncidentId::new();
        let ack = custom_id(incident, &CardButton::Ack).unwrap();
        for bad in [
            "",
            "dv2:k:x",
            "dv1:z:AAAAAAAAAAAAAAAAAAAAAA",
            "dv1:k:not-base64!",
            "dv1:a:AAAAAAAAAAAAAAAAAAAAAA",
            &format!("{ack}:extra"),
        ] {
            assert_eq!(parse_custom_id(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn short_ids_are_the_random_tail() {
        let id: IncidentId = "01a19721-5d58-7277-a3eb-f866e4efb986".parse().unwrap();
        assert_eq!(short_id(id), "e4efb986");
    }
}
