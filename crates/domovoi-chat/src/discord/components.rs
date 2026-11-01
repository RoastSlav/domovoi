//! A [`Card`]'s buttons as Discord components.

use domovoi_core::{Card, CardButton};
use serenity::all::{ButtonStyle, CreateActionRow, CreateButton};

use super::ids::custom_id;
use super::settings::DiscordSettings;

/// The most buttons in one Discord action row.
pub const ROW_SIZE: usize = 5;

/// The Discord button for `button`, or `None` when it cannot be expressed (an approval
/// token that is not hex).
#[must_use]
pub fn button(
    card: &Card,
    button: &CardButton,
    settings: &DiscordSettings,
) -> Option<CreateButton> {
    let (label, style) = match button {
        CardButton::Ack => ("Ack", ButtonStyle::Secondary),
        CardButton::Mute1h => ("Mute 1h", ButtonStyle::Secondary),
        CardButton::Details => ("Details", ButtonStyle::Secondary),
        CardButton::Approve { .. } => ("Approve", ButtonStyle::Success),
        CardButton::Deny { .. } => ("Deny", ButtonStyle::Danger),
        CardButton::OpenInWeb { .. } => {
            // Destructive actions are approved in the web UI only (INV-6).
            return Some(
                CreateButton::new_link(settings.incident_url(card.incident))
                    .label("Approve in web UI"),
            );
        }
    };
    let id = custom_id(card.incident, button)?;
    Some(CreateButton::new(id).label(label).style(style))
}

/// The card's buttons in rows of up to [`ROW_SIZE`]. A resolved card has no buttons.
#[must_use]
pub fn card_buttons(card: &Card, settings: &DiscordSettings) -> Vec<CreateActionRow> {
    if card.resolved {
        return Vec::new();
    }
    let buttons: Vec<CreateButton> = card
        .buttons
        .iter()
        .filter_map(|b| button(card, b, settings))
        .collect();
    buttons
        .chunks(ROW_SIZE)
        .map(|row| CreateActionRow::Buttons(row.to_vec()))
        .collect()
}
