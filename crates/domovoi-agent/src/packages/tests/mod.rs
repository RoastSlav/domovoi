//! Tests for the package service. The filesystem-backed ones run on Linux only.

mod archive;
mod support;
mod transfer;
mod verify;

#[cfg(target_os = "linux")]
mod crash;
#[cfg(target_os = "linux")]
mod env;
#[cfg(target_os = "linux")]
mod install;
