//! Platform-neutral markdown: short enough to read on a phone. Pure.
//!
//! A card is at most 9 lines: header, headline, three waiting actions, three bullets and
//! the next step. A thread reply is one line, a quiet host one line. Chat gateways turn
//! cards into their own formats; this text is for notifiers and fallbacks.

use std::fmt::Write;

use domovoi_core::{Card, CardBadge, Digest, Risk};

/// The badge text, such as `ACTION NEEDED` or `WARNING`.
#[must_use]
pub fn badge(badge: &CardBadge) -> String {
    match badge {
        CardBadge::Verdict(verdict) => verdict.label().to_owned(),
        CardBadge::Severity(severity) => severity.to_string().to_uppercase(),
    }
}

/// A waiting action's risk, lowercase.
fn risk(risk: Risk) -> &'static str {
    match risk {
        Risk::Read => "read",
        Risk::Write => "write",
        Risk::Destructive => "destructive",
    }
}

/// A card as markdown: header, headline, up to three waiting actions with what each will
/// do, up to three bullets and the next step.
#[must_use]
pub fn card(card: &Card) -> String {
    let mut out = String::new();
    let status = if card.resolved { " · resolved" } else { "" };
    _ = writeln!(out, "**[{}] {}**{status}", badge(&card.badge), card.host);
    _ = writeln!(out, "{}", card.headline);
    for plan in &card.plans {
        let args = plan
            .args_text()
            .unwrap_or_else(|| "arguments too long to show here; review in the web UI".to_owned());
        _ = writeln!(
            out,
            "Waiting: {} on {} ({}), {args} · plan {}",
            plan.skill,
            plan.host,
            risk(plan.risk),
            plan.plan_short
        );
    }
    for bullet in &card.bullets {
        _ = writeln!(out, "- {bullet}");
    }
    if let Some(next) = &card.next_step {
        _ = writeln!(out, "Next: {next}");
    }
    out
}

/// One line, for a thread reply.
#[must_use]
pub fn reply(text: &str) -> String {
    let line = text.lines().next().unwrap_or_default().trim();
    format!("{line}\n")
}

/// A digest as markdown: one section per host, then incidents and advisories.
#[must_use]
pub fn digest(digest: &Digest) -> String {
    let mut out = String::new();
    _ = writeln!(out, "**Daily digest, {}**", digest.date);
    for host in &digest.hosts {
        if host.quiet {
            _ = writeln!(out, "{}: healthy, nothing changed", host.host);
            continue;
        }
        _ = writeln!(out, "**{}**", host.host);
        if host.lines.is_empty() {
            _ = writeln!(
                out,
                "- no metric changes; see open incidents and advisories below"
            );
        }
        for line in &host.lines {
            _ = writeln!(out, "- {line}");
        }
    }
    if !digest.open_incidents.is_empty() {
        _ = writeln!(out, "**Open incidents**");
        for (_, line) in &digest.open_incidents {
            _ = writeln!(out, "- {line}");
        }
    }
    if !digest.advisories.is_empty() {
        _ = writeln!(out, "**Advisories**");
        for advisory in &digest.advisories {
            _ = writeln!(out, "- {}", advisory.title);
        }
    }
    out
}
