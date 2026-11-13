//! How a host's agent stands against this hub's protocol, derived from its stored report.

use domovoi_core::{AgentReport, ReportedProtocol};
use domovoi_proto::ProtocolVersion;
use jiff::Timestamp;

/// How a host's agent stands against this hub's protocol, derived from its stored report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolState {
    /// The agent never authenticated, or has no enrolled agent.
    Unknown,
    /// Its last reported protocol equals the hub's.
    Compatible,
    /// This hub rejected the agent's protocol. Final until the agent is upgraded.
    Incompatible {
        /// The protocol the agent reported.
        reported: ProtocolVersion,
        /// When this hub first rejected it.
        since: Timestamp,
    },
    /// It was last compatible with an older hub and has not reconnected since this hub's
    /// protocol changed.
    ExpectedIncompatible {
        /// The protocol the agent reported.
        reported: ProtocolVersion,
    },
}

fn protocol(version: ReportedProtocol) -> ProtocolVersion {
    ProtocolVersion {
        major: version.major,
        minor: version.minor,
    }
}

/// Derives the state from the stored report; `hub` is `PROTOCOL_VERSION` in production.
#[must_use]
pub fn protocol_state(report: Option<&AgentReport>, hub: ProtocolVersion) -> ProtocolState {
    let Some(report) = report else {
        return ProtocolState::Unknown;
    };
    let reported = protocol(report.protocol);
    if reported == hub {
        return ProtocolState::Compatible;
    }
    match report.incompatible_since {
        Some(since) => ProtocolState::Incompatible { reported, since },
        None => ProtocolState::ExpectedIncompatible { reported },
    }
}

#[cfg(test)]
mod tests {
    use domovoi_core::{AgentReport, ReportedProtocol};
    use domovoi_proto::{PROTOCOL_VERSION, ProtocolVersion};

    use super::{ProtocolState, protocol_state};

    fn at(secs: i64) -> jiff::Timestamp {
        let start = "2026-10-31T12:00:00Z".parse::<jiff::Timestamp>().unwrap();
        start + jiff::SignedDuration::from_secs(secs)
    }

    fn report(minor: u16, since: Option<jiff::Timestamp>) -> AgentReport {
        AgentReport {
            protocol: ReportedProtocol { major: 1, minor },
            release: "0.1.0".to_owned(),
            last_authenticated_at: at(0),
            incompatible_since: since,
        }
    }

    #[test]
    fn expected_incompatible_after_hub_protocol_change() {
        let newer = ProtocolVersion {
            major: PROTOCOL_VERSION.major,
            minor: PROTOCOL_VERSION.minor + 1,
        };
        assert_eq!(
            protocol_state(Some(&report(1, None)), newer),
            ProtocolState::ExpectedIncompatible {
                reported: ProtocolVersion { major: 1, minor: 1 }
            }
        );
        assert_eq!(
            protocol_state(Some(&report(1, Some(at(5)))), newer),
            ProtocolState::Incompatible {
                reported: ProtocolVersion { major: 1, minor: 1 },
                since: at(5),
            }
        );
        assert_eq!(
            protocol_state(Some(&report(1, Some(at(5)))), PROTOCOL_VERSION),
            ProtocolState::Compatible,
            "a matching report is compatible even when a stale rejection is stored"
        );
        assert_eq!(protocol_state(None, newer), ProtocolState::Unknown);
    }
}
