//! Source scans of the helper and identity rules: HELPER-1, HELPER-2, IDENT-1, ID-2, ID-3, READ-1.
//!
//! They read the same lexed views as the line rules in [`super::scan`]: identifiers from the
//! code view (comments and literals blanked), literals from the literal view, and the text
//! inside string literals from a third view built here. A test source, and an inline
//! `#[cfg(test)]` module, is exempt from every rule on this page.

use std::collections::BTreeSet;

use super::Violation;
use super::lex::lex;
use super::rules::{
    ACCOUNT_CONSTANTS, ACCOUNT_LITERALS, BOLLARD_ALLOWED, GROUP_CALLS, HELPER_SOCKET_ALLOWED,
    HELPER_SOCKET_LITERAL, HELPER_SOCKET_NAMES, HELPER_STATE_ALLOWED, HELPER_STATE_LITERALS,
    IDENTITY_ALLOWED, IDENTITY_CARDS, INSTANCE_LITERAL_FILE, INSTANCE_NAME_SCOPE,
    SKILL_ACCOUNT_FILE,
};

/// The skill account literal, which IDENT-1 owns.
const SKILL_LITERAL: &str = "\"domovoi-skill\"";

/// Build a violation of rule `id`: the card's message after the specific `detail`.
pub fn violation(id: &'static str, location: &str, detail: &str) -> Violation {
    let message = match IDENTITY_CARDS.iter().find(|card| card.id == id) {
        Some(card) => format!("{detail}; {}", card.message),
        None => detail.to_string(),
    };
    Violation {
        rule: id,
        location: location.to_string(),
        message,
    }
}

/// Whether an allowlist entry covers `rel`: a trailing `/` is a directory, anything else one
/// file.
fn allowed(rel: &str, list: &[&str]) -> bool {
    list.iter().any(|entry| {
        if entry.ends_with('/') {
            rel.starts_with(entry)
        } else {
            rel == *entry
        }
    })
}

/// A test source by location, in addition to the wiring-based `test_only` the caller knows.
fn is_test_path(rel: &str) -> bool {
    rel.contains("/tests/") || rel.ends_with("_tests.rs")
}

/// Where `word` occurs in `line` bounded by non-identifier characters, as byte ranges.
fn word_spans(line: &str, word: &str) -> Vec<(usize, usize)> {
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = line[from..].find(word) {
        let start = from + at;
        let end = start + word.len();
        from = end;
        let before = line[..start].chars().next_back();
        let after = line[end..].chars().next();
        if !before.is_some_and(ident) && !after.is_some_and(ident) {
            out.push((start, end));
        }
    }
    out
}

/// Whether `word` occurs in `line` bounded by non-identifier characters.
fn contains_word(line: &str, word: &str) -> bool {
    !word_spans(line, word).is_empty()
}

/// Whether `line` calls `name`: the word followed by `(`, and not declared with `fn`. A trait
/// method definition and its fake are declarations, not uses.
fn contains_call(line: &str, name: &str) -> bool {
    word_spans(line, name).into_iter().any(|(start, end)| {
        line[end..].trim_start().starts_with('(') && !line[..start].trim_end().ends_with("fn")
    })
}

/// A whole-word occurrence of `needle` in a string's contents: not followed by an identifier
/// character, so `domovoi-helper.socket` is not the socket path.
fn names_literal(inside: &str, needle: &str) -> bool {
    let mut from = 0;
    while let Some(at) = inside[from..].find(needle) {
        let end = from + at + needle.len();
        if !inside[end..]
            .chars()
            .next()
            .is_some_and(|c| c.is_alphanumeric() || c == '_')
        {
            return true;
        }
        from = end;
    }
    false
}

/// The text inside string literals only: a character is kept where the literal view has it and
/// the code view has blanked it. Line breaks are kept so line numbers match.
fn string_contents(code: &str, literals: &str) -> String {
    code.chars()
        .zip(literals.chars())
        .map(|(c, l)| {
            if l == '\n' {
                '\n'
            } else if c == ' ' && l != ' ' {
                l
            } else {
                ' '
            }
        })
        .collect()
}

/// Zero-based indexes of the lines inside inline `#[cfg(test)] mod name { .. }` blocks.
fn cfg_test_lines(code: &str) -> BTreeSet<usize> {
    let lines: Vec<&str> = code.lines().collect();
    let mut out = BTreeSet::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim() == "#[cfg(test)]" {
            let mut j = i + 1;
            while j < lines.len() && lines[j].trim().is_empty() {
                j += 1;
            }
            let opens = lines.get(j).map(|l| l.trim_start()).unwrap_or_default();
            if (opens.starts_with("mod ") || opens.starts_with("pub mod ")) && opens.ends_with('{')
            {
                let mut depth = 0i32;
                let mut k = j;
                while k < lines.len() {
                    depth += lines[k].matches('{').count() as i32;
                    depth -= lines[k].matches('}').count() as i32;
                    out.insert(k);
                    if depth <= 0 {
                        break;
                    }
                    k += 1;
                }
                out.insert(i);
                i = k;
            }
        }
        i += 1;
    }
    out
}

/// Whether `line` assigns a decimal number other than 0 after a word containing `uid` or `gid`.
fn assigns_numeric_id(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    let Some(at) = lower.find("uid").into_iter().chain(lower.find("gid")).min() else {
        return false;
    };
    line[at..]
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .any(|token| !token.is_empty() && token.chars().all(|c| c.is_ascii_digit()) && token != "0")
}

/// Check one file against HELPER-1, HELPER-2, IDENT-1, ID-2 and ID-3. `rel` is the path relative
/// to the workspace root with `/` separators; `test_only` says whether only test builds compile
/// the file.
pub fn check_file(rel: &str, text: &str, test_only: bool) -> Vec<Violation> {
    if test_only || is_test_path(rel) || !rel.starts_with("crates/") {
        return Vec::new();
    }
    let socket_ok = allowed(rel, HELPER_SOCKET_ALLOWED);
    let state_ok = allowed(rel, HELPER_STATE_ALLOWED);
    let identity_ok = allowed(rel, IDENTITY_ALLOWED);
    let skill_ok = rel == SKILL_ACCOUNT_FILE;
    let instance_scope = allowed(rel, INSTANCE_NAME_SCOPE);
    if socket_ok && state_ok && identity_ok && skill_ok && !instance_scope {
        return Vec::new();
    }

    let lexed = lex(text);
    let strings = string_contents(&lexed.code, &lexed.literals);
    let skip = cfg_test_lines(&lexed.code);
    let code_lines: Vec<&str> = lexed.code.lines().collect();
    let literal_lines: Vec<&str> = lexed.literals.lines().collect();
    let string_lines: Vec<&str> = strings.lines().collect();
    let mut out = Vec::new();

    for (i, code) in code_lines.iter().enumerate() {
        if skip.contains(&i) {
            continue;
        }
        let at = format!("{rel}:{}", i + 1);
        let literals = literal_lines.get(i).copied().unwrap_or_default();
        let inside = string_lines.get(i).copied().unwrap_or_default();

        if !socket_ok {
            for name in HELPER_SOCKET_NAMES {
                if contains_word(code, name) {
                    out.push(violation(
                        "HELPER-1",
                        &at,
                        &format!("`{name}` is named here"),
                    ));
                }
            }
            if names_literal(inside, HELPER_SOCKET_LITERAL) {
                out.push(violation(
                    "HELPER-1",
                    &at,
                    &format!("the literal `{HELPER_SOCKET_LITERAL}` is spelled here"),
                ));
            }
        }
        if !state_ok {
            for fragment in HELPER_STATE_LITERALS {
                if inside.contains(fragment) {
                    out.push(violation(
                        "HELPER-2",
                        &at,
                        &format!("the helper state path `{fragment}` is spelled here"),
                    ));
                }
            }
        }
        if !skill_ok && literals.contains(SKILL_LITERAL) {
            out.push(violation(
                "IDENT-1",
                &at,
                "the literal \"domovoi-skill\" is spelled here",
            ));
        }
        if !identity_ok {
            for literal in ACCOUNT_LITERALS {
                // IDENT-1 already reports the skill account literal outside its one file.
                if literals.contains(literal) && *literal != SKILL_LITERAL {
                    out.push(violation(
                        "ID-2",
                        &at,
                        &format!("the account literal {literal} is spelled here"),
                    ));
                }
            }
            for call in GROUP_CALLS {
                if contains_call(code, call) {
                    out.push(violation(
                        "ID-2",
                        &at,
                        &format!("the group-membership call `{call}` is used here"),
                    ));
                }
            }
        }
        if instance_scope {
            let squeezed: String = literals.chars().filter(|c| !c.is_whitespace()).collect();
            let parses = [
                "split('@')",
                "rsplit('@')",
                "split_once('@')",
                "rsplit_once('@')",
                "split(\"@\")",
                "rsplit(\"@\")",
                "split_once(\"@\")",
                "rsplit_once(\"@\")",
            ];
            if parses.iter().any(|p| squeezed.contains(p)) {
                out.push(violation(
                    "ID-3",
                    &at,
                    "a systemd instance name is split on `@`",
                ));
            }
            if rel != INSTANCE_LITERAL_FILE && inside.contains("domovoi-helper@") {
                out.push(violation(
                    "ID-3",
                    &at,
                    "a systemd instance string is spelled here",
                ));
            }
        }
    }

    // Numeric ids next to an account constant: anywhere in production source.
    for (i, code) in code_lines.iter().enumerate() {
        if skip.contains(&i) || !ACCOUNT_CONSTANTS.iter().any(|c| contains_word(code, c)) {
            continue;
        }
        let from = i.saturating_sub(3);
        let to = (i + 3).min(code_lines.len().saturating_sub(1));
        for (j, near) in code_lines.iter().enumerate().take(to + 1).skip(from) {
            if !skip.contains(&j) && assigns_numeric_id(near) {
                out.push(violation(
                    "ID-2",
                    &format!("{rel}:{}", j + 1),
                    "a numeric uid or gid is assigned next to an account constant",
                ));
            }
        }
    }
    out.sort_by(|a, b| (&a.location, a.rule, &a.message).cmp(&(&b.location, b.rule, &b.message)));
    out.dedup();
    out
}

/// READ-1: the `bollard` crate is named only in the checks' Docker source and the helper's
/// read-only Docker client. `rel` is relative to the workspace root; test sources are exempt.
pub fn check_read_file(rel: &str, text: &str, test_only: bool) -> Vec<Violation> {
    if test_only
        || is_test_path(rel)
        || !rel.starts_with("crates/")
        || allowed(rel, BOLLARD_ALLOWED)
    {
        return Vec::new();
    }
    let lexed = lex(text);
    let skip = cfg_test_lines(&lexed.code);
    lexed
        .code
        .lines()
        .enumerate()
        .filter(|(i, code)| !skip.contains(i) && contains_word(code, "bollard"))
        .map(|(i, _)| {
            violation(
                "READ-1",
                &format!("{rel}:{}", i + 1),
                "`bollard` is named here",
            )
        })
        .collect()
}

#[cfg(test)]
#[path = "identity_rules_tests.rs"]
mod tests;
