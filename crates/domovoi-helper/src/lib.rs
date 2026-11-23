//! The root helper runtime of Domovoi: connection adoption, the closed request dispatcher, the
//! start checks, the host inspection adapter, the isolation self-test and the repair command.
//!
//! The helper spawns no process. Pure types, the platform tier and the check evaluators live in
//! `domovoi-core`; this crate holds everything that touches the host.
