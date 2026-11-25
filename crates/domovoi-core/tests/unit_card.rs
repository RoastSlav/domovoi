//! The approval card text: what is bound, what is not, the residual race and the start-failure
//! risk, exactly as the owner reads them.

use domovoi_core::unit::card::{CardFacts, STALE_CARD, UNTESTED_LINE, render_facts};

#[cfg(test)]
fn facts() -> CardFacts {
    CardFacts {
        id: "example.service".to_owned(),
        fragment_path: "/etc/systemd/system/example.service".to_owned(),
        fragment_sha256: "0123456789abcdef".repeat(4),
        drop_ins: vec![(
            "/etc/systemd/system/example.service.d/10-local.conf".to_owned(),
            "fedcba9876543210".repeat(4),
        )],
        exec_start: "/usr/bin/example-daemon --serve".to_owned(),
        user: "svc".to_owned(),
        working_directory: "/srv/example".to_owned(),
        followers: Vec::new(),
    }
}

#[test]
fn approval_card_text_snapshot() {
    insta::assert_snapshot!("card", render_facts("nas", &facts(), false));
}

#[test]
fn the_untested_platform_line_is_added_and_nothing_else_changes() {
    let plain = render_facts("nas", &facts(), false);
    let untested = render_facts("nas", &facts(), true);
    assert_eq!(untested, format!("{plain}\n{UNTESTED_LINE}"));
    assert!(UNTESTED_LINE.contains("not release-tested"));
}

#[test]
fn the_stale_variant_says_to_reload_by_hand_and_that_domovoi_never_does() {
    assert!(STALE_CARD.contains("systemctl daemon-reload"));
    assert!(STALE_CARD.contains("Domovoi never reloads for you."));
}

#[test]
fn followers_are_listed_and_extra_drop_ins_are_summarised_by_count() {
    let mut with_followers = facts();
    with_followers.followers = vec!["worker.service".to_owned(), "cache.service".to_owned()];
    let card = render_facts("nas", &with_followers, false);
    assert!(card.contains("(worker.service, cache.service)"), "{card}");

    let mut many = facts();
    many.drop_ins = (0..11)
        .map(|n| {
            (
                format!("/etc/systemd/system/example.service.d/{n:02}.conf"),
                "ab".repeat(32),
            )
        })
        .collect();
    let card = render_facts("nas", &many, false);
    assert!(card.contains("and 3 more"), "{card}");
    assert!(card.contains("environment values are hidden"));
}
