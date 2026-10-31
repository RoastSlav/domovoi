//! Concise builders for domain values with sensible defaults.
//!
//! Builders that parse names return `Result` so tests surface a typo as an error rather than
//! a panic inside testkit; tests usually just `unwrap` them.

use std::collections::BTreeMap;

use domovoi_core::{
    ActionRequest, ArgError, CheckKey, Host, HostId, HostKind, HostName, Incident, IncidentEvent,
    IncidentId, IncidentPolicy, Principal, Risk, Severity, Signal, SignalClass, SkillManifest,
    SkillName, SourceKind, TargetKey, TransitionError, ValidationError, apply,
};
use jiff::{SignedDuration, Timestamp};

/// The instant every builder uses unless told otherwise: 2023-11-14T22:13:20Z.
pub const T0: Timestamp = Timestamp::constant(1_700_000_000, 0);

/// Why a builder failed.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum BuildError {
    /// A name or key did not parse.
    #[error(transparent)]
    Validation(#[from] ValidationError),
    /// Arguments did not match the manifest.
    #[error(transparent)]
    Args(#[from] ArgError),
    /// An incident event could not be applied.
    #[error(transparent)]
    Transition(#[from] TransitionError),
}

/// [`T0`] plus `secs` seconds (negative goes back). Returns [`T0`] if the result would be
/// out of range, which no test needs.
#[must_use]
pub fn at(secs: i64) -> Timestamp {
    T0.checked_add(SignedDuration::from_secs(secs))
        .unwrap_or(T0)
}

/// A remote host named `name`, created at [`T0`], without labels.
pub fn host(name: &str) -> Result<Host, BuildError> {
    Ok(Host {
        id: HostId::new(),
        name: HostName::parse(name)?,
        kind: HostKind::Remote,
        labels: BTreeMap::new(),
        created_at: T0,
    })
}

/// A firing warning from an agent check, observed at [`T0`].
pub fn signal(host: HostId, check: &str, target: Option<&str>) -> Result<Signal, BuildError> {
    let check = CheckKey::parse(check)?;
    let target = target.map(TargetKey::parse).transpose()?;
    let summary = format!("{check} firing");
    Ok(Signal::firing(
        SourceKind::Agent,
        host,
        check,
        target,
        Severity::Warning,
        summary,
        T0,
    ))
}

/// A pending incident for the [`signal`] with the same arguments, opened at [`T0`].
pub fn incident(host: HostId, check: &str, target: Option<&str>) -> Result<Incident, BuildError> {
    Ok(Incident::new_pending(&signal(host, check, target)?, T0))
}

/// An open incident: [`incident`] after one immediate firing at [`T0`].
pub fn open_incident(
    host: HostId,
    check: &str,
    target: Option<&str>,
) -> Result<Incident, BuildError> {
    let mut incident = incident(host, check, target)?;
    let firing = IncidentEvent::Firing {
        severity: Severity::Warning,
        class: SignalClass::Immediate,
        observed_at: T0,
        value: None,
    };
    apply(&mut incident, firing, &IncidentPolicy::default(), T0)?;
    Ok(incident)
}

/// An [`open_incident`] that resolved at `resolved_at`.
pub fn resolved_incident(
    host: HostId,
    check: &str,
    target: Option<&str>,
    resolved_at: Timestamp,
) -> Result<Incident, BuildError> {
    let mut incident = open_incident(host, check, target)?;
    let resolved = IncidentEvent::Resolved {
        observed_at: resolved_at,
    };
    apply(
        &mut incident,
        resolved,
        &IncidentPolicy::default(),
        resolved_at,
    )?;
    Ok(incident)
}

/// A valid manifest with no parameters: entry `bin/run`, 60 s timeout, 64 KiB output cap.
pub fn manifest(name: &str, risk: Risk) -> Result<SkillManifest, BuildError> {
    Ok(SkillManifest {
        name: SkillName::parse(name)?,
        description: format!("Test skill {name}"),
        risk,
        entry: "bin/run".to_owned(),
        params: Vec::new(),
        timeout: SignedDuration::from_secs(60),
        run_as: None,
        hosts: Vec::new(),
        applies_to: Vec::new(),
        verify: None,
        revert: None,
        max_output_bytes: 64 * 1024,
    })
}

/// An action proposed by triage at [`T0`] for the [`manifest`] `restart_container` with
/// `risk`, with a one-hour approval window.
pub fn action(incident: IncidentId, host: HostId, risk: Risk) -> Result<ActionRequest, BuildError> {
    let summary = manifest("restart_container", risk)?.summary();
    let args = summary.validate_args(&BTreeMap::new())?;
    Ok(ActionRequest::propose(
        incident,
        host,
        &summary,
        &args,
        Principal::System("triage".into()),
        SignedDuration::from_hours(1),
        T0,
    ))
}

#[cfg(test)]
mod tests {
    use domovoi_core::IncidentStatus;

    use super::*;

    #[test]
    fn builders_produce_valid_values() {
        let host = host("nas").unwrap();
        assert_eq!(host.name.as_str(), "nas");
        assert!(
            manifest("zpool_status", Risk::Read)
                .unwrap()
                .validate()
                .is_ok()
        );
        let open = open_incident(host.id, "disk.smart", Some("/dev/sda")).unwrap();
        assert_eq!(open.status(), IncidentStatus::Open);
        let resolved = resolved_incident(host.id, "disk.smart", None, at(60)).unwrap();
        assert_eq!(resolved.status(), IncidentStatus::Resolved);
        assert_eq!(resolved.resolved_at(), Some(at(60)));
        assert!(signal(host.id, "Bad Key", None).is_err());
        let action = action(open.id(), host.id, Risk::Write).unwrap();
        assert_eq!(action.created_at(), T0);
    }
}
