//! Bounds the dispatcher applies on top of the per-operation limits of `domovoi-core`: the
//! request deadline, and the cut that keeps an answer inside one frame.

use domovoi_config::HelperConfig;
use domovoi_core::helper::{HelperResponse, MAX_RESPONSE_BYTES};
use domovoi_core::ports::Clock;
use jiff::SignedDuration;

/// Room left in a frame for the envelope (`v`, `id`, the variant tag) around a response body.
const ENVELOPE_BYTES: usize = 512;

/// How long one whole read request may run, from `[limits] timeout`.
#[must_use]
pub fn request_deadline(config: &HelperConfig) -> SignedDuration {
    config.limits.timeout.as_signed()
}

/// Runs `fut` and gives up when `limit` has passed on `clock`; the timer is the injected clock's,
/// never the runtime's.
pub async fn within<F: Future>(
    clock: &dyn Clock,
    limit: SignedDuration,
    fut: F,
) -> Option<F::Output> {
    let now = clock.now();
    let deadline = now.saturating_add(limit).unwrap_or(now);
    tokio::select! {
        out = fut => Some(out),
        () = clock.sleep_until(deadline) => None,
    }
}

/// The longest prefix of `text` that is at most `max` bytes and ends on a character boundary.
#[must_use]
pub fn cut_utf8(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn encoded_len(response: &HelperResponse) -> usize {
    serde_json::to_vec(response).map_or(usize::MAX, |bytes| bytes.len())
}

/// Shrinks `response` until its encoded frame fits [`MAX_RESPONSE_BYTES`], setting its `truncated`
/// flag. Text is cut on a character boundary; lists lose their tail. A response with no payload
/// that can be cut (an inspect document) is left alone: its own cap is far below the limit.
#[must_use]
pub fn fit_frame(mut response: HelperResponse) -> HelperResponse {
    let budget = MAX_RESPONSE_BYTES - ENVELOPE_BYTES;
    let mut guard = 0;
    while encoded_len(&response) > budget && guard < 64 {
        guard += 1;
        if !shrink(&mut response) {
            break;
        }
    }
    response
}

/// Cuts a quarter off the largest payload of `response`. Returns whether anything was cut.
fn shrink(response: &mut HelperResponse) -> bool {
    match response {
        HelperResponse::DockerLogs { text, truncated }
        | HelperResponse::KernelLog { text, truncated } => cut_text(text, truncated),
        HelperResponse::SmartReport {
            json, truncated, ..
        } => cut_text(json, truncated),
        HelperResponse::DockerContainers {
            containers,
            truncated,
        } => cut_list(containers, truncated),
        HelperResponse::DockerEvents { events, truncated } => cut_list(events, truncated),
        HelperResponse::NetListeners {
            listeners,
            truncated,
        } => cut_list(listeners, truncated),
        _ => false,
    }
}

fn cut_text(text: &mut String, truncated: &mut bool) -> bool {
    if text.is_empty() {
        return false;
    }
    let keep = cut_utf8(text, text.len() * 3 / 4).len();
    text.truncate(keep);
    *truncated = true;
    true
}

fn cut_list<T>(items: &mut Vec<T>, truncated: &mut bool) -> bool {
    if items.is_empty() {
        return false;
    }
    items.truncate(items.len() * 3 / 4);
    *truncated = true;
    true
}
