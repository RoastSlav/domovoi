//! Dependency graph rules: R1, R2 (external allowlists), R5, R9, R11 (banned crates).

use cargo_metadata::{DependencyKind, Metadata};

use super::Violation;
use super::rules::{ANYHOW_ALLOWED, BANNED, CRATES, DEV_ONLY};

/// Kind of a declared dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Normal,
    Dev,
    Build,
}

/// A declared dependency of a workspace package.
#[derive(Debug, Clone)]
pub struct Dep {
    pub name: String,
    pub kind: Kind,
    pub internal: bool,
}

/// A workspace package and its declared dependencies.
#[derive(Debug, Clone)]
pub struct Package {
    pub name: String,
    pub deps: Vec<Dep>,
}

/// Build the package list from `cargo metadata`.
pub fn from_metadata(metadata: &Metadata) -> Vec<Package> {
    let members: Vec<String> = metadata
        .workspace_packages()
        .iter()
        .map(|p| p.name.to_string())
        .collect();
    metadata
        .workspace_packages()
        .iter()
        .map(|p| Package {
            name: p.name.to_string(),
            deps: p
                .dependencies
                .iter()
                .map(|d| Dep {
                    name: d.name.clone(),
                    kind: match d.kind {
                        DependencyKind::Development => Kind::Dev,
                        DependencyKind::Build => Kind::Build,
                        _ => Kind::Normal,
                    },
                    internal: members.contains(&d.name),
                })
                .collect(),
        })
        .collect()
}

/// Check the graph against the rules table.
pub fn check(packages: &[Package]) -> Vec<Violation> {
    let mut out = Vec::new();
    for pkg in packages {
        let Some(rule) = CRATES.iter().find(|r| r.name == pkg.name) else {
            out.push(Violation {
                rule: "R1",
                location: pkg.name.clone(),
                message:
                    "crate has no entry in xtask/src/arch/rules.rs; adding a crate needs an ADR"
                        .into(),
            });
            continue;
        };
        for dep in &pkg.deps {
            let normal = dep.kind != Kind::Dev;
            if DEV_ONLY.contains(&dep.name.as_str()) && normal {
                out.push(violation(
                    "R5",
                    pkg,
                    &format!("`{}` may only be a dev-dependency", dep.name),
                ));
            }
            if BANNED.contains(&dep.name.as_str()) {
                out.push(violation(
                    "R11",
                    pkg,
                    &format!("`{}` is banned; use rustls", dep.name),
                ));
            }
            if !normal {
                continue;
            }
            if dep.internal && !rule.internal.contains(&dep.name.as_str()) {
                out.push(violation(
                    "R1",
                    pkg,
                    &format!("must not depend on `{}` (see ARCHITECTURE.md §4)", dep.name),
                ));
            }
            if dep.name == "anyhow" && !ANYHOW_ALLOWED.contains(&pkg.name.as_str()) {
                out.push(violation("R9", pkg, "libraries use thiserror, not anyhow"));
            }
            if let Some(allow) = rule.external_allowlist
                && !dep.internal
                && !allow.contains(&dep.name.as_str())
            {
                out.push(violation(
                    "R2",
                    pkg,
                    &format!(
                        "external dependency `{}` is not on this crate's allowlist",
                        dep.name
                    ),
                ));
            }
        }
    }
    out
}

fn violation(rule: &'static str, pkg: &Package, message: &str) -> Violation {
    Violation {
        rule,
        location: pkg.name.clone(),
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(name: &str, deps: &[(&str, Kind, bool)]) -> Package {
        Package {
            name: name.into(),
            deps: deps
                .iter()
                .map(|(n, k, i)| Dep {
                    name: (*n).into(),
                    kind: *k,
                    internal: *i,
                })
                .collect(),
        }
    }

    #[test]
    fn allowed_graph_passes() {
        let pkgs = vec![
            pkg("domovoi-core", &[("serde", Kind::Normal, false)]),
            pkg(
                "domovoi-engine",
                &[
                    ("domovoi-core", Kind::Normal, true),
                    ("tokio", Kind::Normal, false),
                ],
            ),
            pkg(
                "domovoi-store",
                &[
                    ("domovoi-core", Kind::Normal, true),
                    ("domovoi-testkit", Kind::Dev, true),
                ],
            ),
        ];
        assert!(check(&pkgs).is_empty(), "{:?}", check(&pkgs));
    }

    #[test]
    fn adapter_depending_on_adapter_is_rejected() {
        let pkgs = vec![pkg("domovoi-llm", &[("domovoi-store", Kind::Normal, true)])];
        let v = check(&pkgs);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "R1");
    }

    #[test]
    fn engine_depending_on_sqlx_is_rejected() {
        let pkgs = vec![pkg("domovoi-engine", &[("sqlx", Kind::Normal, false)])];
        assert_eq!(check(&pkgs)[0].rule, "R2");
    }

    #[test]
    fn testkit_as_normal_dependency_is_rejected() {
        let pkgs = vec![pkg(
            "domovoi-hub",
            &[("domovoi-testkit", Kind::Normal, true)],
        )];
        assert!(check(&pkgs).iter().any(|v| v.rule == "R5"));
    }

    #[test]
    fn anyhow_in_library_is_rejected() {
        let pkgs = vec![pkg("domovoi-chat", &[("anyhow", Kind::Normal, false)])];
        assert_eq!(check(&pkgs)[0].rule, "R9");
    }

    #[test]
    fn unknown_crate_is_rejected() {
        let pkgs = vec![pkg("domovoi-new", &[])];
        assert_eq!(check(&pkgs)[0].rule, "R1");
    }

    #[test]
    fn openssl_is_banned_even_as_dev_dependency() {
        let pkgs = vec![pkg("domovoi-llm", &[("openssl", Kind::Dev, false)])];
        assert_eq!(check(&pkgs)[0].rule, "R11");
    }
}
