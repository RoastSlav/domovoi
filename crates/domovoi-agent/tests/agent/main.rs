//! Agent runtime tests against a test hub on loopback, with a fake clock.

#[cfg(test)]
mod attest;
#[cfg(test)]
#[cfg(target_os = "linux")]
mod capability;
#[cfg(test)]
#[path = "../common/skills.rs"]
mod common;
#[cfg(test)]
mod docker;
#[cfg(test)]
mod dry_run;
#[cfg(test)]
mod harness;
#[cfg(test)]
mod helper_client;
#[cfg(test)]
mod hub;
#[cfg(test)]
mod link;
#[cfg(test)]
#[cfg(target_os = "linux")]
#[path = "../common/packages.rs"]
mod package_rig;
#[cfg(test)]
mod packages;
#[cfg(test)]
mod relay;
#[cfg(test)]
mod requests;
#[cfg(test)]
#[cfg(unix)]
#[path = "../common/runner.rs"]
mod runner_rig;
#[cfg(test)]
mod runtime;
#[cfg(test)]
mod scheduler;
#[cfg(test)]
#[cfg(unix)]
mod skill_runner;
#[cfg(test)]
#[cfg(unix)]
mod skill_runner_requests;
#[cfg(test)]
#[cfg(unix)]
mod skill_runner_status;
#[cfg(test)]
mod spool;
