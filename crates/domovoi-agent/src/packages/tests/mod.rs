//! Tests for the package service. The filesystem-backed ones run on Linux only.

mod archive;
mod support;
mod transfer;
mod verify;

#[cfg(test)]
pub(crate) use support::SteppingClock;

#[cfg(target_os = "linux")]
mod crash;
#[cfg(target_os = "linux")]
mod enable;
#[cfg(target_os = "linux")]
mod env;
#[cfg(target_os = "linux")]
mod install;
#[cfg(target_os = "linux")]
mod staging;
