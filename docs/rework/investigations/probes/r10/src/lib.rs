//! Standalone R-10 scratch harness: a reimplementation of the proposed helper safety
//! attestation contracts (RW-37, 04b) used only to try to break them. Not production code.

pub mod config;
pub mod coverage;
pub mod decision;
pub mod encoding;
pub mod fixture;
pub mod hash;
pub mod helper;
pub mod hub;
pub mod keys;
pub mod lines;
pub mod report;
pub mod routes;
pub mod safety;
pub mod state;
pub mod time;
