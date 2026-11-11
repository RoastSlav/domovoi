//! The contract for listing incidents by group and by text.

use domovoi_core::ports::{IncidentFilter, IncidentRepo, Page};
use domovoi_core::{
    HostId, Incident, IncidentEvent, IncidentGroup, IncidentId, IncidentPolicy, UserId, Verdict,
    apply,
};

use super::{ContractError, ContractResult};
use crate::build::{BuildError, T0, at, incident, open_incident, resolved_incident};

fn with_verdict(mut incident: Incident, verdict: Verdict) -> Incident {
    incident.set_verdict(Some(verdict));
    incident
}

fn acked(mut incident: Incident) -> Result<Incident, BuildError> {
    let event = IncidentEvent::Ack { by: UserId::new() };
    apply(&mut incident, event, &IncidentPolicy::default(), T0)?;
    Ok(incident)
}

async fn ids(
    repo: &dyn IncidentRepo,
    filter: IncidentFilter,
) -> Result<Vec<IncidentId>, ContractError> {
    let mut ids: Vec<IncidentId> = repo
        .list(&filter, &Page::first(100))
        .await?
        .items
        .iter()
        .map(Incident::id)
        .collect();
    ids.sort();
    Ok(ids)
}

/// Every incident is listed in exactly the group [`Incident::group`] says, and the text
/// filter matches the title, check and target literally, ignoring case.
pub(super) async fn groups_and_text(repo: &dyn IncidentRepo) -> ContractResult {
    let host = HostId::new();
    let stored = [
        open_incident(host, "grp.open", Some("/srv"))?,
        with_verdict(open_incident(host, "grp.act", None)?, Verdict::ActionNeeded),
        with_verdict(open_incident(host, "grp.watch", None)?, Verdict::Monitor),
        with_verdict(open_incident(host, "grp.known", None)?, Verdict::Expected),
        acked(with_verdict(
            open_incident(host, "grp.seen", None)?,
            Verdict::ActionNeeded,
        ))?,
        incident(host, "grp.wait", None)?,
        with_verdict(
            resolved_incident(host, "grp.done", None, at(60))?,
            Verdict::Monitor,
        ),
    ];
    for incident in &stored {
        repo.insert(incident).await?;
    }
    let on_host = IncidentFilter {
        host: Some(host),
        ..IncidentFilter::default()
    };

    for group in IncidentGroup::ALL {
        let mut expected: Vec<IncidentId> = stored
            .iter()
            .filter(|i| i.group() == group)
            .map(Incident::id)
            .collect();
        expected.sort();
        assert!(!expected.is_empty(), "the contract covers {group:?}");
        let filter = IncidentFilter {
            group: Some(group),
            ..on_host.clone()
        };
        assert_eq!(ids(repo, filter).await?, expected, "{group:?}");
    }

    let text = |text: &str| IncidentFilter {
        text: Some(text.to_owned()),
        ..on_host.clone()
    };
    assert_eq!(ids(repo, text("GRP.W")).await?.len(), 2, "check, any case");
    assert_eq!(ids(repo, text("/srv")).await?.len(), 1, "target");
    assert_eq!(ids(repo, text("watch firing")).await?.len(), 1, "title");
    assert_eq!(ids(repo, text("nothing")).await?.len(), 0);
    assert_eq!(ids(repo, text("%")).await?.len(), 0, "no wildcards");
    assert_eq!(ids(repo, text("grp_open")).await?.len(), 0, "no wildcards");
    let both = IncidentFilter {
        group: Some(IncidentGroup::Monitor),
        ..text("grp.w")
    };
    assert_eq!(ids(repo, both).await?.len(), 1, "filters combine");
    Ok(())
}
