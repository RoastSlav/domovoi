//! Integration tests for adoption, serving, start checks and the state directory.
#![cfg(unix)]

#[cfg(test)]
mod adopt;
#[cfg(test)]
mod common;
#[cfg(test)]
mod gates;
#[cfg(test)]
mod serve;
#[cfg(test)]
mod start_check;
#[cfg(test)]
mod state;
