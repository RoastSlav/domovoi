//! Mapping engine answers to the wire types.

use std::collections::BTreeMap;

use bollard::models::{
    ContainerInspectResponse, ContainerStateStatusEnum, EventMessage, HealthStatusEnum,
};
use domovoi_core::helper::read::{ContainerWire, EventWire, WireHealth, WireRunState};
use jiff::Timestamp;

/// A Docker timestamp; Docker's zero time (`0001-01-01T00:00:00Z`) is `None`.
#[must_use]
pub fn parse_time(text: &str) -> Option<Timestamp> {
    let at: Timestamp = text.parse().ok()?;
    (at > Timestamp::UNIX_EPOCH).then_some(at)
}

fn run_state(status: Option<ContainerStateStatusEnum>) -> WireRunState {
    match status {
        Some(ContainerStateStatusEnum::RUNNING) => WireRunState::Running,
        Some(ContainerStateStatusEnum::RESTARTING) => WireRunState::Restarting,
        Some(ContainerStateStatusEnum::PAUSED) => WireRunState::Paused,
        Some(ContainerStateStatusEnum::CREATED) => WireRunState::Created,
        Some(ContainerStateStatusEnum::DEAD | ContainerStateStatusEnum::REMOVING) => {
            WireRunState::Dead
        }
        _ => WireRunState::Exited,
    }
}

fn health(status: Option<HealthStatusEnum>) -> Option<WireHealth> {
    match status? {
        HealthStatusEnum::STARTING => Some(WireHealth::Starting),
        HealthStatusEnum::HEALTHY => Some(WireHealth::Healthy),
        HealthStatusEnum::UNHEALTHY => Some(WireHealth::Unhealthy),
        _ => None,
    }
}

/// Maps one inspect response; `None` when it has no name.
#[must_use]
pub fn container_wire(
    inspect: &ContainerInspectResponse,
    image_created: Option<Timestamp>,
) -> Option<ContainerWire> {
    let name = inspect.name.as_deref()?.trim_start_matches('/').to_owned();
    if name.is_empty() {
        return None;
    }
    let state = inspect.state.as_ref();
    let config = inspect.config.as_ref();
    let labels: BTreeMap<String, String> = config
        .and_then(|c| c.labels.clone())
        .unwrap_or_default()
        .into_iter()
        .collect();
    Some(ContainerWire {
        name,
        image: config
            .and_then(|c| c.image.clone())
            .or_else(|| inspect.image.clone())
            .unwrap_or_default(),
        image_created,
        state: run_state(state.and_then(|s| s.status)),
        health: health(state.and_then(|s| s.health.as_ref()).and_then(|h| h.status)),
        restart_count: inspect
            .restart_count
            .and_then(|n| u64::try_from(n).ok())
            .unwrap_or(0),
        labels,
        auto_remove: inspect
            .host_config
            .as_ref()
            .and_then(|h| h.auto_remove)
            .unwrap_or(false),
        started_at: state
            .and_then(|s| s.started_at.as_deref())
            .and_then(parse_time),
    })
}

/// Maps one engine event; `None` when it carries no container name.
#[must_use]
pub fn event_wire(message: EventMessage, until: Timestamp) -> Option<EventWire> {
    let attributes = message.actor.and_then(|a| a.attributes).unwrap_or_default();
    let name = attributes.get("name").cloned()?;
    let at = message
        .time_nano
        .and_then(|n| Timestamp::from_nanosecond(i128::from(n)).ok())
        .or_else(|| message.time.and_then(|s| Timestamp::from_second(s).ok()))
        .unwrap_or(until);
    Some(EventWire {
        at,
        name,
        action: message.action.unwrap_or_default(),
        image: attributes.get("image").cloned(),
    })
}

/// Cuts `text` to at most `max` bytes on a character boundary.
pub fn truncate_utf8(text: &mut String, max: usize) {
    if text.len() <= max {
        return;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
}
