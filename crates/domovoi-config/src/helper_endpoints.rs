//! The one value of `agent.toml` the helper's safety attestation reads: the hub's address.
//!
//! The helper must not load the whole agent configuration: that applies `DOMOVOI_*` overrides
//! and runs the agent's own validation, and an unrelated mistake in `agent.toml` would then
//! hide the hub address. This reader looks at `[agent] hub_url` and nothing else.

use core::net::IpAddr;

use url::{Host, Url};

use crate::helper_safety::HelperSafetyError;

/// The literal IP address in the host part of `[agent] hub_url`, or `None` when the URL names a
/// host. A name is never resolved: resolving would churn the attested state with DNS, and a name
/// the agent controls must not decide an exclusion.
///
/// # Errors
///
/// [`HelperSafetyError::Unreadable`] when the text is not TOML, `[agent] hub_url` is missing or
/// not a string, or the string is not a URL with a host.
pub fn hub_url_ip(agent_toml: &str) -> Result<Option<IpAddr>, HelperSafetyError> {
    let bad = |why: &str| HelperSafetyError::Unreadable(why.to_owned());
    let table: toml::Table = agent_toml
        .parse()
        .map_err(|err: toml::de::Error| bad(err.message()))?;
    let text = table
        .get("agent")
        .and_then(toml::Value::as_table)
        .and_then(|agent| agent.get("hub_url"))
        .and_then(toml::Value::as_str)
        .ok_or_else(|| bad("agent.toml has no [agent] hub_url"))?;
    let url = Url::parse(text).map_err(|_| bad("[agent] hub_url is not a URL"))?;
    match url.host() {
        Some(Host::Ipv4(ip)) => Ok(Some(IpAddr::V4(ip))),
        Some(Host::Ipv6(ip)) => Ok(Some(IpAddr::V6(ip))),
        Some(Host::Domain(_)) => Ok(None),
        None => Err(bad("[agent] hub_url has no host")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toml(url: &str) -> String {
        format!("[agent]\nhost_name = \"a\"\nhub_url = \"{url}\"\n")
    }

    #[test]
    fn a_literal_address_is_returned_and_a_name_is_not() {
        let v4 = hub_url_ip(&toml("wss://192.0.2.7:7421")).expect("reads");
        assert_eq!(v4, Some("192.0.2.7".parse().expect("ip")));
        let v6 = hub_url_ip(&toml("wss://[2001:db8::7]:7421")).expect("reads");
        assert_eq!(v6, Some("2001:db8::7".parse().expect("ip")));
        assert_eq!(hub_url_ip(&toml("wss://hub.example.net:7421")), Ok(None));
    }

    #[test]
    fn a_missing_or_malformed_hub_url_is_unreadable() {
        for text in [
            "",
            "[agent]\n",
            "[agent]\nhub_url = 7\n",
            "not toml [",
            "[agent]\nhub_url = \"nonsense\"\n",
        ] {
            assert!(
                matches!(hub_url_ip(text), Err(HelperSafetyError::Unreadable(_))),
                "{text:?}"
            );
        }
    }
}
