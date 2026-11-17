//! Source reachability: which files production and test wiring reach (SRC-1, ADR 0023).
//!
//! [`explore`] follows the declarations [`super::wiring::edges_of`] finds from the Cargo target
//! roots, in two contexts. See [`super::wiring`] for the rules.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use super::Violation;
use super::rules::SRC_RULE;
use super::wiring::{Edge, Target, edges_of};

/// Answer to a request for one workspace-relative file.
pub enum Load {
    /// The file's text.
    Text(String),
    /// No such file.
    Missing,
    /// The path is not allowed to be read, with the reason.
    Refused(String),
}

/// Where source text comes from: the disk in `cargo xtask arch-check`, a map in tests.
pub trait Source {
    /// Load one workspace-relative path with `/` separators.
    fn load(&mut self, rel: &str) -> Load;
}

/// Cargo target roots, workspace-relative.
#[derive(Debug, Default, Clone)]
pub struct Roots {
    /// Roots compiled outside tests: libraries, binaries, build scripts, examples, benches.
    pub production: BTreeSet<String>,
    /// Roots Cargo compiles only as `test` targets.
    pub test: BTreeSet<String>,
}

/// Which files each context reaches, and what could not be followed.
#[derive(Debug, Default)]
pub struct Reach {
    /// Files production module wiring reaches.
    pub production: BTreeSet<String>,
    /// Files test wiring reaches.
    pub test: BTreeSet<String>,
    /// The text of every file read while following the wiring.
    pub texts: BTreeMap<String, String>,
    /// Declarations that could not be followed.
    pub problems: Vec<Violation>,
}

impl Reach {
    /// Files compiled only for tests: reached by the test context and not by production.
    pub fn test_only(&self) -> BTreeSet<String> {
        self.test.difference(&self.production).cloned().collect()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Context {
    Production,
    Test,
}

/// A file to visit: where it is, in which context, and which declaration asked for it.
struct Pending {
    rel: String,
    context: Context,
    from: Option<(String, usize)>,
}

struct Explorer<'a> {
    source: &'a mut dyn Source,
    reach: Reach,
    queue: Vec<Pending>,
}

/// Follow the wiring from `roots`, reading files through `source`. Each file is parsed once,
/// and visited at most once per context.
pub fn explore(source: &mut dyn Source, roots: &Roots) -> Reach {
    let root_set: BTreeSet<&String> = roots.production.iter().chain(&roots.test).collect();
    let mut explorer = Explorer {
        source,
        reach: Reach::default(),
        queue: Vec::new(),
    };
    for (set, context) in [
        (&roots.production, Context::Production),
        (&roots.test, Context::Test),
    ] {
        for root in set {
            explorer.queue.push(Pending {
                rel: root.clone(),
                context,
                from: None,
            });
        }
    }
    let mut edges: BTreeMap<String, Option<Vec<Edge>>> = BTreeMap::new();
    let mut seen: BTreeSet<(String, Context)> = BTreeSet::new();
    while let Some(visit) = explorer.queue.pop() {
        let Pending { rel, context, from } = visit;
        if !seen.insert((rel.clone(), context)) {
            continue;
        }
        if !explorer.reach.texts.contains_key(&rel) {
            match explorer.source.load(&rel) {
                Load::Text(text) => {
                    explorer.reach.texts.insert(rel.clone(), text);
                }
                Load::Missing => continue,
                Load::Refused(why) => {
                    explorer
                        .reach
                        .problems
                        .push(refused(&rel, from.as_ref(), &why));
                    continue;
                }
            }
        }
        match context {
            Context::Production => explorer.reach.production.insert(rel.clone()),
            Context::Test => explorer.reach.test.insert(rel.clone()),
        };
        if !edges.contains_key(&rel) {
            let parsed = explorer
                .reach
                .texts
                .get(&rel)
                .and_then(|text| edges_of(&rel, text, root_set.contains(&rel)));
            edges.insert(rel.clone(), parsed);
        }
        let Some(Some(found)) = edges.get(&rel) else {
            if context == Context::Production {
                explorer.reach.problems.push(Violation {
                    rule: SRC_RULE,
                    location: rel.clone(),
                    message: "cannot parse this file, so the modules and includes it loads cannot be followed (fail-closed)".to_owned(),
                });
            }
            continue;
        };
        for edge in found {
            let child = if context == Context::Test || edge.cfg_test {
                Context::Test
            } else {
                Context::Production
            };
            explorer.follow(&rel, edge, child);
        }
    }
    explorer.reach
}

impl Explorer<'_> {
    /// Resolve one edge and queue the file it loads, or record why it cannot be followed.
    fn follow(&mut self, rel: &str, edge: &Edge, child: Context) {
        let strict = child == Context::Production;
        let candidates = match &edge.target {
            Target::Unsupported(why) => {
                if strict {
                    self.reach.problems.push(problem(rel, edge.line, why));
                }
                return;
            }
            Target::Include(None) => {
                if strict {
                    self.reach.problems.push(problem(
                        rel,
                        edge.line,
                        "this path climbs out of the workspace, so the source it loads is not checked",
                    ));
                }
                return;
            }
            Target::Include(Some(path)) => vec![path.clone()],
            Target::Module(candidates) => candidates.clone(),
        };
        let from = Some((rel.to_owned(), edge.line));
        for candidate in &candidates {
            if !self.reach.texts.contains_key(candidate) {
                match self.source.load(candidate) {
                    Load::Text(text) => {
                        self.reach.texts.insert(candidate.clone(), text);
                    }
                    Load::Missing => continue,
                    Load::Refused(why) => {
                        if strict {
                            self.reach
                                .problems
                                .push(refused(candidate, from.as_ref(), &why));
                        }
                        return;
                    }
                }
            }
            self.queue.push(Pending {
                rel: candidate.clone(),
                context: child,
                from,
            });
            return;
        }
        if strict {
            self.reach.problems.push(problem(
                rel,
                edge.line,
                &format!("no source file found at {}", candidates.join(" or ")),
            ));
        }
    }
}

fn problem(rel: &str, line: usize, why: &str) -> Violation {
    Violation {
        rule: SRC_RULE,
        location: format!("{rel}:{line}"),
        message: why.to_owned(),
    }
}

fn refused(path: &str, from: Option<&(String, usize)>, why: &str) -> Violation {
    let location = match from {
        Some((rel, line)) => format!("{rel}:{line}"),
        None => path.to_owned(),
    };
    Violation {
        rule: SRC_RULE,
        location,
        message: format!("`{path}` is refused: {why}"),
    }
}

/// A [`Source`] over an in-memory tree of workspace-relative files.
pub struct MapSource<'a>(pub &'a BTreeMap<String, String>);

impl Source for MapSource<'_> {
    fn load(&mut self, rel: &str) -> Load {
        match self.0.get(rel) {
            Some(text) => Load::Text(text.clone()),
            None => Load::Missing,
        }
    }
}

/// Reads workspace files from disk, refusing anything that resolves outside the workspace
/// root or into `target/` or `.git/`.
pub struct DiskSource {
    root: PathBuf,
}

impl DiskSource {
    /// A source rooted at the workspace root, which is canonicalized once.
    pub fn new(root: &std::path::Path) -> std::io::Result<Self> {
        Ok(Self {
            root: std::fs::canonicalize(root)?,
        })
    }
}

impl Source for DiskSource {
    fn load(&mut self, rel: &str) -> Load {
        let mut parts: Vec<&str> = rel.split('/').collect();
        parts.pop();
        if parts.iter().any(|part| matches!(*part, "target" | ".git")) {
            return Load::Refused("it is under a build or version-control directory".to_owned());
        }
        let Ok(path) = std::fs::canonicalize(self.root.join(rel)) else {
            return Load::Missing;
        };
        if !path.starts_with(&self.root) {
            return Load::Refused("it resolves outside the workspace".to_owned());
        }
        match std::fs::read_to_string(&path) {
            Ok(text) => Load::Text(text),
            Err(err) => Load::Refused(format!("it cannot be read: {err}")),
        }
    }
}

#[cfg(test)]
#[path = "reach_tests.rs"]
mod tests;
