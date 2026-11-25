//! The source scans RUNNER-1 and RUNNER-2 (ADR 0034): where the runner socket literal and the
//! runner request type may appear, and what the runner's serve code may import.
//!
//! They read the same lexed views as the other line rules in [`super::scan`]. A test source,
//! and an inline `#[cfg(test)]` module, is exempt.

use super::Violation;
use super::identity_rules::violation;
use super::lex::lex;

/// Where RUNNER-1 allows the socket literal and the request type. An entry ending in `/` is a
/// directory, any other entry is one file; test sources are always allowed, and `deploy/` is
/// not scanned.
pub const RUNNER_SOCKET_ALLOWED: &[&str] = &[
    "crates/domovoi-core/src/skill_runner/",
    "crates/domovoi-core/src/skill_runner.rs",
    "crates/domovoi-agent/src/skill_runner/",
    "crates/domovoi-runner/src/serve/",
    "crates/domovoi/src/cmd/skill_runner.rs",
    "crates/domovoi-testkit/",
];

/// The runner socket file name RUNNER-1 looks for in string literals.
pub const RUNNER_SOCKET_LITERAL: &str = "domovoi-skill-runner.sock";

/// The request type RUNNER-1 keeps inside the allowed files.
pub const RUNNER_REQUEST_TYPE: &str = "RunnerRequest";

/// The runner's own source tree, which RUNNER-2 keeps free of helper and agent state.
pub const RUNNER_SERVE_DIR: &str = "crates/domovoi-runner/src/serve/";

/// Names RUNNER-2 refuses in the runner's serve tree: the helper crate and client, and the
/// agent crate and its state types.
pub const RUNNER_SERVE_FORBIDDEN: &[&str] = &[
    "domovoi_helper",
    "HelperClient",
    "HelperRequest",
    "HelperRequestFrame",
    "HELPER_SOCKET",
    "domovoi_agent",
    "AgentStatusSource",
    "AgentStatus",
    "AgentDeps",
    "RequestCtx",
    "RunnerRoute",
    "SkillRoute",
];

/// Check one source file against RUNNER-1 and RUNNER-2. `rel` is the path relative to the
/// workspace root with `/` separators; `test_only` says whether only test builds compile it.
pub fn check_file(rel: &str, text: &str, test_only: bool) -> Vec<Violation> {
    let is_test =
        rel.contains("/tests/") || rel.ends_with("_tests.rs") || rel.ends_with("/tests.rs");
    if test_only || is_test || !rel.starts_with("crates/") {
        return Vec::new();
    }
    let socket_ok = RUNNER_SOCKET_ALLOWED.iter().any(|entry| {
        if entry.ends_with('/') {
            rel.starts_with(entry)
        } else {
            rel == *entry
        }
    });
    let serve = rel.starts_with(RUNNER_SERVE_DIR);
    if socket_ok && !serve {
        return Vec::new();
    }
    let lexed = lex(text);
    let mut out = Vec::new();
    let in_test_module = cfg_test_lines(&lexed.code);
    let strings = text_in_strings(&lexed.code, &lexed.literals);
    let string_lines: Vec<&str> = strings.lines().collect();
    for (i, code) in lexed.code.lines().enumerate() {
        if in_test_module.contains(&i) {
            continue;
        }
        let at = format!("{rel}:{}", i + 1);
        if !socket_ok {
            if has_word(code, RUNNER_REQUEST_TYPE) {
                out.push(violation(
                    "RUNNER-1",
                    &at,
                    &format!("`{RUNNER_REQUEST_TYPE}` is named here"),
                ));
            }
            if string_lines
                .get(i)
                .copied()
                .is_some_and(names_socket_literal)
            {
                out.push(violation(
                    "RUNNER-1",
                    &at,
                    &format!("the literal `{RUNNER_SOCKET_LITERAL}` is spelled here"),
                ));
            }
        }
        if serve {
            for name in RUNNER_SERVE_FORBIDDEN {
                if has_word(code, name) {
                    out.push(violation(
                        "RUNNER-2",
                        &at,
                        &format!("`{name}` is named in the runner's serve code"),
                    ));
                }
            }
        }
    }
    out.dedup();
    out
}

/// Whether `word` occurs in `line` bounded by non-identifier characters.
fn has_word(line: &str, word: &str) -> bool {
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    let mut from = 0;
    while let Some(found) = line[from..].find(word) {
        let start = from + found;
        let end = start + word.len();
        from = end;
        if !line[..start].chars().next_back().is_some_and(ident)
            && !line[end..].chars().next().is_some_and(ident)
        {
            return true;
        }
    }
    false
}

/// Whether a string literal's contents name the runner socket file, and not a longer word
/// such as `domovoi-skill-runner.socket`.
fn names_socket_literal(inside: &str) -> bool {
    let mut from = 0;
    while let Some(found) = inside[from..].find(RUNNER_SOCKET_LITERAL) {
        let end = from + found + RUNNER_SOCKET_LITERAL.len();
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
fn text_in_strings(code: &str, literals: &str) -> String {
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
fn cfg_test_lines(code: &str) -> std::collections::BTreeSet<usize> {
    let lines: Vec<&str> = code.lines().collect();
    let mut out = std::collections::BTreeSet::new();
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

#[cfg(test)]
#[path = "runner_scan_tests.rs"]
mod tests;
