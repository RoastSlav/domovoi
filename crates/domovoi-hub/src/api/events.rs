//! `GET /api/v1/events`: server-sent events the web UI uses to refresh, derived from the
//! incident manager's domain events.

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::Stream;
use tokio::sync::broadcast;

use super::dto_admin::UiEvent;
use super::error::ErrorBody;
use super::{ApiState, Authed};
use domovoi_engine::{DomainEvent, EventKind};

/// How often an idle stream sends a keep-alive comment.
pub const KEEP_ALIVE: Duration = Duration::from_secs(15);

/// The UI event for `event`, or `None` for events the UI does not show.
#[must_use]
pub fn ui_event(event: &DomainEvent) -> Option<UiEvent> {
    let incident_id = event.incident.id().to_string();
    let incident = |name: &str| UiEvent {
        kind: "incident".to_owned(),
        event: name.to_owned(),
        incident_id: incident_id.clone(),
        action_id: None,
        status: Some(event.incident.status().name().to_owned()),
    };
    let ui = match &event.kind {
        EventKind::IncidentOpened => incident("incident_opened"),
        EventKind::IncidentFolded { .. } => incident("incident_folded"),
        EventKind::IncidentEscalated { .. } => incident("incident_escalated"),
        EventKind::IncidentReopened => incident("incident_reopened"),
        EventKind::IncidentResolved => incident("incident_resolved"),
        EventKind::IncidentAcked { .. } => incident("incident_acked"),
        EventKind::IncidentMuted { .. } => incident("incident_muted"),
        EventKind::IncidentUnmuted => incident("incident_unmuted"),
        EventKind::TriageCompleted { .. } => incident("triage_completed"),
        EventKind::ActionChanged { action, status } => UiEvent {
            kind: "action".to_owned(),
            event: "action_changed".to_owned(),
            incident_id,
            action_id: Some(action.to_string()),
            status: Some(status.name().to_owned()),
        },
        EventKind::ActionBlocked { action, .. } => UiEvent {
            kind: "action".to_owned(),
            event: "action_blocked".to_owned(),
            incident_id,
            action_id: Some(action.to_string()),
            status: None,
        },
        EventKind::TriageRequested
        | EventKind::TriageDegraded { .. }
        | EventKind::ActionApproved { .. } => return None,
    };
    Some(ui)
}

fn events(rx: broadcast::Receiver<DomainEvent>) -> impl Stream<Item = Result<Event, Infallible>> {
    futures::stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let Some(ui) = ui_event(&event) else { continue };
                    let Ok(sse) = Event::default().event(ui.kind.clone()).json_data(&ui) else {
                        continue;
                    };
                    return Some((Ok(sse), rx));
                }
                // A slow client misses some updates; the UI refetches on the next one.
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    })
}

/// A stream of incident and action changes, as `incident` and `action` events whose data is
/// a JSON `UiEvent`.
#[utoipa::path(
    get, path = "/api/v1/events", operation_id = "stream_events", tag = "system",
    responses(
        (status = 200, description = "An event stream", body = UiEvent, content_type = "text/event-stream"),
        (status = 401, description = "Not logged in", body = ErrorBody),
    )
)]
pub(super) async fn stream(
    State(state): State<ApiState>,
    _auth: Authed,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    Sse::new(events(state.deps.bus.subscribe())).keep_alive(KeepAlive::new().interval(KEEP_ALIVE))
}
