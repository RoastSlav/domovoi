//! One implementation per repository port, all on [`crate::Store`].

mod accounts;
mod actions;
mod audit;
mod hosts;
mod incidents;
mod knowledge;
mod notes;
mod notify;
mod triage;

pub use notes::SNIPPET_CHARS;

pub(crate) use actions::{insert_action, update_action_from};
pub(crate) use audit::insert_audit;
pub(crate) use incidents::{
    DiscardOutcome, discard_pending_incident, insert_incident, insert_incident_event,
    insert_signal_and_link, update_incident,
};
pub(crate) use notes::{insert_note, update_note};
