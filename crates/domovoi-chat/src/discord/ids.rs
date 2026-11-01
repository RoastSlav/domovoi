//! Button custom IDs and short incident IDs.
//!
//! A custom ID is `dv1:{kind}:{incident}[:{action}:{token}]`, with UUIDs and the hex chat
//! token in unpadded base64url so the longest one, an approval, is 95 characters (Discord
//! allows 100). The hub never sees these strings: the gateway turns them back into a
//! [`CardButton`] before anything is delivered.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use domovoi_core::{ActionId, CardButton, IncidentId};
use uuid::Uuid;

/// The longest custom ID Discord accepts.
pub const MAX_CUSTOM_ID: usize = 100;

/// The version prefix of every custom ID.
const PREFIX: &str = "dv1";

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

/// The custom ID of `button` on `incident`'s card, or `None` for link buttons and for
/// buttons whose ID would not fit.
#[must_use]
pub fn custom_id(incident: IncidentId, button: &CardButton) -> Option<String> {
    let inc = uuid_text(incident.as_uuid());
    let id = match button {
        CardButton::Ack => format!("{PREFIX}:k:{inc}"),
        CardButton::Mute1h => format!("{PREFIX}:m:{inc}"),
        CardButton::Details => format!("{PREFIX}:d:{inc}"),
        CardButton::Approve { action, token } => format!(
            "{PREFIX}:a:{inc}:{}:{}",
            uuid_text(action.as_uuid()),
            token_text(token)?
        ),
        CardButton::Deny { action, token } => format!(
            "{PREFIX}:n:{inc}:{}:{}",
            uuid_text(action.as_uuid()),
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
    let action_and_token = |parts: &mut std::str::Split<'_, char>| {
        let action = ActionId::from_uuid(uuid_of(parts.next()?)?);
        let token = token_of(parts.next()?)?;
        Some((action, token))
    };
    let button = match kind {
        "k" => CardButton::Ack,
        "m" => CardButton::Mute1h,
        "d" => CardButton::Details,
        "a" => {
            let (action, token) = action_and_token(&mut parts)?;
            CardButton::Approve { action, token }
        }
        "n" => {
            let (action, token) = action_and_token(&mut parts)?;
            CardButton::Deny { action, token }
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

    #[test]
    fn custom_id_round_trip_and_length_limit() {
        let incident = IncidentId::new();
        let action = ActionId::new();
        let buttons = [
            CardButton::Ack,
            CardButton::Mute1h,
            CardButton::Details,
            CardButton::Approve {
                action,
                token: token(),
            },
            CardButton::Deny {
                action,
                token: token(),
            },
        ];
        for button in buttons {
            let id = custom_id(incident, &button).unwrap();
            assert!(id.len() <= MAX_CUSTOM_ID, "{id} is {} long", id.len());
            assert!(id.starts_with("dv1:"));
            assert_eq!(parse_custom_id(&id), Some((incident, button)));
        }
        let approve = custom_id(
            incident,
            &CardButton::Approve {
                action,
                token: token(),
            },
        )
        .unwrap();
        assert_eq!(approve.len(), 95, "the longest id has room to spare");

        assert_eq!(custom_id(incident, &CardButton::OpenInWeb { action }), None);
        let not_hex = CardButton::Approve {
            action,
            token: "not hex".to_owned(),
        };
        assert_eq!(custom_id(incident, &not_hex), None);
        let huge = CardButton::Approve {
            action,
            token: "ab".repeat(64),
        };
        assert_eq!(custom_id(incident, &huge), None, "too long for Discord");
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
