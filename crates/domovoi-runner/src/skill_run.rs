//! Turning a skill and its validated arguments into one process invocation.

use std::collections::BTreeMap;
use std::path::Path;

use domovoi_core::ports::ProcessSpec;
use domovoi_core::{SkillManifest, ValidatedArgs};
use sha2::{Digest, Sha256};

/// Environment variable naming the skill being run.
pub const SKILL_ENV: &str = "DOMOVOI_SKILL";
/// Prefix of the per-parameter environment variables.
pub const PARAM_ENV_PREFIX: &str = "DOMOVOI_PARAM_";

/// The invocation for `manifest` with `args`, with no shell anywhere.
///
/// - argv is `[entry_path, one value per declared param, in declared order]`. An optional
///   param that was not supplied is an empty element, so positions never shift.
/// - The environment holds `DOMOVOI_SKILL=<name>` and `DOMOVOI_PARAM_<NAME>=<value>` for
///   each supplied param (name upper-cased).
/// - The working directory is the skill folder; timeout, output cap and `run_as` come
///   from the manifest.
#[must_use]
pub fn build_spec(
    manifest: &SkillManifest,
    entry_path: &Path,
    dir: &Path,
    args: &ValidatedArgs,
) -> ProcessSpec {
    let mut env = BTreeMap::from([(SKILL_ENV.to_owned(), manifest.name.to_string())]);
    let mut argv = Vec::with_capacity(manifest.params.len());
    for param in &manifest.params {
        let name = param.name.as_str();
        match args.get(name) {
            Some(value) => {
                argv.push(value.to_owned());
                env.insert(
                    format!("{PARAM_ENV_PREFIX}{}", name.to_ascii_uppercase()),
                    value.to_owned(),
                );
            }
            None => argv.push(String::new()),
        }
    }
    ProcessSpec {
        program: entry_path.to_string_lossy().into_owned(),
        args: argv,
        env,
        cwd: Some(dir.to_string_lossy().into_owned()),
        stdin: None,
        timeout: manifest.timeout,
        max_output_bytes: manifest.max_output_bytes,
        run_as: manifest.run_as.clone(),
    }
}

/// SHA-256 of the full argv (program first) joined with `\0`, for the audit log.
#[must_use]
pub fn argv_hash(spec: &ProcessSpec) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(spec.program.as_bytes());
    for arg in &spec.args {
        hasher.update([0u8]);
        hasher.update(arg.as_bytes());
    }
    hasher.finalize().into()
}

/// [`argv_hash`] as lowercase hex.
#[must_use]
pub fn argv_hash_hex(spec: &ProcessSpec) -> String {
    hex::encode(argv_hash(spec))
}
