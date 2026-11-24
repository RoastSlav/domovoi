//! The installer rule INSTALL-1 and the line parser behind it (and behind UNIT-7).
//!
//! `install.sh` is read as lines. The bodies of heredocs (the embedded unit texts) are skipped.
//! The parser finds the created-set, the list `CREATED_PATHS` between `# BEGIN created-set` and
//! `# END created-set`, the line that creates each of those paths, and the first
//! `systemctl enable` or `systemctl start`.

use std::collections::BTreeMap;

use super::Violation;
use super::identity_rules::violation;
use super::rules::INSTALL_SCRIPT;

const BEGIN_MARK: &str = "# BEGIN created-set";
const END_MARK: &str = "# END created-set";

/// Words that make a line one that creates a file or directory.
const CREATE_WORDS: &[&str] = &[
    "mkdir",
    "install",
    "make_dir",
    "make_file",
    "ensure_dir",
    "ensure_file",
    "create_dir",
    "create_file",
    "touch",
    "cp",
    "tee",
];

/// Words that may sit before `systemctl` on the same command line.
const COMMAND_LEADERS: &[&str] = &[
    "&&", "||", ";", "then", "if", "do", "else", "elif", "while", "!", "{", "(", "run", "sudo",
];

/// What the line parser found in an installer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallScript {
    /// The paths of `CREATED_PATHS`, or `None` when the block is absent or defines nothing.
    pub created: Option<Vec<String>>,
    /// The one-based line of the line that creates each created path, when there is one.
    pub creation: BTreeMap<String, usize>,
    /// The one-based line of the first `systemctl enable` or `systemctl start`.
    pub first_enable: Option<usize>,
}

/// Strip one pair of matching quotes.
fn unquote(s: &str) -> &str {
    let s = s.trim();
    for q in ['"', '\''] {
        if let Some(inner) = s.strip_prefix(q).and_then(|r| r.strip_suffix(q)) {
            return inner;
        }
    }
    s
}

/// The heredoc delimiters a line opens, in order.
fn heredoc_openers(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(found) = line[from..].find("<<") {
        let at = from + found;
        let after = &line[at + 2..];
        from = at + 2;
        let spaced = line[..at]
            .chars()
            .next_back()
            .is_none_or(char::is_whitespace);
        if after.starts_with('<') {
            // A here-string, `<<<`, has no body.
            from = at + 3;
            continue;
        }
        let after = after.strip_prefix('-').unwrap_or(after).trim_start();
        let word: String = after
            .trim_start_matches(['"', '\''])
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        let starts_alpha = word.chars().next().is_some_and(|c| !c.is_ascii_digit());
        if spaced && starts_alpha {
            out.push(word);
        }
    }
    out
}

/// A simple `NAME=value` assignment on a line, with a `${NAME:-default}` value resolved to its
/// default.
fn assignment(line: &str) -> Option<(String, String)> {
    let line = line.trim().strip_prefix("export ").unwrap_or(line.trim());
    let (name, value) = line.split_once('=')?;
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    let value = unquote(value.split(" #").next().unwrap_or(value));
    let value = value
        .strip_prefix("${")
        .and_then(|v| v.strip_suffix('}'))
        .and_then(|v| v.split_once(":-").map(|(_, default)| default))
        .unwrap_or(value);
    Some((name.to_string(), unquote(value).to_string()))
}

/// Replace `$NAME` and `${NAME}` by their value, up to three levels deep.
fn expand(text: &str, vars: &BTreeMap<String, String>) -> String {
    let mut current = text.to_string();
    for _ in 0..3 {
        let mut next = current.clone();
        for (name, value) in vars {
            next = next
                .replace(&format!("${{{name}}}"), value)
                .replace(&format!("${name}"), value);
        }
        if next == current {
            break;
        }
        current = next;
    }
    current
}

/// The value of `CREATED_PATHS` in the created-set block, split into paths.
fn created_value(block: &[&str], vars: &BTreeMap<String, String>) -> Option<Vec<String>> {
    let body: Vec<&str> = block
        .iter()
        .copied()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect();
    let text = body.join("\n");
    let value = text.split_once("CREATED_PATHS=")?.1;
    let value = value.trim_start();
    let raw = match value.chars().next() {
        Some(q @ ('"' | '\'')) => value[1..].split(q).next().unwrap_or_default().to_string(),
        _ => value.lines().next().unwrap_or_default().to_string(),
    };
    let raw = expand(&raw.replace("\\\n", " "), vars);
    let paths: Vec<String> = raw
        .split_whitespace()
        .filter(|p| *p != "\\")
        .map(str::to_string)
        .collect();
    (!paths.is_empty()).then_some(paths)
}

/// Whether `line` is a command line that runs `systemctl enable` or `systemctl start`.
fn enables_unit(line: &str) -> bool {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    tokens.iter().enumerate().any(|(i, token)| {
        let leads =
            i == 0 || COMMAND_LEADERS.contains(&tokens[i - 1]) || tokens[i - 1].ends_with(';');
        leads
            && token.trim_matches(['"', '\'']) == "systemctl"
            && tokens[i + 1..]
                .iter()
                .any(|t| matches!(*t, "enable" | "start"))
    })
}

/// Whether `line` has a creating command as one of its words, or redirects into a file.
fn creates_something(line: &str) -> bool {
    line.split(|c: char| c.is_whitespace() || c == ';' || c == '(' || c == ')')
        .any(|w| CREATE_WORDS.contains(&w.trim_matches(['"', '\''])))
        || line.contains(" > ")
        || line.contains(" >> ")
}

/// Whether `line` names `path` itself or a variable that holds exactly it.
fn names_path(line: &str, path: &str, aliases: &[&str]) -> bool {
    let at_boundary = |hay: &str, needle: &str| {
        let mut from = 0;
        while let Some(i) = hay[from..].find(needle) {
            let end = from + i + needle.len();
            let next = hay[end..].chars().next();
            if !next.is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '/')) {
                return true;
            }
            from = end;
        }
        false
    };
    at_boundary(line, path)
        || aliases
            .iter()
            .any(|a| at_boundary(line, &format!("${a}")) || line.contains(&format!("${{{a}}}")))
}

/// Parse an installer script.
pub fn parse(text: &str) -> InstallScript {
    let lines: Vec<&str> = text.lines().collect();
    // Pass 1: which lines are heredoc bodies, comments, or the created-set block.
    let mut skipped = vec![false; lines.len()];
    let mut block: Vec<&str> = Vec::new();
    let mut in_block = false;
    let mut pending: Vec<String> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if let Some(delimiter) = pending.first() {
            skipped[i] = true;
            if line.trim() == delimiter {
                pending.remove(0);
            }
            continue;
        }
        let trimmed = line.trim();
        if trimmed == BEGIN_MARK {
            in_block = true;
            skipped[i] = true;
            continue;
        }
        if trimmed == END_MARK {
            in_block = false;
            skipped[i] = true;
            continue;
        }
        if in_block {
            skipped[i] = true;
            block.push(line);
            continue;
        }
        if trimmed.starts_with('#') || trimmed.is_empty() {
            skipped[i] = true;
            continue;
        }
        pending = heredoc_openers(line);
    }

    let mut vars = BTreeMap::new();
    for (i, line) in lines.iter().enumerate() {
        if !skipped[i]
            && let Some((name, value)) = assignment(line)
        {
            vars.insert(name, value);
        }
    }
    let created = created_value(&block, &vars);

    let mut creation = BTreeMap::new();
    let mut first_enable = None;
    for (i, line) in lines.iter().enumerate() {
        if skipped[i] {
            continue;
        }
        if first_enable.is_none() && enables_unit(line) {
            first_enable = Some(i + 1);
        }
    }
    if let Some(paths) = &created {
        let loop_line = lines.iter().enumerate().find(|(i, l)| {
            !skipped[*i]
                && l.trim_start().starts_with("for ")
                && (l.contains("$CREATED_PATHS") || l.contains("${CREATED_PATHS}"))
        });
        // A loop over the whole set creates each path at the first creating line in its body.
        let loop_creates = loop_line.and_then(|(start, _)| {
            (start + 1..lines.len().min(start + 8))
                .find(|&j| !skipped[j] && creates_something(lines[j]))
                .map(|j| j + 1)
        });
        for path in paths {
            let aliases: Vec<&str> = vars
                .iter()
                .filter(|(_, v)| *v == path)
                .map(|(k, _)| k.as_str())
                .collect();
            let line = lines
                .iter()
                .enumerate()
                .find(|(i, l)| {
                    !skipped[*i] && creates_something(l) && names_path(l, path, &aliases)
                })
                .map(|(i, _)| i + 1);
            if let Some(n) = line.or(loop_creates) {
                creation.insert(path.clone(), n);
            }
        }
    }
    InstallScript {
        created,
        creation,
        first_enable,
    }
}

/// INSTALL-1: every created path is created before the first `systemctl enable` or `start`.
/// A missing created-set block is a violation only with `require_created_set`.
pub fn check(script: &InstallScript, require_created_set: bool) -> Vec<Violation> {
    let Some(paths) = &script.created else {
        return if require_created_set {
            vec![violation(
                "INSTALL-1",
                INSTALL_SCRIPT,
                "no CREATED_PATHS between `# BEGIN created-set` and `# END created-set`",
            )]
        } else {
            Vec::new()
        };
    };
    let mut out = Vec::new();
    for path in paths {
        match (script.creation.get(path), script.first_enable) {
            (None, _) => out.push(violation(
                "INSTALL-1",
                INSTALL_SCRIPT,
                &format!("no line creates `{path}`"),
            )),
            (Some(created), Some(enabled)) if created > &enabled => out.push(violation(
                "INSTALL-1",
                &format!("{INSTALL_SCRIPT}:{created}"),
                &format!(
                    "`{path}` is created after the first systemctl enable or start (line {enabled})"
                ),
            )),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
#[path = "install_script_tests.rs"]
mod tests;
