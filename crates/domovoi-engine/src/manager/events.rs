//! Asking for an investigation. Pure.

use domovoi_core::Incident;

use super::decide::Builder;
use crate::events::EventKind;

impl Builder<'_> {
    /// Asks for an investigation, unless the incident is a blind check: it already says why
    /// and how to fix it, and a model can read nothing the check could not.
    pub(super) fn request_triage(&mut self, incident: &Incident, notify: bool) {
        if !incident.is_monitoring_blind() {
            self.event(EventKind::TriageRequested, incident, notify);
        }
    }
}
