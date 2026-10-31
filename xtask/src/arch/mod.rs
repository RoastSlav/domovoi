//! `cargo xtask arch-check`: enforce the rules in ARCHITECTURE.md.

mod git;
mod graph;
mod manifest;
pub mod rules;
mod scan;

use std::fmt;
use std::path::Path;

/// One broken rule, with where and how to fix it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// Rule identifier from ARCHITECTURE.md, e.g. `R1` or `INV-1`.
    pub rule: &'static str,
    /// File or crate the violation is in.
    pub location: String,
    /// What is wrong and how to fix it.
    pub message: String,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}: {}", self.rule, self.location, self.message)
    }
}

/// Run every check and fail if any rule is broken.
pub fn run(root: &Path, base: Option<&str>) -> anyhow::Result<()> {
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(root.join("Cargo.toml"))
        .no_deps()
        .exec()?;

    let mut violations = Vec::new();
    violations.extend(graph::check(&graph::from_metadata(&metadata)));
    violations.extend(manifest::check(&metadata)?);
    violations.extend(scan::check_tree(root)?);
    if let Some(base) = base {
        violations.extend(git::check(root, base)?);
    }

    if violations.is_empty() {
        println!("arch-check: all rules hold");
        return Ok(());
    }
    eprintln!("arch-check: {} violation(s)\n", violations.len());
    for v in &violations {
        eprintln!("  {v}");
    }
    eprintln!("\nSee ARCHITECTURE.md. Changing a rule needs an ADR in docs/adr.");
    anyhow::bail!("architecture rules violated")
}
