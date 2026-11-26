//! Checks that the decision records are all in force and that the index lists exactly them.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

/// Files in `docs/adr` that are not decision records.
const NOT_RECORDS: [&str; 2] = ["0000-template.md", "README.md"];

/// Checks every decision record under `docs/adr` against the index in its README.
///
/// A record is in force only when its status line says `Accepted`, and the index must list
/// each record once and nothing else.
pub fn run(root: &Path) -> Result<()> {
    let dir = root.join("docs/adr");
    let mut records = BTreeSet::new();
    let mut problems = Vec::new();
    for entry in fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".md") || NOT_RECORDS.contains(&name.as_str()) {
            continue;
        }
        let text = fs::read_to_string(dir.join(&name))?;
        if let Some(problem) = status_problem(&name, &text) {
            problems.push(problem);
        }
        records.insert(name);
    }
    let index = fs::read_to_string(dir.join("README.md")).context("reading docs/adr/README.md")?;
    problems.extend(index_problems(&records, &index));
    if problems.is_empty() {
        println!(
            "adr-index: {} decision records, all accepted and listed",
            records.len()
        );
        return Ok(());
    }
    bail!(
        "decision records are not in order:\n  {}",
        problems.join("\n  ")
    )
}

/// Why a record is not `Accepted`, if it is not.
fn status_problem(name: &str, text: &str) -> Option<String> {
    let status = text
        .lines()
        .find_map(|line| line.strip_prefix("- Status:"))
        .map(str::trim);
    match status {
        Some("Accepted") => None,
        Some(other) => Some(format!(
            "{name}: status is `{other}`, it must be `Accepted`"
        )),
        None => Some(format!("{name}: no `- Status:` line")),
    }
}

/// Records the index does not list, and index rows that point at no record.
fn index_problems(records: &BTreeSet<String>, index: &str) -> Vec<String> {
    let listed: BTreeSet<String> = index
        .lines()
        .filter(|line| line.starts_with("| ["))
        .filter_map(|line| {
            let target = line.split("](").nth(1)?.split(')').next()?;
            target.ends_with(".md").then(|| target.to_owned())
        })
        .collect();
    let missing = records
        .difference(&listed)
        .map(|name| format!("{name}: not in the index"));
    let dangling = listed
        .difference(records)
        .map(|name| format!("{name}: in the index but the file does not exist"));
    missing.chain(dangling).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_that_is_not_accepted_is_reported() {
        let problem = status_problem("0001-x.md", "# 1. X\n\n- Status: Proposed\n");
        assert!(problem.is_some_and(|text| text.contains("Proposed")));
        assert!(status_problem("0001-x.md", "- Status: Accepted\n").is_none());
        assert!(status_problem("0001-x.md", "no status here").is_some());
    }

    #[test]
    fn the_index_must_list_exactly_the_records() {
        let records: BTreeSet<String> = ["0001-a.md", "0002-b.md"].map(str::to_owned).into();
        let index = "| [0001](0001-a.md) | A |\n| [0003](0003-c.md) | C |\n";
        let problems = index_problems(&records, index);
        assert_eq!(
            problems,
            [
                "0002-b.md: not in the index",
                "0003-c.md: in the index but the file does not exist"
            ]
        );
    }

    #[test]
    fn every_adr_is_accepted_and_indexed() -> Result<()> {
        run(&Path::new(env!("CARGO_MANIFEST_DIR")).join(".."))
    }
}
