//! Chat gateway and notifier adapters.
//!
//! Translates `Card`s into platform messages and platform events into
//! `Interaction`s. Authorization of interactions happens in the hub, not here.
//! See `ARCHITECTURE.md` §13 and ADR 0010.

pub mod discord;
