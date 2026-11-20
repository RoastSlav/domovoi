//! The context sent with the first message of a model call. The wiring only: which sources
//! each call asks, the plan and the size limits. The sources live in `crate::context` and
//! the aggregator does the rest.

use std::sync::Arc;

use domovoi_core::context::ContextScope;
use domovoi_core::ports::{ContextSource, Presence};
use domovoi_core::{HostId, Incident, SkillSummary};

use super::Shared;
use crate::context::sources::{
    ChangesSource, IncidentSource, NotesSource, ProfileSource, SignalsSource,
    SimilarIncidentsSource, SkillsSource, ThreadSource,
};
use crate::context::{Aggregator, ContextBudget, ContextPlan, ContextPurpose};

/// Longest search text, in characters.
const SEARCH_CHARS: usize = 200;

/// Builds the redacted context for an investigation of `incident`.
pub(crate) async fn build(shared: &Shared, incident: &Incident, skills: &[SkillSummary]) -> String {
    incident_context(shared, ContextPurpose::Investigate, incident, skills).await
}

/// Builds the redacted context for a follow-up question about `incident`: everything an
/// investigation sees, and the incident's report, actions and timeline.
pub(crate) async fn build_follow_up(
    shared: &Shared,
    incident: &Incident,
    skills: &[SkillSummary],
) -> String {
    incident_context(shared, ContextPurpose::FollowUp, incident, skills).await
}

/// Builds the redacted context for a chat about `host`: its profile and the read-only skills.
pub(crate) async fn build_chat(
    shared: &Shared,
    host: HostId,
    read_skills: &[SkillSummary],
) -> String {
    let deps = &shared.deps;
    let mut sources: Vec<Arc<dyn ContextSource>> = vec![
        Arc::new(ProfileSource::new(deps.clone())),
        Arc::new(SkillsSource::new(
            deps.clone(),
            read_skills.to_vec(),
            connected(shared, host),
        )),
    ];
    sources.extend(deps.extra_context.iter().cloned());
    let plan = ContextPlan {
        purpose: ContextPurpose::Chat,
        scope: ContextScope::Host(host),
        search_text: None,
        budget: ContextBudget::for_purpose(ContextPurpose::Chat, shared.settings.max_prompt_bytes),
    };
    gather(shared, sources, &plan).await
}

async fn incident_context(
    shared: &Shared,
    purpose: ContextPurpose,
    incident: &Incident,
    skills: &[SkillSummary],
) -> String {
    let deps = &shared.deps;
    let mut sources: Vec<Arc<dyn ContextSource>> = vec![
        Arc::new(IncidentSource::new(deps.clone())),
        Arc::new(SignalsSource::new(deps.clone())),
    ];
    if purpose == ContextPurpose::FollowUp {
        sources.push(Arc::new(ThreadSource::new(deps.clone())));
    }
    sources.push(Arc::new(ProfileSource::new(deps.clone())));
    sources.push(Arc::new(NotesSource::new(deps.clone(), incident.clone())));
    sources.push(Arc::new(ChangesSource::new(deps.clone())));
    sources.push(Arc::new(SimilarIncidentsSource::new(
        deps.clone(),
        incident.clone(),
    )));
    sources.push(Arc::new(SkillsSource::new(
        deps.clone(),
        skills.to_vec(),
        connected(shared, incident.host()),
    )));
    sources.extend(deps.extra_context.iter().cloned());
    let plan = ContextPlan {
        purpose,
        scope: ContextScope::Incident {
            host: incident.host(),
            incident: incident.id(),
        },
        search_text: search_text(shared, incident),
        budget: ContextBudget::for_purpose(purpose, shared.settings.max_prompt_bytes),
    };
    gather(shared, sources, &plan).await
}

async fn gather(
    shared: &Shared,
    sources: Vec<Arc<dyn ContextSource>>,
    plan: &ContextPlan,
) -> String {
    let deps = &shared.deps;
    let aggregator = Aggregator::new(sources, deps.redactor.clone(), deps.clock.clone());
    deps.redactor
        .redact(&aggregator.gather(plan).await.render())
}

fn connected(shared: &Shared, host: HostId) -> bool {
    matches!(
        shared.deps.catalog.presence(host),
        Presence::Connected { .. }
    )
}

/// What a search source is asked for: the incident's check words and its target.
fn search_text(shared: &Shared, incident: &Incident) -> Option<String> {
    let mut text = incident.check().as_str().replace(['.', '_'], " ");
    if let Some(target) = incident.target() {
        text.push(' ');
        text.push_str(target.as_str());
    }
    let text = shared.deps.redactor.redact(text.trim());
    let text: String = text.chars().take(SEARCH_CHARS).collect();
    (!text.is_empty()).then_some(text)
}
