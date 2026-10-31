//! Manifest rules: R7 (workspace lints), R11 (versions only in the workspace).

use cargo_metadata::Metadata;
use toml::Value;

use super::Violation;

/// Check every workspace member's Cargo.toml.
pub fn check(metadata: &Metadata) -> anyhow::Result<Vec<Violation>> {
    let mut out = Vec::new();
    for pkg in metadata.workspace_packages() {
        let text = std::fs::read_to_string(&pkg.manifest_path)?;
        let doc: Value = toml::from_str(&text)?;
        out.extend(check_manifest(pkg.name.as_ref(), &doc));
    }
    Ok(out)
}

/// Check one parsed manifest.
pub fn check_manifest(name: &str, doc: &Value) -> Vec<Violation> {
    let mut out = Vec::new();
    let lints_ok = doc
        .get("lints")
        .and_then(|l| l.get("workspace"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !lints_ok {
        out.push(Violation {
            rule: "R7",
            location: name.to_string(),
            message: "add `[lints] workspace = true` to Cargo.toml".into(),
        });
    }
    for table in ["dependencies", "dev-dependencies", "build-dependencies"] {
        let Some(deps) = doc.get(table).and_then(Value::as_table) else {
            continue;
        };
        for (dep, spec) in deps {
            let inherits = spec
                .get("workspace")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if !inherits {
                out.push(Violation {
                    rule: "R11",
                    location: format!("{name} [{table}] {dep}"),
                    message: "declare the version in [workspace.dependencies] and use `{ workspace = true }`"
                        .into(),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Value {
        toml::from_str(s).unwrap()
    }

    #[test]
    fn workspace_inherited_manifest_passes() {
        let doc =
            parse("[dependencies]\nserde = { workspace = true }\n[lints]\nworkspace = true\n");
        assert!(check_manifest("x", &doc).is_empty());
    }

    #[test]
    fn missing_lints_is_rejected() {
        let doc = parse("[dependencies]\n");
        assert_eq!(check_manifest("x", &doc)[0].rule, "R7");
    }

    #[test]
    fn inline_version_is_rejected() {
        let doc = parse("[dependencies]\nserde = \"1\"\n[lints]\nworkspace = true\n");
        assert_eq!(check_manifest("x", &doc)[0].rule, "R11");
    }
}
