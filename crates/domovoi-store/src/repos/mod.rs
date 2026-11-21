//! One implementation per repository port, all on [`crate::Store`].

mod accounts;
mod actions;
mod admission;
mod audit;
mod conversations;
mod conversations_rows;
mod hosts;
mod incidents;
mod install;
mod knowledge;
mod notes;
mod notify;
mod operations;
mod operations_rows;
mod policy;
mod triage;

pub use notes::SNIPPET_CHARS;

pub(crate) use actions::{insert_action, update_action_from};
pub(crate) use incidents::{
    DiscardOutcome, discard_pending_incident, insert_incident, insert_incident_event,
    insert_signal_and_link, update_incident,
};
pub(crate) use notes::{insert_note, update_note};
