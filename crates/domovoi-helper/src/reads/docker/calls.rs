//! The closed list of Engine API requests the helper can send.

use domovoi_core::helper::read::ContainerRef;
use jiff::Timestamp;

/// An HTTP method. There is exactly one, so no write can be expressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    /// A read.
    Get,
}

impl HttpMethod {
    /// The method as it appears on the request line.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
        }
    }
}

/// One of the five Engine API reads. Nothing else can be expressed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DockerCall<'a> {
    /// `GET /containers/json?all=true`.
    ListContainers,
    /// `GET /containers/{c}/json`.
    InspectContainer(&'a ContainerRef),
    /// `GET /images/{i}/json`; the id is hex or `sha256:` and hex.
    InspectImage(&'a str),
    /// `GET /events` for container events in a window.
    Events {
        /// Start of the window.
        since: Timestamp,
        /// End of the window.
        until: Timestamp,
    },
    /// `GET /containers/{c}/logs` for the last `tail` lines.
    Logs {
        /// The container.
        container: &'a ContainerRef,
        /// How many lines.
        tail: u16,
    },
}

/// Whether `id` is an image id taken from an inspect answer: hex, optionally after `sha256:`.
#[must_use]
pub fn is_image_id(id: &str) -> bool {
    let hex = id.strip_prefix("sha256:").unwrap_or(id);
    !hex.is_empty() && hex.len() <= 128 && hex.bytes().all(|b| b.is_ascii_hexdigit())
}

/// A Docker `since`/`until` value: seconds, a dot and nanoseconds.
#[must_use]
pub fn docker_time(at: Timestamp) -> String {
    format!("{}.{:09}", at.as_second(), at.subsec_nanosecond().max(0))
}

impl<'a> DockerCall<'a> {
    /// An image inspect, or `None` when `id` is not hex or `sha256:` and hex.
    #[must_use]
    pub fn inspect_image(id: &'a str) -> Option<Self> {
        is_image_id(id).then_some(Self::InspectImage(id))
    }

    /// The method: always a read.
    #[must_use]
    pub const fn method(&self) -> HttpMethod {
        HttpMethod::Get
    }

    /// The request path and query, without the API version prefix.
    #[must_use]
    pub fn path(&self) -> String {
        match self {
            Self::ListContainers => "/containers/json?all=true".to_owned(),
            Self::InspectContainer(c) => format!("/containers/{}/json", c.as_str()),
            Self::InspectImage(id) => format!("/images/{id}/json"),
            Self::Events { since, until } => format!(
                "/events?since={}&until={}&filters=%7B%22type%22%3A%5B%22container%22%5D%7D",
                docker_time(*since),
                docker_time(*until)
            ),
            Self::Logs { container, tail } => format!(
                "/containers/{}/logs?stdout=1&stderr=1&tail={tail}",
                container.as_str()
            ),
        }
    }
}
