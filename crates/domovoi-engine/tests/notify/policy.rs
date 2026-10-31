//! The notification policy: posts, folded replies, reminders, resolutions, quiet hours.

use domovoi_core::ports::MessageRef;
use domovoi_core::{ChatRefs, NotifyState, Severity, SignalId, UserId};
use domovoi_engine::EventKind;
use domovoi_engine::cards::{CardLinks, build_card};
use domovoi_engine::notify::{
    NotifyAction, NotifySettings, in_quiet_hours, plan, quiet_ends_at, tick,
};
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};

use crate::harness::{event, host, open, quiet, resolve, sofia, ts};

fn posted(at: Timestamp) -> NotifyState {
    let mut state = NotifyState::default();
    state.messages.insert(
        "discord".to_owned(),
        ChatRefs {
            message: MessageRef {
                platform: "discord".to_owned(),
                id: "m1".to_owned(),
            },
            thread: None,
        },
    );
    state.first_alert_at = Some(at);
    state
}

fn folded(n: u32) -> EventKind {
    EventKind::IncidentFolded {
        signal: SignalId::new(),
        occurrences: n,
    }
}

fn replies(actions: &[NotifyAction]) -> Vec<&str> {
    actions
        .iter()
        .filter_map(|a| match a {
            NotifyAction::ThreadReply { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn first_open_posts_card_and_thread() {
    let at = ts("2026-10-31T12:00:00Z");
    let incident = open(Severity::Warning, at);
    let settings = NotifySettings::new(TimeZone::UTC);
    let out = plan(
        &event(EventKind::IncidentOpened, &incident),
        &NotifyState::default(),
        &settings,
        at,
    );
    assert_eq!(
        out.actions,
        [NotifyAction::PostCard {
            incident: incident.id()
        }]
    );
    assert_eq!(out.state.first_alert_at, Some(at));
    // Triage requests say nothing by themselves.
    let triage = plan(
        &event(EventKind::TriageRequested, &incident),
        &out.state,
        &settings,
        at,
    );
    assert!(triage.actions.is_empty());
}

#[test]
fn alerts_post_without_triage() {
    // INV-11: an opened incident with no triage report is still posted, with a usable card.
    let at = ts("2026-10-31T12:00:00Z");
    let incident = open(Severity::Critical, at);
    let settings = NotifySettings::new(TimeZone::UTC);
    let out = plan(
        &event(EventKind::IncidentOpened, &incident),
        &NotifyState::default(),
        &settings,
        at,
    );
    assert_eq!(
        out.actions,
        [NotifyAction::PostCard {
            incident: incident.id()
        }]
    );
    let card = build_card(&incident, None, &[], &host(), &CardLinks::default(), None);
    assert!(!card.headline.is_empty());
    assert!(card.validate().is_ok());
}

#[test]
fn folded_signal_replies_once_per_30_minutes_with_counter() {
    let start = ts("2026-10-31T12:00:00Z");
    let incident = open(Severity::Warning, start);
    let settings = NotifySettings::new(TimeZone::UTC);
    let mut state = posted(start);
    let mut said = Vec::new();
    for (mins, n) in [(5, 2), (10, 3), (20, 4), (36, 5), (40, 6)] {
        let now = start + SignedDuration::from_mins(mins);
        let out = plan(&event(folded(n), &incident), &state, &settings, now);
        assert!(
            out.actions
                .iter()
                .all(|a| matches!(a, NotifyAction::ThreadReply { .. })),
            "no new card"
        );
        said.extend(replies(&out.actions).into_iter().map(str::to_owned));
        state = out.state;
    }
    assert_eq!(
        said,
        [
            "Again: plex restarted 7 times in 10 minutes (2nd time in 5m)",
            "Again: plex restarted 7 times in 10 minutes (5th time in 36m, +2 more)",
        ]
    );
    assert!(said.iter().all(|s| !s.contains('\n')), "one line each");
    assert_eq!(state.folds_since_reply, 1);
}

#[test]
fn warning_reminder_after_24h_and_critical_after_4h() {
    let start = ts("2026-10-31T00:00:00Z");
    let settings = NotifySettings::new(TimeZone::UTC);
    let warning = open(Severity::Warning, start);
    let mut critical = open(Severity::Critical, start);
    critical = critical.with_id(domovoi_core::IncidentId::new());
    let items = |w: &NotifyState, c: &NotifyState| {
        vec![(warning.clone(), w.clone()), (critical.clone(), c.clone())]
    };
    let (mut w, mut c) = (posted(start), posted(start));

    let early = tick(
        &items(&w, &c),
        &settings,
        start + SignedDuration::from_hours(3),
    );
    assert!(early.actions.is_empty());
    let four = tick(
        &items(&w, &c),
        &settings,
        start + SignedDuration::from_hours(4),
    );
    assert_eq!(
        replies(&four.actions),
        ["Still ongoing (4h): plex restarted 7 times in 10 minutes"]
    );
    for (id, state) in four.states {
        if id == critical.id() {
            c = state;
        }
    }
    let eight = tick(
        &items(&w, &c),
        &settings,
        start + SignedDuration::from_hours(8),
    );
    assert_eq!(eight.actions.len(), 1, "critical again after another 4 h");
    for (id, state) in eight.states {
        if id == critical.id() {
            c = state;
        }
    }
    let day = tick(
        &items(&w, &c),
        &settings,
        start + SignedDuration::from_hours(24),
    );
    let ids: Vec<_> = day
        .actions
        .iter()
        .filter_map(|a| match a {
            NotifyAction::ThreadReply { incident, .. } => Some(*incident),
            _ => None,
        })
        .collect();
    assert!(ids.contains(&warning.id()), "the warning after 24 h");
    for (id, state) in day.states {
        if id == warning.id() {
            w = state;
        }
    }
    let soon = tick(
        &items(&w, &c),
        &settings,
        start + SignedDuration::from_hours(25),
    );
    assert!(!soon.actions.iter().any(
        |a| matches!(a, NotifyAction::ThreadReply { incident, .. } if *incident == warning.id())
    ));
}

#[test]
fn quiet_hours_hold_non_critical_and_post_critical() {
    let settings = quiet((22, 0), (7, 0), TimeZone::UTC);
    let night = ts("2026-10-31T23:30:00Z");
    let warning = open(Severity::Warning, night);
    let held = plan(
        &event(EventKind::IncidentOpened, &warning),
        &NotifyState::default(),
        &settings,
        night,
    );
    assert!(matches!(held.actions[..], [NotifyAction::Hold { .. }]));
    assert!(held.state.is_held());

    let critical = open(Severity::Critical, night);
    let posted = plan(
        &event(EventKind::IncidentOpened, &critical),
        &NotifyState::default(),
        &settings,
        night,
    );
    assert_eq!(
        posted.actions,
        [NotifyAction::PostCard {
            incident: critical.id()
        }]
    );

    // Warnings are not reminded during quiet hours either.
    let mut state = crate::policy::posted(ts("2026-10-30T20:00:00Z"));
    state.first_alert_at = Some(ts("2026-10-30T20:00:00Z"));
    let reminders = tick(&[(warning.clone(), state)], &settings, night);
    assert!(reminders.actions.is_empty());
}

#[test]
fn held_queue_released_as_one_message_at_end_of_quiet_hours() {
    let settings = quiet((22, 0), (7, 0), TimeZone::UTC);
    let night = ts("2026-10-31T23:00:00Z");
    let mut disk = open(Severity::Warning, night);
    let unit = open(Severity::Info, night).with_id(domovoi_core::IncidentId::new());
    let open_event = |i: &domovoi_core::Incident| event(EventKind::IncidentOpened, i);
    let disk_state = plan(
        &open_event(&disk),
        &NotifyState::default(),
        &settings,
        night,
    )
    .state;
    let unit_state = plan(
        &open_event(&unit),
        &NotifyState::default(),
        &settings,
        night,
    )
    .state;
    // The disk recovers while held: it is listed as resolved, not posted.
    resolve(&mut disk, ts("2026-11-01T02:00:00Z"));
    let resolved = plan(
        &event(EventKind::IncidentResolved, &disk),
        &disk_state,
        &settings,
        ts("2026-11-01T02:00:00Z"),
    );
    assert!(resolved.actions.is_empty());

    let items = vec![(disk.clone(), resolved.state), (unit.clone(), unit_state)];
    let still_night = tick(&items, &settings, ts("2026-11-01T06:59:00Z"));
    assert!(still_night.release.is_none());
    let morning = tick(&items, &settings, ts("2026-11-01T07:00:00Z"));
    let message = morning.release.expect("one message when quiet hours end");
    assert_eq!(message.lines().count(), 3, "{message}");
    assert!(message.starts_with("While you were away: 2 alerts"));
    assert!(message.contains("(resolved)"));
    assert_eq!(morning.states.len(), 2);
    assert!(morning.states.iter().all(|(_, s)| !s.is_held()), "released");
}

#[test]
fn quiet_hours_spanning_midnight() {
    let settings = quiet((22, 0), (7, 0), TimeZone::UTC);
    for (at, inside) in [
        ("2026-10-31T21:59:00Z", false),
        ("2026-10-31T22:00:00Z", true),
        ("2026-10-31T23:59:00Z", true),
        ("2026-11-01T00:00:00Z", true),
        ("2026-11-01T06:59:00Z", true),
        ("2026-11-01T07:00:00Z", false),
        ("2026-11-01T12:00:00Z", false),
    ] {
        assert_eq!(in_quiet_hours(ts(at), &settings), inside, "{at}");
    }
    assert_eq!(
        quiet_ends_at(ts("2026-10-31T23:00:00Z"), &settings),
        Some(ts("2026-11-01T07:00:00Z"))
    );
    assert_eq!(
        quiet_ends_at(ts("2026-11-01T03:00:00Z"), &settings),
        Some(ts("2026-11-01T07:00:00Z"))
    );
    assert_eq!(quiet_ends_at(ts("2026-11-01T12:00:00Z"), &settings), None);
    // A window within one day works too.
    let lunch = quiet((12, 0), (13, 0), TimeZone::UTC);
    assert!(in_quiet_hours(ts("2026-11-01T12:30:00Z"), &lunch));
    assert!(!in_quiet_hours(ts("2026-11-01T13:00:00Z"), &lunch));
}

#[test]
fn quiet_hours_across_dst_change() {
    // Europe/Sofia leaves summer time on the last Sunday of October (2026-10-25):
    // 04:00 EEST (UTC+3) becomes 03:00 EET (UTC+2).
    let settings = quiet((22, 0), (7, 0), sofia());
    // 23:00 EEST on Saturday; quiet hours end at 07:00 EET on Sunday, 05:00 UTC.
    assert_eq!(
        quiet_ends_at(ts("2026-10-24T20:00:00Z"), &settings),
        Some(ts("2026-10-25T05:00:00Z"))
    );
    assert!(
        in_quiet_hours(ts("2026-10-25T04:59:00Z"), &settings),
        "06:59 EET"
    );
    assert!(
        !in_quiet_hours(ts("2026-10-25T05:00:00Z"), &settings),
        "07:00 EET"
    );
    // The night before, still in summer time, they end at 04:00 UTC.
    assert_eq!(
        quiet_ends_at(ts("2026-10-23T20:00:00Z"), &settings),
        Some(ts("2026-10-24T04:00:00Z"))
    );
    // A local end time that happens twice uses the first one: 03:30 EEST, 00:30 UTC.
    let early = quiet((1, 0), (3, 30), sofia());
    assert_eq!(
        quiet_ends_at(ts("2026-10-24T22:30:00Z"), &early),
        Some(ts("2026-10-25T00:30:00Z"))
    );
}

#[test]
fn suppressed_events_never_notify() {
    let at = ts("2026-10-31T12:00:00Z");
    let incident = open(Severity::Critical, at);
    let settings = NotifySettings::new(TimeZone::UTC);
    for kind in [
        EventKind::IncidentOpened,
        EventKind::IncidentReopened,
        folded(3),
        EventKind::IncidentResolved,
        EventKind::IncidentEscalated {
            from: Severity::Warning,
            to: Severity::Critical,
        },
        EventKind::IncidentAcked { by: UserId::new() },
    ] {
        let mut quiet_event = event(kind.clone(), &incident);
        quiet_event.notify = false;
        let out = plan(&quiet_event, &posted(at), &settings, at);
        assert!(out.actions.is_empty(), "{kind:?}");
        assert_eq!(out.state, posted(at), "{kind:?}");
    }
}

#[test]
fn resolution_updates_card_and_replies() {
    let start = ts("2026-10-31T12:00:00Z");
    let mut incident = open(Severity::Warning, start);
    let later = start + SignedDuration::from_mins(42);
    resolve(&mut incident, later);
    let settings = NotifySettings::new(TimeZone::UTC);
    let out = plan(
        &event(EventKind::IncidentResolved, &incident),
        &posted(start),
        &settings,
        later,
    );
    assert_eq!(
        out.actions,
        [
            NotifyAction::ThreadReply {
                incident: incident.id(),
                text: "Resolved after 42m".to_owned()
            },
            NotifyAction::UpdateCard {
                incident: incident.id()
            },
        ]
    );
}

#[test]
fn escalation_replies_and_updates_and_breaks_through_quiet_hours() {
    let night = ts("2026-10-31T23:30:00Z");
    let incident = open(Severity::Warning, night);
    let to_critical = EventKind::IncidentEscalated {
        from: Severity::Warning,
        to: Severity::Critical,
    };
    let day = NotifySettings::new(TimeZone::UTC);
    let out = plan(
        &event(to_critical.clone(), &incident),
        &posted(night),
        &day,
        night,
    );
    assert_eq!(
        replies(&out.actions),
        ["Escalated to critical: plex restarted 7 times in 10 minutes"]
    );
    assert!(out.actions.contains(&NotifyAction::UpdateCard {
        incident: incident.id()
    }));

    let settings = quiet((22, 0), (7, 0), TimeZone::UTC);
    let held = plan(
        &event(EventKind::IncidentOpened, &incident),
        &NotifyState::default(),
        &settings,
        night,
    )
    .state;
    let out = plan(&event(to_critical, &incident), &held, &settings, night);
    assert_eq!(
        out.actions,
        [NotifyAction::PostCard {
            incident: incident.id()
        }],
        "critical does not wait"
    );
    assert!(!out.state.is_held());
}
