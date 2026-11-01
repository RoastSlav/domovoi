//! The Domovoi hub runtime.
//!
//! Serves the HTTP API, SSE and the embedded web UI, authenticates users and
//! chat interactions, ingests webhooks, runs the agent server and wires the
//! engine to the store, LLM and chat adapters. Never spawns processes. See
//! `ARCHITECTURE.md` §7.1 and §10.

pub mod admin;
pub mod agent_server;
pub mod api;
pub mod auth;
mod digest;
pub mod ingest;
pub mod middleware;
pub mod router;
pub mod runtime;
pub mod selftest;
pub mod spa;

pub use domovoi_engine::PatternRedactor;
