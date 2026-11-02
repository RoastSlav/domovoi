//! `cargo xtask docs-links`: checks the book in `docs/book/src`.
//!
//! - every page `SUMMARY.md` lists exists and is not a "coming in v0.1" stub;
//! - every relative link points at a file inside the book;
//! - every `#anchor` names a heading in its target page, using mdBook's heading ids.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

/// The book's sources.
pub const BOOK_SRC: &str = "docs/book/src";

/// Runs every check and fails with the list of problems.
pub fn run(root: &Path) -> anyhow::Result<()> {
    let src = root.join(BOOK_SRC);
    let mut pages = BTreeMap::new();
    collect(&src, &src, &mut pages)?;
    let problems = check(&pages);
    if problems.is_empty() {
        println!("docs-links: {} pages, all links resolve", pages.len());
        Ok(())
    } else {
        for problem in &problems {
            println!("error  {problem}");
        }
        anyhow::bail!("docs-links: {} problems", problems.len())
    }
}

/// Every Markdown page under `dir`, keyed by its path relative to the book's `src`.
fn collect(src: &Path, dir: &Path, pages: &mut BTreeMap<PathBuf, String>) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect(src, &path, pages)?;
        } else if path.extension().is_some_and(|e| e == "md") {
            let relative = path.strip_prefix(src)?.to_path_buf();
            pages.insert(relative, std::fs::read_to_string(&path)?);
        }
    }
    Ok(())
}

/// Every problem in `pages`, as `page: message` lines.
pub fn check(pages: &BTreeMap<PathBuf, String>) -> Vec<String> {
    let anchors: BTreeMap<&PathBuf, BTreeSet<String>> = pages
        .iter()
        .map(|(path, text)| (path, heading_ids(text)))
        .collect();
    let mut problems = Vec::new();
    let summary = PathBuf::from("SUMMARY.md");
    match pages.get(&summary) {
        None => problems.push("SUMMARY.md is missing".to_owned()),
        Some(text) => {
            for target in links(text) {
                let page = PathBuf::from(target.split('#').next().unwrap_or_default());
                match pages.get(&page) {
                    None => problems.push(format!("SUMMARY.md: {} does not exist", page.display())),
                    Some(body) if body.to_lowercase().contains("coming in v0.1") => problems.push(
                        format!("{}: still a \"coming in v0.1\" stub", page.display()),
                    ),
                    Some(_) => {}
                }
            }
        }
    }
    for (path, text) in pages.iter().filter(|(path, _)| **path != summary) {
        for target in links(text) {
            if is_external(&target) {
                continue;
            }
            let (file, anchor) = match target.split_once('#') {
                Some((file, anchor)) => (file, Some(anchor)),
                None => (target.as_str(), None),
            };
            let resolved = if file.is_empty() {
                Some(path.clone())
            } else {
                resolve(path, file)
            };
            let Some(resolved) = resolved else {
                problems.push(format!(
                    "{}: {target} points outside the book",
                    path.display()
                ));
                continue;
            };
            let is_page = resolved.extension().is_some_and(|e| e == "md");
            if is_page && !pages.contains_key(&resolved) {
                problems.push(format!("{}: {target} does not exist", path.display()));
                continue;
            }
            if let (true, Some(anchor)) = (is_page, anchor)
                && !anchors
                    .get(&resolved)
                    .is_some_and(|ids| ids.contains(anchor))
            {
                problems.push(format!(
                    "{}: {target} names no heading in {}",
                    path.display(),
                    resolved.display()
                ));
            }
        }
    }
    problems
}

fn is_external(target: &str) -> bool {
    target.contains("://") || target.starts_with("mailto:")
}

/// `link` relative to the page at `from`, or `None` when it leaves the book.
fn resolve(from: &Path, link: &str) -> Option<PathBuf> {
    let mut parts: Vec<Component<'_>> = from
        .parent()
        .map_or_else(Vec::new, |p| p.components().collect());
    for part in Path::new(link).components() {
        match part {
            Component::ParentDir => {
                parts.pop()?;
            }
            Component::CurDir => {}
            other => parts.push(other),
        }
    }
    Some(parts.iter().collect())
}

/// Link and image targets in `text`, outside code.
fn links(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut fenced = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        let line = strip_code(line);
        let mut rest = line.as_str();
        while let Some(start) = rest.find("](") {
            let after = &rest[start + 2..];
            let Some(end) = after.find(')') else { break };
            let target = after[..end].split_whitespace().next().unwrap_or_default();
            if !target.is_empty() {
                out.push(target.to_owned());
            }
            rest = &after[end..];
        }
    }
    out
}

/// `line` without its inline code spans.
fn strip_code(line: &str) -> String {
    line.split('`')
        .enumerate()
        .filter(|(n, _)| n % 2 == 0)
        .map(|(_, part)| part)
        .collect()
}

/// The ids mdBook gives the headings in `text`.
fn heading_ids(text: &str) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut fenced = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced || !line.starts_with('#') {
            continue;
        }
        let title = line.trim_start_matches('#').trim();
        let id = heading_id(title);
        let count = seen.entry(id.clone()).or_insert(0);
        ids.insert(if *count == 0 {
            id.clone()
        } else {
            format!("{id}-{count}")
        });
        *count += 1;
    }
    ids
}

/// mdBook's `normalize_id` over the heading's text, with link targets dropped.
fn heading_id(title: &str) -> String {
    let mut text = String::new();
    let mut rest = title;
    while let Some(start) = rest.find("](") {
        text.push_str(&rest[..start]);
        rest = rest[start..]
            .find(')')
            .map_or("", |end| &rest[start + end + 1..]);
    }
    text.push_str(rest);
    text.chars()
        .filter_map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                Some(c.to_lowercase().collect::<String>())
            } else if c.is_whitespace() {
                Some("-".to_owned())
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::{check, heading_id};

    fn book(pages: &[(&str, &str)]) -> BTreeMap<PathBuf, String> {
        pages
            .iter()
            .map(|(path, text)| (PathBuf::from(path), (*text).to_owned()))
            .collect()
    }

    #[test]
    fn heading_ids_follow_mdbook() {
        assert_eq!(heading_id("Install an agent"), "install-an-agent");
        assert_eq!(heading_id("`[notify.digest]`"), "notifydigest");
        assert_eq!(
            heading_id("Upgrading and uninstalling"),
            "upgrading-and-uninstalling"
        );
    }

    #[test]
    fn broken_links_anchors_and_stubs_are_reported() {
        let pages = book(&[
            (
                "SUMMARY.md",
                "- [A](a.md)\n- [Stub](stub.md)\n- [Gone](gone.md)",
            ),
            (
                "a.md",
                "# A\n\n## Real heading\n\n[ok](#real-heading) [bad](#nope) [b](sub/b.md#top) [out](../../README.md) [web](https://example.com) `[code](x.md)`",
            ),
            ("sub/b.md", "# Top\n\n[back](../a.md#a)"),
            ("stub.md", "# Stub\n\nComing in v0.1."),
        ]);
        let problems = check(&pages);
        let joined = problems.join("\n");
        assert!(joined.contains("gone.md does not exist"), "{joined}");
        assert!(joined.contains("stub.md: still a"), "{joined}");
        assert!(joined.contains("#nope names no heading"), "{joined}");
        assert!(
            joined.contains("../../README.md points outside the book"),
            "{joined}"
        );
        assert!(!joined.contains("real-heading"), "{joined}");
        assert!(!joined.contains("sub/b.md#top"), "{joined}");
        assert!(!joined.contains("x.md"), "code is skipped: {joined}");
        assert_eq!(problems.len(), 4, "{joined}");
    }
}
