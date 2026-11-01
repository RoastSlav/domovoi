//! A [`Card`] as a Discord embed.

use domovoi_core::{Card, CardBadge, Severity, Verdict};
use twilight_model::channel::message::Embed;
use twilight_util::builder::embed::{EmbedBuilder, EmbedFooterBuilder};

use super::ids::short_id;
use super::settings::DiscordSettings;

/// The longest embed title Discord accepts, in characters.
pub const MAX_TITLE: usize = 256;

/// The colour of a resolved card, whatever its badge.
pub const RESOLVED: u32 = 0x0086_8E96;

/// The colour of `badge`: ACTION NEEDED red, MONITOR amber, EXPECTED grey, FIXED green, and
/// severity colours before triage.
#[must_use]
pub fn colour(badge: CardBadge) -> u32 {
    match badge {
        CardBadge::Verdict(Verdict::ActionNeeded) | CardBadge::Severity(Severity::Critical) => {
            0x00C9_2A2A
        }
        CardBadge::Verdict(Verdict::Monitor) | CardBadge::Severity(Severity::Warning) => {
            0x00E6_7700
        }
        CardBadge::Verdict(Verdict::Expected) => 0x006B_6B73,
        CardBadge::Verdict(Verdict::Fixed) => 0x002B_8A3E,
        CardBadge::Severity(Severity::Info) => 0x003B_5BDB,
    }
}

/// The badge as the title shows it, for example `ACTION NEEDED` or `WARNING`.
#[must_use]
pub fn badge_text(badge: CardBadge) -> String {
    match badge {
        CardBadge::Verdict(v) => v.label().to_owned(),
        CardBadge::Severity(s) => s.to_string().to_uppercase(),
    }
}

/// `text` cut to at most `max` characters, ending in "…" when cut.
fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let kept: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

/// The embed title: `{badge} · {host}: {headline}`, with ✅ in front once resolved.
#[must_use]
pub fn title(card: &Card) -> String {
    let text = format!(
        "{} · {}: {}",
        badge_text(card.badge),
        card.host,
        card.headline
    );
    let text = if card.resolved {
        format!("✅ {text}")
    } else {
        text
    };
    clip(&text, MAX_TITLE)
}

/// The embed body: the bullets, then the next step in bold.
#[must_use]
pub fn description(card: &Card) -> String {
    let mut lines: Vec<String> = card
        .bullets
        .iter()
        .take(domovoi_core::card::MAX_BULLETS)
        .map(|b| format!("• {b}"))
        .collect();
    if let Some(next) = &card.next_step {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push(format!("**Next:** {next}"));
    }
    lines.join("\n")
}

/// `card` as an embed whose title links to the incident's web page. The link is also how
/// the gateway finds the incident a thread belongs to.
#[must_use]
pub fn card_embed(card: &Card, settings: &DiscordSettings) -> Embed {
    let colour = if card.resolved {
        RESOLVED
    } else {
        colour(card.badge)
    };
    let mut embed = EmbedBuilder::new()
        .title(title(card))
        .url(settings.incident_url(card.incident))
        .color(colour)
        .footer(EmbedFooterBuilder::new(format!(
            "incident {}",
            short_id(card.incident)
        )));
    let body = description(card);
    if !body.is_empty() {
        embed = embed.description(body);
    }
    embed.build()
}
