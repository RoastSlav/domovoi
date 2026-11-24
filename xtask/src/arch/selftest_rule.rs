//! SELFTEST-1: every self-test check id, check token and refusal token has a test.
//!
//! The spellings come from the source of `CheckId`, `CheckToken` and `RefusalToken`: the
//! `=> "spelling"` arms of the enum's `as_str`, or of the macro that declares it. A test claims
//! a spelling with a `// covers: <id or token>` comment above a `#[test]` function in
//! `domovoi-helper` or `domovoi-core`.

use std::collections::BTreeSet;
use std::path::Path;

use super::Violation;
use super::identity_rules::violation;
use super::lex::lex;
use super::rules::{SELFTEST_TEST_CRATES, SELFTEST_TOKEN_FILES};

/// The enums whose spellings need a test.
const ENUMS: &[&str] = &["CheckId", "CheckToken", "RefusalToken"];

/// Macros that declare an enum's variants and spellings in one list.
const DECLARING_MACROS: &[&str] = &["check_ids!"];

/// The sources SELFTEST-1 reads: where the enums are defined and where tests may claim them.
#[derive(Debug, Default)]
pub struct Sources {
    /// `(path, text)` of the files that define the enums.
    pub enum_files: Vec<(String, String)>,
    /// `(path, text)` of every Rust file whose comments may claim a spelling.
    pub test_files: Vec<(String, String)>,
}

/// What a run of the rule found.
#[derive(Debug)]
pub enum Outcome {
    /// The enum files do not exist yet; the message says so.
    Skipped(String),
    /// The rule ran; the violations, possibly none.
    Ran(Vec<Violation>),
}

/// The index just past the `}` that closes the `{` at `open`, over chars of the code view.
fn block_end(code: &[char], open: usize) -> usize {
    let mut depth = 0usize;
    for (i, c) in code.iter().enumerate().skip(open) {
        match c {
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
    }
    code.len()
}

/// Every `=> "text"` spelling in `literals[from..to]`.
fn arms(literals: &[char], from: usize, to: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = from;
    while i + 1 < to {
        if literals[i] == '=' && literals[i + 1] == '>' {
            let mut j = i + 2;
            while j < to && literals[j].is_whitespace() {
                j += 1;
            }
            if j < to && literals[j] == '"' {
                let text: String = literals[j + 1..to]
                    .iter()
                    .take_while(|c| **c != '"')
                    .collect();
                i = j + 1 + text.chars().count();
                out.push(text);
                continue;
            }
        }
        i += 1;
    }
    out
}

/// The `{ .. }` block that follows the first occurrence of `head` in the code view.
fn block_after(code: &[char], head: &str) -> Option<(usize, usize)> {
    let text: String = code.iter().collect();
    let mut from = 0;
    while let Some(found) = text[from..].find(head) {
        let at = from + found;
        let before = text[..at].trim_end();
        let defining = before.ends_with("enum") || before.ends_with("impl");
        let char_at = text[..at].chars().count();
        let after = char_at + head.chars().count();
        let open = code[after..].iter().position(|c| !c.is_whitespace());
        if let Some(open) = open
            && code[after + open] == '{'
            && (head.ends_with('!') || !defining)
        {
            let open = after + open;
            return Some((open, block_end(code, open)));
        }
        from = at + head.len();
    }
    None
}

/// The spellings of `name` declared in `text`, or `None` when none can be read there.
fn spellings(name: &str, text: &str) -> Option<Vec<String>> {
    let lexed = lex(text);
    let code: Vec<char> = lexed.code.chars().collect();
    let literals: Vec<char> = lexed.literals.chars().collect();
    let mut found: Vec<String> = Vec::new();
    // Hand-written: the arms of `fn as_str` inside `impl Name { .. }`.
    if let Some((open, end)) = block_after(&code, &format!("impl {name}")) {
        let body: String = code[open..end].iter().collect();
        if let Some(f) = body.find("fn as_str") {
            let fn_open = open + body[..f].chars().count();
            if let Some(brace) = code[fn_open..end].iter().position(|c| *c == '{') {
                let from = fn_open + brace;
                found = arms(&literals, from, block_end(&code, from));
            }
        }
    }
    // Declared through a macro invocation such as `tokens! { Name { A => "a", } }`.
    if found.is_empty()
        && let Some((open, end)) = block_after(&code, name)
    {
        found = arms(&literals, open, end);
    }
    if found.is_empty() && text.contains(&format!("pub enum {name}")) {
        for head in DECLARING_MACROS {
            if let Some((open, end)) = block_after(&code, head) {
                found = arms(&literals, open, end);
            }
        }
    }
    (!found.is_empty()).then_some(found)
}

/// The spellings claimed by `// covers:` comments above a test function.
fn covered(files: &[(String, String)]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (_, text) in files {
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let Some(list) = line.trim().strip_prefix("// covers:") else {
                continue;
            };
            let mut saw_test_attribute = false;
            let mut above_test = false;
            for next in &lines[i + 1..] {
                let next = next.trim();
                if next.starts_with("//") {
                    continue;
                }
                if next.starts_with("#[") {
                    saw_test_attribute |= next.contains("test");
                    continue;
                }
                above_test = saw_test_attribute
                    && (next.starts_with("fn ")
                        || next.starts_with("async fn ")
                        || next.starts_with("pub fn "));
                break;
            }
            if above_test {
                out.extend(
                    list.split(|c: char| c == ',' || c.is_whitespace())
                        .filter(|t| !t.is_empty())
                        .map(str::to_string),
                );
            }
        }
    }
    out
}

/// Run SELFTEST-1 over `sources`.
pub fn check(sources: &Sources) -> Outcome {
    if sources.enum_files.is_empty() {
        return Outcome::Skipped(format!(
            "SELFTEST-1 skipped: none of {} exists yet",
            SELFTEST_TOKEN_FILES.join(", ")
        ));
    }
    let claimed = covered(&sources.test_files);
    let mut out = Vec::new();
    for name in ENUMS {
        let found = sources
            .enum_files
            .iter()
            .find_map(|(path, text)| spellings(name, text).map(|s| (path, s)));
        let Some((path, spellings)) = found else {
            out.push(violation(
                "SELFTEST-1",
                SELFTEST_TOKEN_FILES[0],
                &format!("the spellings of `{name}` were not found"),
            ));
            continue;
        };
        for spelling in spellings {
            if !claimed.contains(&spelling) {
                out.push(violation(
                    "SELFTEST-1",
                    path,
                    &format!("`{name}` spelling `{spelling}` has no `// covers: {spelling}` test"),
                ));
            }
        }
    }
    Outcome::Ran(out)
}

fn collect(dir: &Path, root: &Path, out: &mut Vec<(String, String)>) -> anyhow::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if path.is_dir() {
            if name != "target" && !name.starts_with('.') {
                collect(&path, root, out)?;
            }
        } else if name.ends_with(".rs") {
            let rel = path
                .strip_prefix(root)?
                .to_string_lossy()
                .replace('\\', "/");
            out.push((rel, std::fs::read_to_string(&path)?));
        }
    }
    Ok(())
}

/// Read the sources SELFTEST-1 needs from the workspace at `root`.
pub fn load(root: &Path) -> anyhow::Result<Sources> {
    let mut sources = Sources::default();
    for rel in SELFTEST_TOKEN_FILES {
        let path = root.join(rel);
        if path.is_file() {
            sources
                .enum_files
                .push((rel.to_string(), std::fs::read_to_string(path)?));
        }
    }
    for rel in SELFTEST_TEST_CRATES {
        collect(&root.join(rel), root, &mut sources.test_files)?;
    }
    Ok(sources)
}

#[cfg(test)]
#[path = "selftest_rule_tests.rs"]
mod tests;
