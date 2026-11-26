//! Integration tests for the runner: real process execution and skill folder loading
//! (Linux), and argv construction (everywhere).

#[cfg(test)]
#[cfg(target_os = "linux")]
mod catalog;
#[cfg(test)]
#[cfg(target_os = "linux")]
mod examples;
#[cfg(test)]
#[cfg(target_os = "linux")]
mod exec;
#[cfg(test)]
#[cfg(target_os = "linux")]
mod helpers;
#[cfg(test)]
#[cfg(target_os = "linux")]
mod serve_cancel;
#[cfg(test)]
#[cfg(target_os = "linux")]
mod serve_catalog;
#[cfg(test)]
#[cfg(target_os = "linux")]
mod serve_checks;
#[cfg(test)]
#[cfg(target_os = "linux")]
mod serve_common;
#[cfg(test)]
#[cfg(target_os = "linux")]
mod serve_run;
#[cfg(test)]
mod skill_run;
