//! Incidents and the pure state machine that moves them between statuses.

mod group;
mod state;
mod transition;

pub use group::IncidentGroup;
pub use state::{Incident, IncidentPolicy, IncidentStatus};
pub use transition::{IncidentChange, IncidentEvent, apply};
