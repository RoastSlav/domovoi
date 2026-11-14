//! A [`Card`]'s buttons as Discord components.

use domovoi_core::{ActionId, Card, CardButton};
use twilight_model::channel::message::Component;
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle};

use super::embed::clip;
use super::ids::custom_id;
use super::settings::DiscordSettings;

/// The most buttons in one Discord action row.
pub const ROW_SIZE: usize = 5;

/// The longest button label Discord accepts, in characters.
pub const MAX_LABEL: usize = 80;

fn plain(
    label: &str,
    style: ButtonStyle,
    custom_id: Option<String>,
    url: Option<String>,
) -> Button {
    Button {
        custom_id,
        disabled: false,
        emoji: None,
        id: None,
        label: Some(label.to_owned()),
        sku_id: None,
        style,
        url,
    }
}

/// `verb` and what the plan will do: `Approve restart_container on nas`, or just the verb
/// when the card names no plan for `action`, cut to [`MAX_LABEL`].
fn plan_label(card: &Card, action: ActionId, verb: &str) -> String {
    match card.plans.iter().find(|plan| plan.action == action) {
        Some(plan) => clip(
            &format!("{verb} {} on {}", plan.skill, plan.host),
            MAX_LABEL,
        ),
        None => verb.to_owned(),
    }
}

/// The Discord button for `button`, or `None` when it cannot be expressed (an approval
/// token that is not hex).
#[must_use]
pub fn button(card: &Card, button: &CardButton, settings: &DiscordSettings) -> Option<Button> {
    let (label, style) = match button {
        CardButton::Ack => ("Ack".to_owned(), ButtonStyle::Secondary),
        CardButton::Mute1h => ("Mute 1h".to_owned(), ButtonStyle::Secondary),
        CardButton::Details => ("Details".to_owned(), ButtonStyle::Secondary),
        CardButton::Approve { action, .. } => {
            (plan_label(card, *action, "Approve"), ButtonStyle::Success)
        }
        CardButton::Deny { action, .. } => (plan_label(card, *action, "Deny"), ButtonStyle::Danger),
        CardButton::OpenInWeb { .. } => {
            // Destructive actions are approved in the web UI only (INV-6). A link button has
            // no custom id, so pressing it never reaches the hub.
            return Some(plain(
                "Approve in web UI",
                ButtonStyle::Link,
                None,
                Some(settings.incident_url(card.incident)),
            ));
        }
    };
    let id = custom_id(card.incident, button)?;
    Some(plain(&label, style, Some(id), None))
}

/// The card's buttons in rows of up to [`ROW_SIZE`]. A resolved card has no buttons.
#[must_use]
pub fn card_buttons(card: &Card, settings: &DiscordSettings) -> Vec<Component> {
    if card.resolved {
        return Vec::new();
    }
    let buttons: Vec<Component> = card
        .buttons
        .iter()
        .filter_map(|b| button(card, b, settings))
        .map(Component::Button)
        .collect();
    buttons
        .chunks(ROW_SIZE)
        .map(|row| {
            Component::ActionRow(ActionRow {
                components: row.to_vec(),
                id: None,
            })
        })
        .collect()
}
