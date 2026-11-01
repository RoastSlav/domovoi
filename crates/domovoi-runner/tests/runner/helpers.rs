//! Shared helpers: real binaries, executors and skill folders built in a tempdir.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::ports::{ProcessError, ProcessExecutor, ProcessOutput, ProcessSpec};
use domovoi_runner::SandboxExecutor;
use domovoi_testkit::ListRedactor;
use jiff::SignedDuration;

/// The secret the test executor redacts.
pub const SECRET: &str = "s3cr3t-token";

/// `path` if it exists on this machine; otherwise prints why the test is skipped.
pub fn bin(path: &str) -> Option<String> {
    if Path::new(path).exists() {
        Some(path.to_owned())
    } else {
        eprintln!("skipping: {path} is not available");
        None
    }
}

/// The first of `paths` that exists.
pub fn any_bin(paths: &[&str]) -> Option<String> {
    paths
        .iter()
        .find(|p| Path::new(p).exists())
        .map(|p| (*p).to_owned())
        .or_else(|| {
            eprintln!("skipping: none of {paths:?} is available");
            None
        })
}

pub fn executor() -> TestExecutor {
    TestExecutor(SandboxExecutor::new(Arc::new(ListRedactor::new([SECRET]))))
}

/// The sandbox executor, retrying a spawn that fails with "Text file busy".
///
/// Tests write scripts just before running them, while other tests in this process
/// fork. A child forked in that moment holds the script open for writing until it
/// execs, and exec of a file open for writing fails with ETXTBSY. Production code
/// never writes a program it is about to run, so the retry lives here.
pub struct TestExecutor(SandboxExecutor);

#[async_trait]
impl ProcessExecutor for TestExecutor {
    async fn run(&self, spec: ProcessSpec) -> Result<ProcessOutput, ProcessError> {
        for _ in 0..50 {
            match self.0.run(spec.clone()).await {
                Err(ProcessError::Spawn(message)) if message.contains("Text file busy") => {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                other => return other,
            }
        }
        self.0.run(spec).await
    }
}

pub fn spec(program: &str, args: &[&str]) -> ProcessSpec {
    ProcessSpec {
        program: program.to_owned(),
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        env: BTreeMap::new(),
        cwd: None,
        stdin: None,
        timeout: SignedDuration::from_secs(10),
        max_output_bytes: 64 * 1024,
        run_as: None,
    }
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

pub fn chmod(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// Copies fixture skill `name` into `root/<folder>` with safe modes.
pub fn install_skill(root: &Path, name: &str, folder: &str) -> PathBuf {
    let src = fixtures().join("skills").join(name);
    let dst = root.join(folder);
    std::fs::create_dir_all(&dst).unwrap();
    for entry in std::fs::read_dir(&src).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), dst.join(entry.file_name())).unwrap();
    }
    chmod(&dst, 0o755);
    chmod(&dst.join("skill.toml"), 0o644);
    chmod(&dst.join("run"), 0o755);
    dst
}

/// Copies fixture script `name` into `root` as an executable file.
pub fn install_script(root: &Path, name: &str) -> PathBuf {
    let dst = root.join(name);
    std::fs::copy(fixtures().join("scripts").join(name), &dst).unwrap();
    chmod(&dst, 0o755);
    dst
}

/// The canonical form of a tempdir path (macOS and some CI temp dirs are symlinks).
pub fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap()
}
