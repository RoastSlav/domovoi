//! What an Operator answer may state. A sentence saying that something is missing or not
//! installed stays in front only when a `host_binary` read of this turn shows it; a stated
//! cause needs a read of the host in this turn. Everything else is moved behind a plain "I do
//! not know" and quoted as the model's unchecked draft. This runs in code, after the model, so
//! no prompt or model behaviour can put an unchecked claim first.

use domovoi_core::ReadToolCall;

use super::meta::EvidenceRecord;
use super::tools::EvidenceClass;

/// Opens an answer whose absence or cause claim no read of this turn shows.
pub const UNVERIFIED_LEAD: &str = "Not verified: I do not know whether what the model's draft says about a cause, or about something missing or not installed, is true. No check of the host in this turn shows it.";

/// Opens an answer that says something is missing which `host_binary` found in this turn.
pub const CONTRADICTED_LEAD: &str = "Wrong in the model's draft: it says something is missing or not installed, but `host_binary` found it on the host in this turn:";

/// Heads the sentences that were moved out of the answer.
pub const DRAFT_HEADING: &str = "Unchecked claims from the model's draft (do not act on them):";

const ABSENCE: [&str; 12] = [
    "not installed",
    "n't installed",
    "not been installed",
    "uninstalled",
    "is missing",
    "was missing",
    "are missing",
    "missing `",
    "not present",
    "is absent",
    "not recorded by",
    "command not found",
];

const CAUSE: [&str; 6] = [
    "because",
    "caused by",
    "due to",
    "the cause is",
    "the cause was",
    "root cause",
];

/// Phrases that make a sentence say it does not know, rather than state something.
const DOUBT: [&str; 7] = [
    "do not know whether",
    "don't know whether",
    "did not check whether",
    "didn't check whether",
    "cannot tell whether",
    "could not check whether",
    "nothing is missing",
];

/// Whether `sentence` says something is missing or not installed.
pub(crate) fn claims_absence(sentence: &str) -> bool {
    let lower = sentence.to_lowercase();
    states(&lower) && ABSENCE.iter().any(|phrase| lower.contains(phrase))
}

fn claims_cause(sentence: &str) -> bool {
    let lower = sentence.to_lowercase();
    states(&lower) && CAUSE.iter().any(|phrase| lower.contains(phrase))
}

fn states(lower: &str) -> bool {
    !lower.trim_end().ends_with('?') && !DOUBT.iter().any(|phrase| lower.contains(phrase))
}

/// Whether `word` appears in `text` as a whole program or package name.
fn names(text: &str, word: &str) -> bool {
    let part = |c: char| c.is_alphanumeric() || matches!(c, '-' | '_' | '+');
    let text = text.to_lowercase();
    let word = word.to_lowercase();
    text.match_indices(&word).any(|(at, _)| {
        !text[..at].chars().next_back().is_some_and(part)
            && !text[at + word.len()..].chars().next().is_some_and(part)
    })
}

/// What the `host_binary` reads of one turn showed, by program or package name.
#[derive(Debug, Default)]
struct Checked {
    found: Vec<String>,
    absent: Vec<String>,
}

fn checked(reads: &[EvidenceRecord]) -> Checked {
    let mut out = Checked::default();
    for read in reads {
        let ReadToolCall::HostBinary { name, package } = &read.call else {
            continue;
        };
        if read.class != EvidenceClass::Observed {
            continue;
        }
        // The two fixed forms the agent's `host_binary` writes.
        let has = |line: String| read.output.contains(&line);
        if has(format!("file `{name}`: found at")) {
            out.found.push(name.clone());
        } else if has(format!("file `{name}`: not found")) {
            out.absent.push(name.clone());
        }
        if let Some(package) = package {
            if has(format!("package `{package}`: recorded as installed")) {
                out.found.push(package.clone());
            } else if has(format!("package `{package}`: not recorded")) {
                out.absent.push(package.clone());
            }
        }
    }
    out
}

/// The sentences of one line, each with its end mark.
fn sentences(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut chars = line.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        if matches!(c, '.' | '!' | '?') && chars.peek().is_some_and(|(_, next)| *next == ' ') {
            out.push(line[start..=at].trim());
            start = at + 1;
        }
    }
    out.push(line[start..].trim());
    out.retain(|sentence| !sentence.is_empty());
    out
}

/// An answer split into what may stay and what no read of this turn shows.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Review {
    rest: String,
    claims: Vec<String>,
    /// Names the draft calls missing that `host_binary` found.
    contradicted: Vec<String>,
}

impl Review {
    /// Whether every claim in the answer is shown by a read of this turn.
    pub(crate) fn is_clean(&self) -> bool {
        self.claims.is_empty()
    }

    /// The answer as it may be shown: unchanged when clean, otherwise led by what is not
    /// known, with the unchecked sentences last.
    pub(crate) fn apply(self, answer: String) -> String {
        if self.is_clean() {
            return answer;
        }
        let mut out = if self.contradicted.is_empty() {
            UNVERIFIED_LEAD.to_owned()
        } else {
            format!("{CONTRADICTED_LEAD} {}.", self.contradicted.join(", "))
        };
        if !self.rest.is_empty() {
            out.push_str("\n\n");
            out.push_str(&self.rest);
        }
        out.push_str("\n\n");
        out.push_str(DRAFT_HEADING);
        for claim in &self.claims {
            out.push_str("\n> ");
            out.push_str(claim);
        }
        out
    }
}

/// Reviews `answer` against the reads of this turn. `read_host` says whether any read of the
/// host's present state succeeded; without one no cause is known either.
pub(crate) fn review(answer: &str, reads: &[EvidenceRecord], read_host: bool) -> Review {
    let checked = checked(reads);
    let mut out = Review::default();
    let mut kept = Vec::new();
    let mut code = false;
    for line in answer.lines() {
        // A fenced block quotes output; it is not the answer stating something.
        if line.trim_start().starts_with("```") {
            code = !code;
            kept.push(line.to_owned());
            continue;
        }
        if code {
            kept.push(line.to_owned());
            continue;
        }
        let mut stays = Vec::new();
        let mut moved = false;
        for sentence in sentences(line) {
            let found: Vec<&String> = checked
                .found
                .iter()
                .filter(|name| names(sentence, name))
                .collect();
            let shown_absent =
                found.is_empty() && checked.absent.iter().any(|name| names(sentence, name));
            let unchecked = if claims_absence(sentence) {
                !shown_absent
            } else {
                !read_host && claims_cause(sentence)
            };
            if !unchecked {
                stays.push(sentence);
                continue;
            }
            moved = true;
            if claims_absence(sentence) {
                for name in found {
                    let name = format!("`{name}`");
                    if !out.contradicted.contains(&name) {
                        out.contradicted.push(name);
                    }
                }
            }
            out.claims.push(sentence.to_owned());
        }
        if !moved {
            kept.push(line.to_owned());
        } else if !stays.is_empty() {
            kept.push(stays.join(" "));
        }
    }
    out.rest = kept.join("\n").trim().to_owned();
    out
}

/// `text` without the lines' sentences that claim something is missing: what a live view may
/// show before the finished answer has been reviewed.
pub(crate) fn before_claim(text: &str) -> &str {
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        for sentence in sentences(line.trim_end()) {
            if claims_absence(sentence) {
                let at = line.find(sentence).unwrap_or(0);
                return &text[..offset + at];
            }
        }
        offset += line.len();
    }
    text
}

#[cfg(test)]
#[path = "claims_tests.rs"]
mod tests;
