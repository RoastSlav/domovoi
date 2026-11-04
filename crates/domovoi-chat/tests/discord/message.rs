use domovoi_chat::discord::embed::{MAX_TITLE, message_embed};
use domovoi_core::Severity;
use domovoi_core::ports::OutboundMessage;

fn message(title: &str, severity: Option<Severity>) -> OutboundMessage {
    OutboundMessage {
        title: title.to_owned(),
        markdown: "- all quiet".to_owned(),
        severity,
    }
}

#[test]
fn message_embed_has_the_title_and_severity_colour_but_no_body() {
    let severities = [
        ("none", None),
        ("info", Some(Severity::Info)),
        ("warning", Some(Severity::Warning)),
        ("critical", Some(Severity::Critical)),
    ];
    for (name, severity) in severities {
        let embed = message_embed(&message("Daily digest for 2026-11-01", severity));
        insta::assert_json_snapshot!(format!("message_{name}"), embed);
    }
}

#[test]
fn long_message_title_is_clipped_with_an_ellipsis() {
    let embed = message_embed(&message(&"é".repeat(300), None));
    let title = embed.title.unwrap();
    assert_eq!(title.chars().count(), MAX_TITLE);
    assert!(title.ends_with('…'));
}
