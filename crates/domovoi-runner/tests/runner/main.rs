//! Integration tests for the runner: real process execution and skill folder loading
//! (Linux), and argv construction (everywhere).

#[cfg(test)]
#[cfg(target_os = "linux")]
mod catalog;
#[cfg(test)]
#[cfg(target_os = "linux")]
mod exec;
#[cfg(test)]
#[cfg(target_os = "linux")]
mod helpers;
#[cfg(test)]
mod skill_run;
