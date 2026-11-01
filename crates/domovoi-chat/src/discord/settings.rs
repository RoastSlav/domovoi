//! What the Discord gateway needs besides its token.

use url::Url;

/// The channel incidents go to, and the web UI cards link to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordSettings {
    /// The channel cards are posted in; threads open under it.
    pub channel_id: u64,
    /// The hub's public web address. Card titles link to `/incidents/<id>` under it, and
    /// destructive actions link to it for approval.
    pub web_base: Url,
}

impl DiscordSettings {
    /// The web page of `incident`.
    #[must_use]
    pub fn incident_url(&self, incident: domovoi_core::IncidentId) -> String {
        format!(
            "{}/incidents/{incident}",
            self.web_base.as_str().trim_end_matches('/')
        )
    }

    /// The incident a card's title link points at, if it is one of ours.
    #[must_use]
    pub fn incident_of_url(&self, url: &str) -> Option<domovoi_core::IncidentId> {
        let base = format!(
            "{}/incidents/",
            self.web_base.as_str().trim_end_matches('/')
        );
        url.strip_prefix(&base)?.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incident_links_round_trip() {
        let settings = DiscordSettings {
            channel_id: 1,
            web_base: Url::parse("https://domovoi.example.net/").unwrap(),
        };
        let incident = domovoi_core::IncidentId::new();
        let url = settings.incident_url(incident);
        assert_eq!(
            url,
            format!("https://domovoi.example.net/incidents/{incident}")
        );
        assert_eq!(settings.incident_of_url(&url), Some(incident));
        assert_eq!(
            settings.incident_of_url("https://elsewhere.example/incidents/x"),
            None
        );
    }
}
