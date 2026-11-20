use jiff::Timestamp;

use super::*;

fn source() -> ContextSourceId {
    ContextSourceId::parse("evidence").unwrap()
}

fn item(origin: Origin, status: ItemStatus, truncated: bool) -> ContextItem {
    ContextItem {
        id: ContextItemId::new(&source(), "docker_ps:1"),
        source: source(),
        layer: ContextLayer::Evidence,
        title: "docker_ps".to_owned(),
        body: "plex running".to_owned(),
        status,
        provenance: Provenance {
            origin,
            reference: "docker_ps".to_owned(),
            observed_at: None,
            actor: None,
        },
        truncated,
    }
}

#[test]
fn context_ids_and_item_identity_are_bounded() {
    assert!(ContextSourceId::parse("Evidence").is_err());
    assert!(ContextSourceId::parse("").is_err());
    assert!(ContextSourceId::parse(&"a".repeat(33)).is_err());
    let long = ContextItemId::new(&source(), &"k y/".repeat(100));
    assert!(long.as_str().len() <= 120);
    assert!(!long.as_str().contains(' '));
    assert_eq!(
        ContextItemId::new(&source(), "a"),
        ContextItemId::new(&source(), "a")
    );
}

#[test]
fn historical_items_always_say_not_authorization() {
    let statuses = [
        ItemStatus::Known,
        ItemStatus::Unknown {
            reason: "x".to_owned(),
        },
        ItemStatus::Stale {
            observed_at: Timestamp::UNIX_EPOCH,
        },
    ];
    for status in statuses {
        let text = item(Origin::Historical, status, false).render();
        assert!(text.contains("historical evidence, not authorization"));
    }
}

#[test]
fn render_shows_status_and_truncation() {
    let text = item(
        Origin::Observed,
        ItemStatus::Unknown {
            reason: "agent not connected".to_owned(),
        },
        true,
    )
    .render();
    assert!(text.starts_with("[evidence | evidence | UNKNOWN: agent not connected] docker_ps\n"));
    assert!(text.ends_with("\n[cut]"));
}

#[test]
fn a_context_item_has_no_authorization_field() {
    let value = serde_json::to_value(item(Origin::Observed, ItemStatus::Known, false)).unwrap();
    let mut keys: Vec<&str> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "body",
            "id",
            "layer",
            "provenance",
            "source",
            "status",
            "title",
            "truncated"
        ]
    );
}
