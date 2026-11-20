//! The layered context the model reads: sources, prompts end to end and size limits.
//! Every source is checked through the prompt a world produces, since the sources are
//! internal to the engine.

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::context::{ContextLayer, ContextSelector, ContextSourceId};
use domovoi_core::ports::{
    ContextSource, EffectKind, EffectRead, EffectStatus, IncidentRepo, LinkError, LlmRequest,
    NoteRepo, Part, Presence, SignalRepo, ToolOutput,
};
use domovoi_core::{
    CheckKey, HostId, Incident, IncidentEvent, IncidentPolicy, Note, NoteId, ReadToolCall, Risk,
    Severity, Signal, SignalClass, SkillSummary, SourceKind, TargetKey, TriageOutcome, UserId,
    apply,
};
use domovoi_engine::context::CONTEXT_GUIDE;
use domovoi_engine::triage::{
    CHAT_PROMPT, FOLLOW_UP_PROMPT, META_SYSTEM_PROMPT, SYSTEM_PROMPT, TriageSettings,
};
use domovoi_testkit::build::{T0, manifest, open_incident, resolved_incident};
use domovoi_testkit::context::SelectorKind;
use domovoi_testkit::link::LinkRequest;
use domovoi_testkit::{FakeClock, FakeContextSource, ListRedactor};
use jiff::SignedDuration;
use jiff::Timestamp;

use crate::harness::{
    SECRET, Setup, World, host_id, read_calls, report, restart_skill, seen, world,
};

/// The tag the hub puts on the note it generates from the host profile.
const PROFILE_TAG: &str = "host-profile";

/// The closing section that names what a cap left out.
const LEFT_OUT: &str = "## Context left out to fit the size limit";

/// The text of the first message of `request`.
pub(crate) fn opening(request: &LlmRequest) -> String {
    let first = request.messages.first().expect("a first message");
    first
        .parts
        .iter()
        .filter_map(|part| match part {
            Part::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<&str>>()
        .join("\n")
}

/// The first message of the first request the primary model received.
pub(crate) fn first_opening(w: &World) -> String {
    opening(w.primary.requests().first().expect("a model request"))
}

/// The section of `text` under `## heading`, heading included, up to the next heading.
pub(crate) fn section(text: &str, heading: &str) -> String {
    let start = text
        .find(&format!("## {heading}\n\n"))
        .unwrap_or_else(|| panic!("no section {heading:?} in {text}"));
    let rest = &text[start..];
    let end = rest[3..].find("\n## ").map_or(rest.len(), |at| at + 3);
    rest[..end].to_owned()
}

/// Where `needle` first occurs in `text`.
pub(crate) fn at(text: &str, needle: &str) -> usize {
    text.find(needle)
        .unwrap_or_else(|| panic!("{needle:?} is not in {text}"))
}

/// A signal repository over nothing that answers `listing` and stores nothing.
struct StubSignals {
    listing: Result<Vec<Signal>, domovoi_core::ports::StoreError>,
}

#[async_trait]
impl SignalRepo for StubSignals {
    async fn insert(
        &self,
        _signal: &Signal,
        _incident: Option<domovoi_core::IncidentId>,
    ) -> Result<(), domovoi_core::ports::StoreError> {
        Ok(())
    }

    async fn list_for_incident(
        &self,
        _incident: domovoi_core::IncidentId,
        _limit: u16,
    ) -> Result<Vec<Signal>, domovoi_core::ports::StoreError> {
        match &self.listing {
            Ok(signals) => Ok(signals.clone()),
            Err(_) => Err(domovoi_core::ports::StoreError::Backend(
                "the signal table is gone".to_owned(),
            )),
        }
    }

    async fn purge_older_than(
        &self,
        _before: Timestamp,
    ) -> Result<u64, domovoi_core::ports::StoreError> {
        Ok(0)
    }
}

/// A scripted source that does not know the harness secret, so a secret in what it says is
/// removed by the aggregator and nothing else.
fn fake(id: &str, layer: ContextLayer) -> Arc<FakeContextSource> {
    Arc::new(FakeContextSource::new(
        ContextSourceId::parse(id).expect("a source id"),
        layer,
        Arc::new(ListRedactor::default()),
        Arc::new(FakeClock::new(T0)),
    ))
}

fn extra(sources: &[&Arc<FakeContextSource>]) -> Vec<Arc<dyn ContextSource>> {
    sources
        .iter()
        .map(|source| (*source).clone() as Arc<dyn ContextSource>)
        .collect()
}

fn disconnect(w: &World) {
    w.link
        .set_presence(host_id(), Presence::Disconnected { last_seen: None });
}

/// A note carrying the profile tag, updated at `at`.
fn profile_note(body: &str, at: Timestamp) -> Note {
    Note {
        id: NoteId::new(),
        host: Some(host_id()),
        title: "Host profile: nas".to_owned(),
        body_markdown: body.to_owned(),
        tags: vec![PROFILE_TAG.to_owned(), "generated".to_owned()],
        updated_at: at,
    }
}

/// A resolved `docker.container` incident on nginx for `host`, titled `summary`.
async fn resolved(w: &World, host: HostId, summary: &str) -> Incident {
    let signal = Signal::firing(
        SourceKind::Agent,
        host,
        CheckKey::parse("docker.container").expect("a check"),
        Some(TargetKey::parse("nginx").expect("a target")),
        Severity::Warning,
        summary.to_owned(),
        T0,
    );
    let mut incident = Incident::new_pending(&signal, T0);
    let policy = IncidentPolicy::default();
    let firing = IncidentEvent::Firing {
        severity: Severity::Warning,
        class: SignalClass::Immediate,
        observed_at: T0,
        value: None,
    };
    apply(&mut incident, firing, &policy, T0).expect("fires");
    let resolved = IncidentEvent::Resolved { observed_at: T0 };
    apply(&mut incident, resolved, &policy, T0).expect("resolves");
    IncidentRepo::insert(&*w.store, &incident)
        .await
        .expect("stored");
    incident
}

/// The calls of `w.link` that read container events.
fn docker_event_reads(w: &World) -> Vec<LinkRequest> {
    w.link
        .requests()
        .into_iter()
        .filter(|request| {
            matches!(
                request,
                LinkRequest::Tool {
                    call: ReadToolCall::DockerEvents { .. },
                    ..
                }
            )
        })
        .collect()
}

fn read_skill_with(description: &str) -> SkillSummary {
    let mut manifest = manifest("disk_report", Risk::Read).expect("a manifest");
    manifest.description = description.to_owned();
    manifest.summary(domovoi_core::ScriptDigest::of_bytes(b"read entry"))
}

#[tokio::test]
async fn the_incident_source_gives_the_old_snapshot_lines() {
    let w = world(Setup::default()).await;
    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));

    let text = first_opening(&w);
    let incident = section(&text, "Incident");
    assert!(
        incident.contains("[incident | incident | known]"),
        "{incident}"
    );
    for line in [
        "Title: ",
        "Check: docker.container",
        "Target: nginx",
        "Host id: ",
        "Severity: warning",
        "Status: Open",
        "Opened: ",
        "Last firing: ",
        "Firing signals so far: 1",
    ] {
        assert!(incident.contains(line), "{line:?} is missing: {incident}");
    }
}

#[tokio::test]
async fn a_signals_read_failure_is_an_unknown_section_not_none() {
    let w = world(Setup {
        signals: Some(Arc::new(StubSignals {
            listing: Err(domovoi_core::ports::StoreError::NotFound),
        })),
        ..Setup::default()
    })
    .await;
    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));

    let signals = section(&first_opening(&w), "Recent signals, newest first");
    assert!(
        signals.contains("UNKNOWN: the recent signals could not be read"),
        "{signals}"
    );
    assert!(!signals.contains("None."), "{signals}");
}

#[tokio::test]
async fn no_signals_renders_none() {
    let w = world(Setup {
        signals: Some(Arc::new(StubSignals {
            listing: Ok(vec![]),
        })),
        ..Setup::default()
    })
    .await;
    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));

    let signals = section(&first_opening(&w), "Recent signals, newest first");
    assert_eq!(
        signals.trim_end(),
        "## Recent signals, newest first\n\nNone."
    );
}

#[tokio::test]
async fn the_profile_item_turns_stale_after_eight_days() {
    let w = world(Setup::default()).await;
    let body = [
        "# Host profile: nas",
        "",
        "Debian GNU/Linux 13, 4 logical CPUs",
        "",
        "## Details",
        "",
        "uptime: raw tool output that never reaches the model",
    ]
    .join("\n");
    NoteRepo::upsert(&*w.store, &profile_note(&body, T0))
        .await
        .expect("stored");

    w.clock.advance(SignedDuration::from_hours(7 * 24));
    w.primary.push_text("Fresh enough.");
    w.triage
        .follow_up(w.incident.id(), "Is the profile current?", UserId::new())
        .await
        .expect("an answer");
    w.clock.advance(SignedDuration::from_hours(2 * 24));
    w.primary.push_text("Old now.");
    w.triage
        .follow_up(w.incident.id(), "And now?", UserId::new())
        .await
        .expect("an answer");

    let requests = w.primary.requests();
    assert_eq!(requests.len(), 2);
    let young = section(
        &opening(&requests[0]),
        "Host profile (the normal state of this host)",
    );
    assert!(young.contains("[host | profile | known]"), "{young}");
    assert!(young.contains("4 logical CPUs"), "{young}");
    let old = section(
        &opening(&requests[1]),
        "Host profile (the normal state of this host)",
    );
    assert!(
        old.contains(&format!("[host | profile | stale since {T0}]")),
        "{old}"
    );
    for request in &requests {
        assert!(
            !seen(request).contains("raw tool output that never reaches the model"),
            "the output behind ## Details stays out of the request"
        );
    }
}

#[tokio::test]
async fn a_missing_profile_note_renders_none() {
    let w = world(Setup::default()).await;
    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));

    let profile = section(
        &first_opening(&w),
        "Host profile (the normal state of this host)",
    );
    assert_eq!(
        profile.trim_end(),
        "## Host profile (the normal state of this host)\n\nNone."
    );
}

#[tokio::test]
async fn notes_are_declared_by_a_person_and_exclude_the_profile() {
    let w = world(Setup::default()).await;
    NoteRepo::upsert(
        &*w.store,
        &profile_note("docker container nginx profile summary", T0),
    )
    .await
    .expect("stored");
    for title in ["one", "two", "three"] {
        w.note(
            &format!("nginx runbook {title}"),
            &format!("docker container nginx runbook {title}"),
        )
        .await;
    }
    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));

    let notes = section(&first_opening(&w), "Host notes");
    assert_eq!(
        notes.matches("[host | notes | known]").count(),
        3,
        "{notes}"
    );
    assert_eq!(
        notes.matches("from: declared by a person note:").count(),
        3,
        "{notes}"
    );
    assert!(!notes.contains("Host profile: nas"), "{notes}");
    assert!(!notes.contains("profile summary"), "{notes}");
}

#[tokio::test]
async fn a_disconnected_host_makes_container_events_and_skills_unknown() {
    let w = world(Setup::default()).await;
    disconnect(&w);
    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));

    let text = first_opening(&w);
    let changes = section(&text, "What changed in the last 24 hours");
    assert!(
        changes.contains(
            "UNKNOWN: the host's agent is not connected, so its container events of the last 24 hours were not read"
        ),
        "{changes}"
    );
    let skills = section(&text, "Skills you may propose");
    assert!(
        skills.contains("UNKNOWN: the host's agent is not connected, so its skills are not known"),
        "{skills}"
    );
    assert!(!skills.contains("None."), "{skills}");
    assert!(
        docker_event_reads(&w).is_empty(),
        "nothing was asked of the agent"
    );
}

#[tokio::test]
async fn container_events_are_read_once_through_the_evidence_boundary() {
    let w = world(Setup::default()).await;
    w.link.on_tool(|_, call| {
        Ok(ToolOutput {
            text: format!("events: nginx died ({})", call.tool().name()),
            truncated: false,
        })
    });
    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));

    let changes = section(&first_opening(&w), "What changed in the last 24 hours");
    assert!(changes.contains("Docker events:"), "{changes}");
    assert!(
        changes.contains("events: nginx died (docker_events)"),
        "{changes}"
    );
    assert!(
        changes.contains("[evidence | changes | known]"),
        "{changes}"
    );
    let reads = docker_event_reads(&w);
    assert_eq!(reads.len(), 1, "one read for the context: {reads:?}");
    assert!(
        matches!(
            &reads[0],
            LinkRequest::Tool {
                call: ReadToolCall::DockerEvents {
                    container: None,
                    since_minutes: 1440
                },
                ..
            }
        ),
        "{reads:?}"
    );
}

#[tokio::test]
async fn a_failing_container_events_read_is_unknown_not_none() {
    let w = world(Setup::default()).await;
    w.link.on_tool(|_, call| match call {
        ReadToolCall::DockerEvents { .. } => Err(LinkError::Timeout),
        other => Ok(ToolOutput {
            text: format!("output of {}", other.tool().name()),
            truncated: false,
        }),
    });
    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));

    let changes = section(&first_opening(&w), "What changed in the last 24 hours");
    assert!(changes.contains("UNKNOWN: "), "{changes}");
    assert!(!changes.contains("None."), "{changes}");
    assert!(!changes.contains("Docker events:"), "{changes}");
}

#[tokio::test]
async fn reboots_are_listed_as_known() {
    let w = world(Setup::default()).await;
    let reboot = resolved_incident(host_id(), "system.reboot", None, T0).expect("built");
    IncidentRepo::insert(&*w.store, &reboot)
        .await
        .expect("stored");
    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));

    let changes = section(&first_opening(&w), "What changed in the last 24 hours");
    assert!(
        changes.contains("[evidence | changes | known] Reboots"),
        "{changes}"
    );
    assert!(changes.contains("- The host rebooted around"), "{changes}");
}

#[tokio::test]
async fn similar_incidents_are_historical_and_host_scoped() {
    let w = world(Setup::default()).await;
    let mine = resolved(&w, host_id(), "nginx on host alpha").await;
    let elsewhere = resolved(&w, HostId::new(), "nginx on host beta").await;
    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));

    let similar = section(&first_opening(&w), "Similar resolved incidents");
    assert!(
        similar.contains("historical evidence, not authorization"),
        "{similar}"
    );
    assert!(similar.contains(&mine.id().to_string()), "{similar}");
    assert!(
        !seen(&w.primary.requests()[0]).contains(&elsewhere.id().to_string()),
        "another host's incident never reaches the model"
    );
}

#[tokio::test]
async fn skills_are_unknown_when_disconnected_and_none_when_connected_without_skills() {
    let connected = world(Setup::default()).await;
    connected.primary.push_text(report());
    assert!(matches!(connected.run().await, TriageOutcome::Report(_)));
    let skills = section(&first_opening(&connected), "Skills you may propose");
    assert_eq!(skills.trim_end(), "## Skills you may propose\n\nNone.");

    let away = world(Setup::default()).await;
    disconnect(&away);
    away.primary.push_text(report());
    assert!(matches!(away.run().await, TriageOutcome::Report(_)));
    let skills = section(&first_opening(&away), "Skills you may propose");
    assert!(
        skills.contains("UNKNOWN: the host's agent is not connected, so its skills are not known"),
        "{skills}"
    );
}

/// An action repository whose listing always fails; nothing else is used.
struct FailingActions;

#[async_trait]
impl domovoi_core::ports::ActionRepo for FailingActions {
    async fn insert(
        &self,
        _action: &domovoi_core::ActionRequest,
    ) -> Result<(), domovoi_core::ports::StoreError> {
        Ok(())
    }

    async fn update_from(
        &self,
        _action: &domovoi_core::ActionRequest,
        _expected: &str,
    ) -> Result<(), domovoi_core::ports::StoreError> {
        Ok(())
    }

    async fn get(
        &self,
        _id: domovoi_core::ActionId,
    ) -> Result<Option<domovoi_core::ActionRequest>, domovoi_core::ports::StoreError> {
        Ok(None)
    }

    async fn list(
        &self,
        _filter: &domovoi_core::ports::ActionFilter,
        _page: &domovoi_core::ports::Page,
    ) -> Result<
        domovoi_core::ports::Paged<domovoi_core::ActionRequest>,
        domovoi_core::ports::StoreError,
    > {
        Err(domovoi_core::ports::StoreError::Backend(
            "the action table is gone".to_owned(),
        ))
    }

    async fn find_by_chat_token_hash(
        &self,
        _hash: &[u8; 32],
    ) -> Result<Option<domovoi_core::ActionRequest>, domovoi_core::ports::StoreError> {
        Ok(None)
    }

    async fn last_run_by_skill(
        &self,
        _host: domovoi_core::HostId,
    ) -> Result<
        std::collections::BTreeMap<domovoi_core::SkillName, Timestamp>,
        domovoi_core::ports::StoreError,
    > {
        Ok(std::collections::BTreeMap::new())
    }
}

#[tokio::test]
async fn an_actions_read_failure_is_an_unknown_section_not_none() {
    let w = world(Setup {
        actions: Some(Arc::new(FailingActions)),
        ..Setup::default()
    })
    .await;
    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));
    w.primary.push_text("Nothing new.");
    w.triage
        .follow_up(w.incident.id(), "What is the plan?", UserId::new())
        .await
        .expect("an answer");

    let requests = w.primary.requests();
    let text = opening(&requests[1]);
    let thread = section(&text, "This incident so far: report, actions and timeline");
    assert!(
        thread.contains("UNKNOWN: the incident's actions could not be read"),
        "{thread}"
    );
    assert!(
        !thread.contains("Actions of this incident"),
        "no actions item when they could not be read: {thread}"
    );
}

#[tokio::test]
async fn the_thread_source_carries_report_actions_and_timeline() {
    let w = world(Setup::default()).await;
    w.primary.push_text(report());
    let TriageOutcome::Report(found) = w.run().await else {
        panic!("a report");
    };
    w.seed_legacy_action("nginx").await;
    w.primary.push_text("Nothing new.");
    w.triage
        .follow_up(w.incident.id(), "What is the plan?", UserId::new())
        .await
        .expect("an answer");

    let requests = w.primary.requests();
    let text = opening(&requests[1]);
    let thread = section(&text, "This incident so far: report, actions and timeline");
    let report_at = at(&thread, "Latest triage report");
    let actions_at = at(&thread, "Actions of this incident");
    let timeline_at = at(&thread, "Timeline of this incident, oldest first");
    assert!(
        report_at < actions_at && actions_at < timeline_at,
        "{thread}"
    );
    assert!(thread.contains(found.cause.as_str()), "{thread}");
    assert!(
        thread.contains("restart_container container=nginx status: proposed"),
        "{thread}"
    );
    assert!(
        thread.contains("triage.report: nginx keeps restarting"),
        "{thread}"
    );
    for old in [
        "## Latest triage report",
        "## Actions of this incident",
        "## Timeline",
        "## Report",
    ] {
        assert!(
            !text.contains(old),
            "the old heading {old:?} came back: {text}"
        );
    }
    assert_eq!(
        text.matches("## This incident so far").count(),
        1,
        "one heading for the whole thread"
    );
}

#[tokio::test]
async fn the_search_text_is_the_old_notes_query() {
    let search = fake("probe", ContextLayer::Knowledge);
    search.answer(SelectorKind::Search, vec!["a hit".to_owned()]);
    let w = world(Setup {
        extra_context: extra(&[&search]),
        ..Setup::default()
    })
    .await;
    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));

    let asked = |source: &FakeContextSource| -> Vec<String> {
        source
            .queries()
            .into_iter()
            .filter_map(|query| match query.selector {
                ContextSelector::Search { text } => Some(text),
                _ => None,
            })
            .collect()
    };
    assert_eq!(asked(&search), vec!["docker container nginx".to_owned()]);

    // An incident without a target is searched by its check words alone.
    let bare = open_incident(host_id(), "disk.usage", None).expect("built");
    IncidentRepo::insert(&*w.store, &bare)
        .await
        .expect("stored");
    w.primary.push_text(report());
    w.run_on(&bare).await;
    let all = asked(&search);
    assert_eq!(
        all.last().map(String::as_str),
        Some("disk usage"),
        "{all:?}"
    );
}

#[tokio::test]
async fn the_search_text_is_cut_to_200_characters_and_redacted() {
    let search = fake("probe", ContextLayer::Knowledge);
    search.answer(SelectorKind::Search, vec!["a hit".to_owned()]);
    let w = world(Setup {
        extra_context: extra(&[&search]),
        ..Setup::default()
    })
    .await;
    let long_check = format!("{}.{}", "a".repeat(30), "b".repeat(30));
    let secret_target = open_incident(
        host_id(),
        &long_check,
        Some(&format!("{SECRET}-{}", "t".repeat(90))),
    )
    .expect("built");
    IncidentRepo::insert(&*w.store, &secret_target)
        .await
        .expect("stored");
    w.primary.push_text(report());
    w.run_on(&secret_target).await;

    let texts: Vec<String> = search
        .queries()
        .into_iter()
        .filter_map(|query| match query.selector {
            ContextSelector::Search { text } => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(texts.len(), 1, "{texts:?}");
    assert!(texts[0].chars().count() <= 200, "{}", texts[0]);
    assert!(!texts[0].contains(SECRET), "the search text is redacted");
    assert!(texts[0].starts_with(&"a".repeat(30)), "{}", texts[0]);
}

#[tokio::test]
async fn the_investigation_prompt_is_layered_in_order() {
    let w = world(Setup::default()).await;
    NoteRepo::upsert(&*w.store, &profile_note("a quiet host", T0))
        .await
        .expect("stored");
    w.note("nginx runbook", "docker container nginx runbook")
        .await;
    resolved(&w, host_id(), "nginx restarted after a config change").await;
    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));

    let text = first_opening(&w);
    let headings = [
        "## Incident\n",
        "## Recent signals, newest first\n",
        "## Host profile (the normal state of this host)\n",
        "## Host notes\n",
        "## What changed in the last 24 hours\n",
        "## Similar resolved incidents\n",
        "## Skills you may propose\n",
    ];
    let positions: Vec<usize> = headings.iter().map(|heading| at(&text, heading)).collect();
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "the headings are out of order: {positions:?}\n{text}"
    );
}

/// `text` with ids and times replaced by placeholders, so a snapshot stays stable.
fn stable(text: &str) -> String {
    let ids = regex::Regex::new(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")
        .expect("a pattern");
    let times =
        regex::Regex::new(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?Z").expect("a pattern");
    let text = ids.replace_all(text, "[uuid]");
    times.replace_all(&text, "[time]").into_owned()
}

/// The profile note every golden prompt shares.
fn golden_profile() -> Note {
    profile_note(
        "# Host profile: nas\n\n## Summary\n\nDebian GNU/Linux 13, 4 logical CPUs, 32 GiB memory\n\n## Details\n\nraw output",
        T0,
    )
}

#[tokio::test]
async fn golden_investigation_prompt() {
    let w = world(Setup::default()).await;
    w.link.set_skills(host_id(), vec![restart_skill()]);
    NoteRepo::upsert(&*w.store, &golden_profile())
        .await
        .expect("stored");
    w.note(
        "nginx runbook",
        "docker container nginx: restart it after a config change",
    )
    .await;
    resolved(&w, host_id(), "nginx restarted after a config change").await;
    let reboot = resolved_incident(host_id(), "system.reboot", None, T0).expect("built");
    IncidentRepo::insert(&*w.store, &reboot)
        .await
        .expect("stored");
    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));

    insta::assert_snapshot!("golden_investigation_prompt", stable(&first_opening(&w)));
}

#[tokio::test]
async fn golden_follow_up_prompt() {
    let w = world(Setup::default()).await;
    w.link.set_skills(host_id(), vec![restart_skill()]);
    w.note(
        "nginx runbook",
        "docker container nginx: restart it after a config change",
    )
    .await;
    resolved(&w, host_id(), "nginx restarted after a config change").await;
    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));
    w.seed_legacy_action("nginx").await;
    w.primary.push_text("Nothing new.");
    w.triage
        .follow_up(w.incident.id(), "What is the plan?", UserId::new())
        .await
        .expect("an answer");

    insta::assert_snapshot!(
        "golden_follow_up_prompt",
        stable(&opening(&w.primary.requests()[1]))
    );
}

#[tokio::test]
async fn golden_host_chat_prompt() {
    let w = world(Setup::default()).await;
    w.link.set_skills(
        host_id(),
        vec![read_skill_with("Reports the disk usage of every mount.")],
    );
    NoteRepo::upsert(&*w.store, &golden_profile())
        .await
        .expect("stored");
    w.primary.push_text("The disks look fine.");
    w.triage
        .chat(
            Some(host_id()),
            "The NAS and the pi.",
            &Vec::new(),
            "How are the disks?",
        )
        .await
        .expect("an answer");

    insta::assert_snapshot!("golden_host_chat_prompt", stable(&first_opening(&w)));
}

#[tokio::test]
async fn an_extra_source_is_asked_and_labelled() {
    let knowledge = fake("runbooks", ContextLayer::Knowledge);
    knowledge.answer(
        SelectorKind::Overview,
        vec!["restart after a config change".to_owned()],
    );
    knowledge.answer(
        SelectorKind::Search,
        vec!["same id as the first".to_owned(), "nginx notes".to_owned()],
    );
    let experience = fake("past_ops", ContextLayer::Experience);
    experience.answer(
        SelectorKind::Overview,
        vec!["restarted nginx last week".to_owned()],
    );
    let w = world(Setup {
        extra_context: extra(&[&knowledge, &experience]),
        ..Setup::default()
    })
    .await;
    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));

    let text = first_opening(&w);
    let similar = at(&text, "## Similar resolved incidents");
    let past = at(&text, "## Historical evidence, not authorization: past_ops");
    let runbooks = at(&text, "## Knowledge: runbooks");
    assert!(
        similar < past && past < runbooks,
        "the extra sources follow the built-ins in layer order: {text}"
    );
    let past_ops = section(&text, "Historical evidence, not authorization: past_ops");
    assert!(
        past_ops.contains("[experience | past_ops | known]")
            && past_ops.contains("historical evidence, not authorization"),
        "{past_ops}"
    );
    let runbook_section = section(&text, "Knowledge: runbooks");
    assert!(
        runbook_section.contains("restart after a config change")
            && runbook_section.contains("nginx notes"),
        "both the overview and the search answer are shown: {runbook_section}"
    );
    let searched = knowledge.queries().iter().any(|query| {
        matches!(&query.selector, ContextSelector::Search { text } if text == "docker container nginx")
    });
    assert!(searched, "a source that supports search was searched");
    assert!(
        experience
            .queries()
            .iter()
            .all(|query| matches!(query.selector, ContextSelector::Overview)),
        "a source without search is never searched"
    );
}

/// Large notes, signals and a profile, enough to overflow every cap.
async fn crowd(w: &World) {
    NoteRepo::upsert(
        &*w.store,
        &profile_note(
            &format!("## Summary\n\n{}", "profile line. ".repeat(900)),
            T0,
        ),
    )
    .await
    .expect("stored");
    for index in 0..5 {
        w.note(
            &format!("nginx runbook {index}"),
            &format!("docker container nginx {}", "long note text. ".repeat(1500)),
        )
        .await;
    }
    for index in 0..8 {
        let signal = Signal::firing(
            SourceKind::Agent,
            host_id(),
            CheckKey::parse("docker.container").expect("a check"),
            Some(TargetKey::parse("nginx").expect("a target")),
            Severity::Warning,
            format!(
                "nginx is restarting {index} {}",
                "again and again. ".repeat(200)
            ),
            T0 + SignedDuration::from_secs(index + 1),
        );
        SignalRepo::insert(&*w.store, &signal, Some(w.incident.id()))
            .await
            .expect("stored");
    }
}

#[tokio::test]
async fn the_context_stays_inside_the_derived_cap() {
    let big = fake("runbooks", ContextLayer::Knowledge);
    big.answer(SelectorKind::Overview, vec!["knowledge. ".repeat(900); 5]);
    let w = world(Setup {
        settings: TriageSettings {
            max_prompt_bytes: 32 * 1024,
            ..TriageSettings::default()
        },
        extra_context: extra(&[&big]),
        ..Setup::default()
    })
    .await;
    crowd(&w).await;
    w.primary.push_text(report());
    assert!(
        matches!(w.run().await, TriageOutcome::Report(_)),
        "the request is not refused"
    );

    let text = first_opening(&w);
    let context = text
        .strip_prefix("Triage this incident.\n\n")
        .unwrap_or(&text);
    let (kept, closing) = context
        .split_once(LEFT_OUT)
        .unwrap_or_else(|| panic!("nothing was left out: {context}"));
    assert!(
        kept.len() <= 6 * 1024,
        "{} bytes before the closing section",
        kept.len()
    );
    assert!(
        closing.contains("What is listed here was not shown to you"),
        "{closing}"
    );
    assert!(
        closing.contains("items from") || closing.contains("item from"),
        "the closing section names what went: {closing}"
    );
}

#[tokio::test]
async fn a_secret_in_a_note_a_signal_or_a_gap_never_reaches_the_request() {
    let failing = fake("flaky", ContextLayer::Knowledge);
    failing.fail(SelectorKind::Overview, &format!("cannot reach {SECRET}"));
    let w = world(Setup {
        extra_context: extra(&[&failing]),
        ..Setup::default()
    })
    .await;
    w.note(
        "nginx runbook",
        &format!("docker container nginx password {SECRET}"),
    )
    .await;
    let signal = Signal::firing(
        SourceKind::Agent,
        host_id(),
        CheckKey::parse("docker.container").expect("a check"),
        Some(TargetKey::parse("nginx").expect("a target")),
        Severity::Warning,
        format!("login failed with {SECRET}"),
        T0 + SignedDuration::from_secs(1),
    );
    SignalRepo::insert(&*w.store, &signal, Some(w.incident.id()))
        .await
        .expect("stored");
    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));

    let requests = w.primary.requests();
    assert!(!requests.is_empty());
    for request in &requests {
        assert!(
            !seen(request).contains(SECRET),
            "a secret reached the model"
        );
    }
    let text = first_opening(&w);
    assert!(
        text.contains("UNKNOWN: cannot reach"),
        "the gap is still shown: {text}"
    );
    assert!(text.contains("login failed with"), "{text}");
}

#[tokio::test]
async fn the_chat_context_for_a_chosen_host_reads_nothing_from_the_agent() {
    // A model that can call tools: the link sees only what the model asks for.
    let w = world(Setup::default()).await;
    w.primary.push_text("Nothing to read.");
    w.triage
        .chat(Some(host_id()), "One host.", &Vec::new(), "Anything wrong?")
        .await
        .expect("an answer");
    assert!(
        w.link.requests().is_empty(),
        "building the chat context asked the agent: {:?}",
        w.link.requests()
    );

    w.primary.push_ok(read_calls(&[(
        "c1",
        "journal",
        serde_json::json!({"unit": "nginx"}),
    )]));
    w.primary.push_text("The journal is quiet.");
    w.triage
        .chat(
            Some(host_id()),
            "One host.",
            &Vec::new(),
            "And the journal?",
        )
        .await
        .expect("an answer");
    assert_eq!(
        w.link.requests().len(),
        1,
        "only the model's own call: {:?}",
        w.link.requests()
    );

    // A model without tools: the three prefetch reads and nothing else.
    let cli = world(Setup {
        cli: true,
        ..Setup::default()
    })
    .await;
    cli.primary.push_text("Quiet.");
    cli.triage
        .chat(Some(host_id()), "One host.", &Vec::new(), "Anything wrong?")
        .await
        .expect("an answer");
    let requests = cli.link.requests();
    assert_eq!(requests.len(), 3, "the prefetch reads: {requests:?}");
    assert!(
        requests
            .iter()
            .all(|request| matches!(request, LinkRequest::Tool { .. })),
        "{requests:?}"
    );
}

#[tokio::test]
async fn chat_about_all_hosts_is_unchanged() {
    let w = world(Setup::default()).await;
    w.primary.push_text("Both calm.");
    w.triage
        .chat(
            None,
            "Two hosts: the NAS and the pi.",
            &Vec::new(),
            "Anything wrong?",
        )
        .await
        .expect("an answer");

    let requests = w.primary.requests();
    assert_eq!(opening(&requests[0]), "Two hosts: the NAS and the pi.");
    assert!(requests[0].tools.is_empty());
    assert!(w.link.requests().is_empty());
}

#[tokio::test]
async fn the_openings_stay_one_message() {
    let w = world(Setup::default()).await;
    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));
    w.primary.push_text("Answer.");
    w.triage
        .follow_up(w.incident.id(), "Why?", UserId::new())
        .await
        .expect("an answer");
    w.primary.push_text("Chat answer.");
    w.triage
        .chat(Some(host_id()), "One host.", &Vec::new(), "Chat question?")
        .await
        .expect("an answer");
    w.primary.push_text("All answer.");
    w.triage
        .chat(None, "Two hosts.", &Vec::new(), "All question?")
        .await
        .expect("an answer");

    let parts = |request: &LlmRequest| -> Vec<usize> {
        request
            .messages
            .iter()
            .map(|message| {
                message
                    .parts
                    .iter()
                    .filter(|part| matches!(part, Part::Text(_)))
                    .count()
            })
            .collect()
    };
    let requests = w.primary.requests();
    assert_eq!(requests.len(), 4);
    assert_eq!(parts(&requests[0]), vec![1], "the investigation");
    assert_eq!(parts(&requests[1]), vec![1, 1], "the follow-up");
    assert_eq!(parts(&requests[2]), vec![1, 1], "the host chat");
    assert_eq!(parts(&requests[3]), vec![1, 1], "the overview chat");
}

#[test]
fn every_system_prompt_carries_the_context_guide() {
    for (name, prompt) in [
        ("SYSTEM_PROMPT", SYSTEM_PROMPT),
        ("META_SYSTEM_PROMPT", META_SYSTEM_PROMPT),
        ("FOLLOW_UP_PROMPT", FOLLOW_UP_PROMPT),
        ("CHAT_PROMPT", CHAT_PROMPT),
    ] {
        assert!(
            prompt.contains(CONTEXT_GUIDE),
            "{name} does not carry the context guide"
        );
    }
    let bullets = SYSTEM_PROMPT
        .lines()
        .filter(|line| line.starts_with("- ") && line.contains("manual_steps"))
        .count();
    assert_eq!(bullets, 1, "one bullet explains manual_steps");
}

#[tokio::test]
async fn a_huge_skill_catalogue_is_cut_by_the_item_cap_in_the_chat_context() {
    let w = world(Setup::default()).await;
    w.link
        .set_skills(host_id(), vec![read_skill_with(&"d".repeat(10 * 1024))]);
    w.primary.push_text("Done.");
    w.triage
        .chat(
            Some(host_id()),
            "One host.",
            &Vec::new(),
            "What can you read?",
        )
        .await
        .expect("an answer");

    let requests = w.primary.requests();
    let text = opening(&requests[0]);
    let skills = section(&text, "Read-only skills you may use on this host");
    assert!(skills.contains("[cut]"), "{skills}");
    assert!(skills.contains("disk_report"), "{skills}");
    assert!(
        skills.len() < 4 * 1024 + 400,
        "the catalogue is cut at 4 KiB: {} bytes",
        skills.len()
    );
    let last = requests[0].messages.last().expect("a last message");
    assert!(
        matches!(&last.parts[0], Part::Text(question) if question == "What can you read?"),
        "the question is intact"
    );
}

#[tokio::test]
async fn an_investigation_context_fits_the_call_model_payload() {
    let big = fake("runbooks", ContextLayer::Knowledge);
    big.answer(SelectorKind::Overview, vec!["knowledge. ".repeat(900); 5]);
    let w = world(Setup {
        extra_context: extra(&[&big]),
        ..Setup::default()
    })
    .await;
    crowd(&w).await;
    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));

    let done = EffectRead::effects_with_status(&*w.store, EffectStatus::Done, 500)
        .await
        .expect("effects");
    let calls: Vec<_> = done
        .iter()
        .filter(|effect| effect.kind == EffectKind::CallModel.as_str())
        .collect();
    assert_eq!(calls.len(), 1, "one model call effect: {done:?}");
    let size = calls[0].payload.to_string().len();
    assert!(size <= 32 * 1024, "the effect payload is {size} bytes");
    assert!(
        size > 8 * 1024,
        "the test holds enough context to matter: {size} bytes"
    );
}
