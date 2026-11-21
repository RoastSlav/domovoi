//! What a person sees of a job: its targets and the status derived from them.

use std::collections::BTreeMap;

use domovoi_core::package::PrivilegeBoundary;
use domovoi_core::{HostId, HostName, InstallJob, InstallTarget, TargetStatus};

/// How a job is going, derived when it is read and never stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    /// At least one host's latest attempt has not finished.
    Running,
    /// Every host that was not cancelled succeeded.
    Succeeded,
    /// No host succeeded.
    Failed,
    /// Some hosts succeeded and some did not.
    Partial,
}

impl JobStatus {
    /// The wire text.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Partial => "partial",
        }
    }
}

/// The latest attempt of each host, in the order the hosts first appear.
fn latest_attempts(targets: &[InstallTarget]) -> Vec<&InstallTarget> {
    let mut latest: Vec<&InstallTarget> = Vec::new();
    for target in targets {
        match latest.iter_mut().find(|seen| seen.host == target.host) {
            Some(seen) if seen.attempt < target.attempt => *seen = target,
            Some(_) => {}
            None => latest.push(target),
        }
    }
    latest
}

/// The status of a job from its targets. Only each host's latest attempt counts, so a host
/// that failed and then succeeded on retry reads as a success. A job whose every host was
/// cancelled succeeded nowhere, so it is `Failed`.
#[must_use]
pub fn job_status(targets: &[InstallTarget]) -> JobStatus {
    let latest = latest_attempts(targets);
    if latest.iter().any(|target| !target.status.is_terminal()) {
        return JobStatus::Running;
    }
    let counted: Vec<_> = latest
        .iter()
        .filter(|target| target.status != TargetStatus::Cancelled)
        .collect();
    let succeeded = counted
        .iter()
        .filter(|target| target.status == TargetStatus::Succeeded)
        .count();
    if succeeded == 0 {
        JobStatus::Failed
    } else if succeeded == counted.len() {
        JobStatus::Succeeded
    } else {
        JobStatus::Partial
    }
}

/// A job with everything a transport needs to render it.
#[derive(Debug, Clone)]
pub struct JobView {
    /// The job.
    pub job: InstallJob,
    /// Every attempt on every host, oldest first.
    pub targets: Vec<InstallTarget>,
    /// The names of the hosts the targets are on.
    pub host_names: BTreeMap<HostId, HostName>,
    /// What each host reported about the package's privilege boundary, when the hub has seen
    /// it installed there.
    pub boundaries: BTreeMap<HostId, PrivilegeBoundary>,
}

impl JobView {
    /// The derived status.
    #[must_use]
    pub fn status(&self) -> JobStatus {
        job_status(&self.targets)
    }
}

#[cfg(test)]
mod tests {
    use domovoi_core::{ExecutionId, InstallJobId, InstallTargetId, StoredAuthorization, UserId};
    use domovoi_testkit::approvals::{human, provenance};
    use domovoi_testkit::build::at;

    use super::*;

    fn target(host: HostId, attempt: u32, status: TargetStatus) -> InstallTarget {
        InstallTarget {
            id: InstallTargetId::new(),
            job: InstallJobId::new(),
            host,
            attempt,
            execution: ExecutionId::new(),
            status,
            plan_hash: "ab".repeat(32).parse().unwrap(),
            authorization: StoredAuthorization::new(human(UserId::new()), provenance()),
            expires_at: at(900),
            artifact_transfer: None,
            signature_transfer: None,
            dispatched_session: None,
            dispatched_epoch: None,
            failure: None,
            outputs: None,
            outputs_confirmed: false,
            queued_at: at(1),
            started_at: None,
            finished_at: None,
            version: 1,
        }
    }

    #[test]
    fn job_status_is_derived_from_the_latest_attempt_of_each_host() {
        let (a, b) = (HostId::new(), HostId::new());
        use TargetStatus::{Cancelled, Dispatched, Failed, NotRun, Queued, Succeeded};
        let status = |targets: &[InstallTarget]| job_status(targets).as_str();
        assert_eq!(status(&[target(a, 1, Queued)]), "running");
        assert_eq!(
            status(&[target(a, 1, Succeeded), target(b, 1, Dispatched)]),
            "running"
        );
        assert_eq!(
            status(&[target(a, 1, Succeeded), target(b, 1, Succeeded)]),
            "succeeded"
        );
        assert_eq!(
            status(&[target(a, 1, Succeeded), target(b, 1, Cancelled)]),
            "succeeded"
        );
        assert_eq!(
            status(&[target(a, 1, Failed), target(b, 1, NotRun)]),
            "failed"
        );
        assert_eq!(
            status(&[target(a, 1, Succeeded), target(b, 1, Failed)]),
            "partial"
        );
        // A retry that succeeded replaces the failed attempt before it.
        assert_eq!(
            status(&[
                target(a, 1, Succeeded),
                target(b, 1, Failed),
                target(b, 2, Succeeded)
            ]),
            "succeeded"
        );
        // A retry that is still running makes the job running.
        assert_eq!(
            status(&[target(a, 1, Failed), target(a, 2, Queued)]),
            "running"
        );
        // Nothing succeeded anywhere, including a job cancelled in full.
        assert_eq!(
            status(&[target(a, 1, Cancelled), target(b, 1, Cancelled)]),
            "failed"
        );
    }
}
