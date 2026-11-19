//! What a client sends to open the stream: the topics and where to resume.

use axum::http::HeaderMap;
use serde::Deserialize;
use utoipa::IntoParams;

use crate::api::error::ApiError;

/// Query parameters of `GET /api/v1/events`.
#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct EventsQuery {
    /// Comma-separated topics: `incidents`, `approvals`, `operations`, `operation:<id>`,
    /// `hosts`, `conversations:<id>`. At most 16. Absent means `incidents,approvals,operations`.
    pub topics: Option<String>,
    /// Resume after this journal `seq`. The `Last-Event-ID` header wins when both are given.
    pub after: Option<String>,
}

/// Where the client wants to resume, if anywhere.
pub(crate) fn resume_from(
    headers: &HeaderMap,
    query: &EventsQuery,
) -> Result<Option<u64>, ApiError> {
    let header = headers
        .get("last-event-id")
        .map(|value| value.to_str().unwrap_or_default().to_owned());
    let (name, text) = match (header, &query.after) {
        (Some(text), _) => ("Last-Event-ID", text.clone()),
        (None, Some(text)) => ("after", text.clone()),
        (None, None) => return Ok(None),
    };
    text.trim()
        .parse::<u64>()
        .map(Some)
        .map_err(|_| ApiError::BadRequest(format!("{name} `{text}` is not a journal position")))
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    #[test]
    fn the_header_wins_over_the_query() {
        let mut headers = HeaderMap::new();
        headers.insert("last-event-id", HeaderValue::from_static("7"));
        let query = EventsQuery {
            topics: None,
            after: Some("3".to_owned()),
        };
        assert_eq!(resume_from(&headers, &query).unwrap(), Some(7));
        assert_eq!(resume_from(&HeaderMap::new(), &query).unwrap(), Some(3));
        assert_eq!(
            resume_from(&HeaderMap::new(), &EventsQuery::default()).unwrap(),
            None
        );
    }

    #[test]
    fn a_non_numeric_position_is_refused() {
        let query = EventsQuery {
            topics: None,
            after: Some("abc".to_owned()),
        };
        assert!(resume_from(&HeaderMap::new(), &query).is_err());
    }
}
