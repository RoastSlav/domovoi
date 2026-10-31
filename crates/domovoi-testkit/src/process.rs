//! A process executor that returns canned output.

use std::collections::BTreeMap;
use std::sync::Mutex;

use async_trait::async_trait;
use domovoi_core::ports::{ProcessError, ProcessExecutor, ProcessOutput, ProcessSpec};

use crate::lock;

/// Program and arguments an expectation matches.
type Invocation = (String, Vec<String>);

/// A [`ProcessExecutor`] that answers from expectations keyed by program and arguments.
///
/// Every call is recorded. A call with no matching expectation returns
/// [`ProcessError::NotFound`]. Expectations can be matched any number of times.
#[derive(Debug, Default)]
pub struct FakeProcessExecutor {
    expectations: Mutex<BTreeMap<Invocation, Result<ProcessOutput, ProcessError>>>,
    calls: Mutex<Vec<ProcessSpec>>,
}

impl FakeProcessExecutor {
    /// An executor with no expectations.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Answers `program` with exactly `args` with `output`.
    pub fn expect(&self, program: &str, args: &[&str], output: ProcessOutput) {
        self.expect_result(program, args, Ok(output));
    }

    /// Answers `program` with exactly `args` with `result`, which may be an error.
    pub fn expect_result(
        &self,
        program: &str,
        args: &[&str],
        result: Result<ProcessOutput, ProcessError>,
    ) {
        let key = (
            program.to_owned(),
            args.iter().map(|a| (*a).to_owned()).collect(),
        );
        lock(&self.expectations).insert(key, result);
    }

    /// Every spec received so far, oldest first, including unexpected ones.
    #[must_use]
    pub fn calls(&self) -> Vec<ProcessSpec> {
        lock(&self.calls).clone()
    }
}

#[async_trait]
impl ProcessExecutor for FakeProcessExecutor {
    async fn run(&self, spec: ProcessSpec) -> Result<ProcessOutput, ProcessError> {
        let key = (spec.program.clone(), spec.args.clone());
        lock(&self.calls).push(spec);
        lock(&self.expectations)
            .get(&key)
            .cloned()
            .unwrap_or(Err(ProcessError::NotFound))
    }
}

#[cfg(test)]
mod tests {
    use jiff::SignedDuration;

    use super::*;

    fn spec(program: &str, args: &[&str]) -> ProcessSpec {
        ProcessSpec {
            program: program.to_owned(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            env: BTreeMap::new(),
            cwd: None,
            stdin: None,
            timeout: SignedDuration::from_secs(5),
            max_output_bytes: 1024,
            run_as: None,
        }
    }

    #[tokio::test]
    async fn fake_executor_matches_program_and_args_and_records_everything() {
        let exec = FakeProcessExecutor::new();
        let out = ProcessOutput {
            exit_code: Some(0),
            stdout: "ok".to_owned(),
            ..ProcessOutput::default()
        };
        exec.expect("/usr/bin/zpool", &["status", "-x"], out.clone());

        assert_eq!(
            exec.run(spec("/usr/bin/zpool", &["status", "-x"])).await,
            Ok(out)
        );
        assert_eq!(
            exec.run(spec("/usr/bin/zpool", &["status"])).await,
            Err(ProcessError::NotFound)
        );
        assert_eq!(exec.calls().len(), 2);
    }
}
