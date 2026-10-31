//! Skill manifests and sandboxed process execution.
//!
//! The only crate allowed to spawn processes (ARCHITECTURE.md R3, ADR 0004).
//! Commands are argv vectors built from validated parameters, run as a
//! dedicated user with a cleared environment, a timeout that kills the whole
//! process group, capped output and redaction. Never a shell.
