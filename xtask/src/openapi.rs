//! `cargo xtask openapi`: regenerate `web/src/api/openapi.json` from the hub's routes by
//! running the hidden `domovoi openapi` subcommand, or check that it is up to date.

use std::path::Path;
use std::process::Command;

/// Where the generated document is committed.
pub const SPEC_PATH: &str = "web/src/api/openapi.json";

/// Writes the spec, or with `check` fails when the committed file is stale.
pub fn run(root: &Path, check: bool) -> anyhow::Result<()> {
    println!("==> openapi: cargo run -q -p domovoi -- openapi");
    let output = Command::new("cargo")
        .current_dir(root)
        .args(["run", "-q", "--locked", "-p", "domovoi", "--", "openapi"])
        .output()?;
    if !output.status.success() {
        anyhow::bail!(
            "`domovoi openapi` failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let spec = String::from_utf8(output.stdout)?;
    let path = root.join(SPEC_PATH);
    if check {
        let committed = std::fs::read_to_string(&path).unwrap_or_default();
        if committed.replace("\r\n", "\n") != spec {
            anyhow::bail!(
                "{SPEC_PATH} is stale: run `cargo xtask openapi`, then `npm run gen:api` in web/"
            );
        }
        println!("openapi: {SPEC_PATH} is up to date");
    } else {
        std::fs::write(&path, spec)?;
        println!("openapi: wrote {SPEC_PATH}");
    }
    Ok(())
}
