//! Incidents and the pure state machine that moves them between statuses.

mod state;
mod transition;

pub use state::{Incident, IncidentPolicy, IncidentStatus};
pub use transition::{IncidentChange, IncidentEvent, apply};
