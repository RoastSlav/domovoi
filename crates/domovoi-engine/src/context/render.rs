//! The fixed text the model reads for aggregated context.

use std::fmt::Write as _;

use super::layout::{Omission, Section};

/// Aggregated context: one section per source in layer order, and what was left out.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LayeredContext {
    /// The sections, sorted by layer.
    pub sections: Vec<Section>,
    /// What caps and limits left out.
    pub omissions: Vec<Omission>,
}

impl LayeredContext {
    /// The text that goes into a prompt. Empty when there is nothing to say. The caller
    /// redacts it once more before it leaves the engine.
    #[must_use]
    pub fn render(&self) -> String {
        render_all(&self.sections, &self.omissions)
    }
}

/// The section as the model reads it: heading, a blank line, the body and a blank line.
pub(super) fn render_section(section: &Section) -> String {
    let mut blocks: Vec<String> = section.items.iter().map(|item| item.render()).collect();
    for gap in &section.gaps {
        blocks.push(format!(
            "[{} | {} | UNKNOWN: {}] {}\nnothing could be read from this source.",
            section.layer.as_str(),
            gap.source,
            gap.reason,
            section.title,
        ));
    }
    let body = if blocks.is_empty() {
        "None.".to_owned()
    } else {
        blocks.join("\n\n")
    };
    format!("## {}\n\n{body}\n\n", section.title)
}

/// The closing section that names what did not fit.
fn render_omissions(omissions: &[Omission]) -> String {
    let mut out = String::from("## Context left out to fit the size limit\n\n");
    for omission in omissions {
        let noun = if omission.items == 1 { "item" } else { "items" };
        let _ = writeln!(
            out,
            "- {}: {} {noun} from {}",
            omission.layer.as_str(),
            omission.items,
            omission.source
        );
    }
    out.push_str(
        "What is listed here was not shown to you. It is unknown to you, not absent: do not assume there is nothing there.\n\n",
    );
    out
}

/// The whole text, without a trailing newline.
pub(super) fn render_all(sections: &[Section], omissions: &[Omission]) -> String {
    let mut out: String = sections.iter().map(render_section).collect();
    if !omissions.is_empty() {
        out.push_str(&render_omissions(omissions));
    }
    out.truncate(out.trim_end().len());
    out
}
