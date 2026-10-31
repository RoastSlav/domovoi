//! `security.ssh_login`: successful SSH logins.
//!
//! - **Source:** the `ssh.service` and `sshd.service` journal since the last cursor,
//!   matching `Accepted <method> for <user> from <ip> port <port>`.
//! - **Target:** the user. **Class:** immediate (each login is an event).
//! - **Severity:** warning when any login in the run came from the internet, info when
//!   every one came from the local network (see [`classify_ip`]).
//! - **Evidence:** `ip`, `method`, `port` and `network` of the most telling login (the
//!   latest from the internet, else the latest), and `count`.

use std::collections::BTreeMap;
use std::net::IpAddr;

use async_trait::async_trait;
use domovoi_core::{CheckKey, EvidenceValue, Severity, SignalClass, ValidationError};
use jiff::SignedDuration;

use super::{Network, classify_ip, journal_since_cursor};
use crate::commands::{JournalScope, UnitName};
use crate::framework::{
    Check, CheckContext, CheckError, CheckReport, Observation, int, key, target,
};

/// One accepted SSH login.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshLogin {
    /// The user who logged in.
    pub user: String,
    /// `publickey`, `password`, ...
    pub method: String,
    /// The client address.
    pub ip: IpAddr,
    /// The client port.
    pub port: u16,
}

/// Parses an `Accepted <method> for <user> from <ip> port <port>` line.
#[must_use]
pub fn parse_accepted(line: &str) -> Option<SshLogin> {
    let rest = &line[line.find("Accepted ")? + "Accepted ".len()..];
    let mut words = rest.split_whitespace();
    let method = words.next()?.to_owned();
    (words.next()? == "for").then_some(())?;
    let user = words.next()?.to_owned();
    (words.next()? == "from").then_some(())?;
    let ip: IpAddr = words.next()?.parse().ok()?;
    (words.next()? == "port").then_some(())?;
    let port: u16 = words.next()?.parse().ok()?;
    Some(SshLogin {
        user,
        method,
        ip,
        port,
    })
}

/// The `security.ssh_login` check.
pub struct SshLogins {
    key: CheckKey,
    units: Vec<UnitName>,
}

impl SshLogins {
    /// The check.
    pub fn new() -> Result<Self, ValidationError> {
        let unit = |name: &str| {
            UnitName::parse(name).map_err(|e| ValidationError::new("unit name", e.to_string()))
        };
        Ok(Self {
            key: key("security.ssh_login")?,
            units: vec![unit("ssh.service")?, unit("sshd.service")?],
        })
    }
}

#[async_trait]
impl Check for SshLogins {
    fn key(&self) -> CheckKey {
        self.key.clone()
    }

    fn default_cadence(&self) -> SignedDuration {
        SignedDuration::from_mins(1)
    }

    fn class(&self) -> SignalClass {
        SignalClass::Immediate
    }

    async fn run(&self, ctx: &mut CheckContext<'_>) -> Result<CheckReport, CheckError> {
        let lines = journal_since_cursor(ctx, JournalScope::Units(&self.units)).await?;
        let mut by_user: BTreeMap<String, (u64, SshLogin, Network)> = BTreeMap::new();
        for login in lines.iter().filter_map(|l| parse_accepted(l)) {
            let network = classify_ip(login.ip);
            let entry = by_user
                .entry(login.user.clone())
                .or_insert_with(|| (0, login.clone(), network));
            entry.0 += 1;
            if network == Network::Wan || entry.2 == Network::Lan {
                entry.1 = login;
                entry.2 = network;
            }
        }
        let mut report = CheckReport::default();
        for (user, (count, login, network)) in by_user {
            let severity = match network {
                Network::Wan => Severity::Warning,
                Network::Lan => Severity::Info,
            };
            report.observations.push(
                Observation::firing(
                    target(&user),
                    severity,
                    format!(
                        "SSH login for {user} from {} ({})",
                        login.ip,
                        network.as_str()
                    ),
                )
                .with_text("ip", login.ip.to_string())
                .with_text("method", login.method)
                .with("port", EvidenceValue::Int(i64::from(login.port)))
                .with_text("network", network.as_str())
                .with("count", int(count)),
            );
        }
        Ok(report)
    }
}
