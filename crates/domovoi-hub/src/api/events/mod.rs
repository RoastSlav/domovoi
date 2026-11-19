//! `GET /api/v1/events`: the journal as a server-sent event stream the web UI replays.

mod params;
mod stream;
mod wire;

use std::collections::VecDeque;
use std::convert::Infallible;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::sse::{Event, Sse};
use domovoi_core::ports::TopicSet;
use futures::{Stream, StreamExt};

use self::params::{EventsQuery, resume_from};
use self::stream::Machine;
use super::error::{ApiError, ErrorBody};
use super::{ApiState, Authed};

/// How long a stream stays silent before it sends a keepalive comment.
pub const KEEP_ALIVE: Duration = Duration::from_secs(15);
/// The most journal rows read and written before the cursor is checked again.
pub(crate) const BATCH_ROWS: u16 = 100;
/// How far behind the head a client may be before it is told to refetch instead.
pub(crate) const LAG_LIMIT_ROWS: u64 = 5_000;
/// The most event streams the hub serves at once.
pub const MAX_EVENT_STREAMS: usize = 32;

/// A slot among the open event streams, released when the stream is dropped.
pub(crate) struct StreamGuard(Arc<AtomicUsize>);

impl StreamGuard {
    /// Takes a slot, or `None` when [`MAX_EVENT_STREAMS`] are open.
    fn acquire(open: &Arc<AtomicUsize>) -> Option<Self> {
        open.fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
            (n < MAX_EVENT_STREAMS).then_some(n + 1)
        })
        .ok()?;
        metrics::gauge!("domovoi_sse_streams_open").increment(1.0);
        Some(Self(open.clone()))
    }
}

impl Drop for StreamGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
        metrics::gauge!("domovoi_sse_streams_open").decrement(1.0);
    }
}

/// Committed journal rows as server-sent events.
///
/// Frames: `ready` (`head_seq`, `resumed`, `topics`), `journal` (one row; its `id` is the journal
/// `seq`) and `reset` (`reason` is `pruned`, `ahead` or `lagged`: refetch what you hold). A client
/// that reconnects sends `Last-Event-ID` (or `after`) and receives every matching row after it.
/// An idle stream sends a `: keepalive` comment every 15 seconds.
#[utoipa::path(
    get, path = "/api/v1/events", operation_id = "stream_events", tag = "system",
    params(EventsQuery),
    responses(
        (status = 200, description = "An event stream", content_type = "text/event-stream"),
        (status = 400, description = "An unknown topic or a malformed position", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 503, description = "Too many open event streams", body = ErrorBody),
    )
)]
pub(super) async fn stream(
    State(state): State<ApiState>,
    _auth: Authed,
    headers: HeaderMap,
    Query(query): Query<EventsQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let topics = TopicSet::parse_list(query.topics.as_deref())
        .map_err(|err| ApiError::BadRequest(err.to_string()))?;
    let resume = resume_from(&headers, &query)?;
    let guard = StreamGuard::acquire(&state.event_streams)
        .ok_or_else(|| ApiError::Unavailable("too many open event streams".to_owned()))?;
    let deps = &state.deps;
    let machine = Machine {
        feed: deps.journal.clone(),
        clock: deps.clock.clone(),
        redactor: deps.redactor.clone(),
        topics,
        resume,
        started: false,
        cursor: 0,
        pending: VecDeque::new(),
        last_send: deps.clock.now(),
        _guard: guard,
    };
    let frames = futures::stream::unfold(machine, |mut machine| async move {
        let event = machine.next().await?;
        Some((Ok(event), machine))
    });
    Ok(Sse::new(
        frames.take_until(state.shutdown.cancelled_owned()),
    ))
}
