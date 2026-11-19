//! Pins the responses of the commands that live behind the hub's services, so that moving
//! their logic out of the transports cannot change a status, a body, a chat sentence or an
//! audit row. The snapshots were written before the move and must not change with it.

use domovoi_core::ports::{AuditFilter, AuditLog, Notifier, OperationRepo, OutboundMessage, Page};
use domovoi_core::{CardButton, InteractionKind, InteractionReply, Risk};
use domovoi_testkit::RecordingNotifier;
use serde_json::{Value, json};
use std::sync::Arc;

use crate::api_harness::{Api, Reply, api, api_with_notifier, api_with_triage_and_operations};
use crate::harness::pump;
use crate::interactions::{ALLOWED, approve, chat, interaction, press, proposed};

/// Replaces every UUID and every 64-character hex string by `<id-N>` in order of first
/// appearance, so a snapshot does not depend on generated ids.
fn normalise_text(text: &str, seen: &mut Vec<String>) -> String {
    let bytes = text.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let uuid = text.get(i..i + 36).filter(|s| is_uuid(s));
        let hash = text
            .get(i..i + 64)
            .filter(|s| s.bytes().all(|b| b.is_ascii_hexdigit()));
        if let Some(found) = uuid.or(hash) {
            let at = seen.iter().position(|s| s == found).unwrap_or_else(|| {
                seen.push(found.to_owned());
                seen.len() - 1
            });
            out.push_str(&format!("<id-{}>", at + 1));
            i += found.len();
        } else {
            let ch = text[i..].chars().next().unwrap_or(' ');
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

fn is_uuid(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(n, b)| match n {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
}

fn normalise(value: &Value, seen: &mut Vec<String>) -> Value {
    match value {
        Value::String(text) => Value::String(normalise_text(text, seen)),
        Value::Array(items) => Value::Array(items.iter().map(|v| normalise(v, seen)).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), normalise(v, seen)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// `(status, body)` of each reply, normalised together.
fn replies(all: &[Reply]) -> Value {
    let mut seen = Vec::new();
    Value::Array(
        all.iter()
            .map(|r| json!({ "status": r.status.as_u16(), "body": normalise(&r.body, &mut seen) }))
            .collect(),
    )
}

#[tokio::test]
async fn incident_commands_answer_as_before() {
    let a = api().await;
    let login = a.admin().await;
    let id = a.incident.id();
    let base = format!("/api/v1/incidents/{id}");
    let unknown = "/api/v1/incidents/0190f2a8-0000-7000-8000-000000000000";
    let long = "q".repeat(1001);
    let mut all = Vec::new();
    all.push(a.post(&format!("{unknown}/ack"), &login, json!({})).await);
    all.push(
        a.post(&format!("{base}/mute"), &login, json!({ "minutes": 0 }))
            .await,
    );
    all.push(
        a.post(&format!("{base}/mute"), &login, json!({ "minutes": 10081 }))
            .await,
    );
    all.push(a.post(&format!("{base}/unmute"), &login, json!({})).await);
    all.push(a.post(&format!("{base}/ack"), &login, json!({})).await);
    all.push(
        a.post(&format!("{base}/mute"), &login, json!({ "minutes": 60 }))
            .await,
    );
    all.push(a.post(&format!("{base}/unmute"), &login, json!({})).await);
    all.push(
        a.post(&format!("{base}/ask"), &login, json!({ "question": "  " }))
            .await,
    );
    all.push(
        a.post(&format!("{base}/ask"), &login, json!({ "question": long }))
            .await,
    );
    all.push(
        a.post(
            &format!("{base}/ask"),
            &login,
            json!({ "question": "why?" }),
        )
        .await,
    );
    insta::assert_json_snapshot!("incident_commands", replies(&all));
}

#[tokio::test]
async fn retry_triage_answers_as_before() {
    let a = api_with_triage_and_operations().await;
    let login = a.admin().await;
    a.llm.push_text(
        r#"{"headline":"disk filling","summary":"s","cause":"c","evidence":[],"verdict":"ACTION_NEEDED","next_step":"n","proposals":[],"details":"d"}"#,
    );
    let uri = format!("/api/v1/incidents/{}/triage", a.incident.id());
    let step = jiff::SignedDuration::from_millis(100);
    // The investigation's effects run against the fake clock, so it moves while a route waits.
    let first = pump(&a.clock, step, a.post(&uri, &login, json!({}))).await;
    // The outcome reaches the caller before the operation closes; let it close first.
    for _ in 0..2000 {
        let open = OperationRepo::for_incident(&*a.store, a.incident.id())
            .await
            .unwrap()
            .iter()
            .any(|op| !op.status.is_terminal());
        if !open {
            break;
        }
        a.clock.advance(jiff::SignedDuration::from_secs(1));
        for _ in 0..20 {
            tokio::task::yield_now().await;
        }
    }
    let cached = pump(
        &a.clock,
        step,
        a.post(&format!("{uri}?fresh=false"), &login, json!({})),
    )
    .await;
    let off = api().await;
    let off_login = off.admin().await;
    let refused = off
        .post(
            &format!("/api/v1/incidents/{}/triage", off.incident.id()),
            &off_login,
            json!({}),
        )
        .await;
    insta::assert_json_snapshot!("retry_triage", replies(&[first, cached, refused]));
}

async fn audit_of(a: &Api) -> Value {
    let page = AuditLog::list(&*a.store, &AuditFilter::default(), &Page::first(100))
        .await
        .unwrap();
    let mut seen = Vec::new();
    let mut rows: Vec<Value> = page
        .items
        .iter()
        .map(|e| {
            json!({
                "kind": e.kind,
                "subject": normalise_text(&e.subject, &mut seen),
                "actor": normalise_text(&e.actor, &mut seen),
            })
        })
        .collect();
    rows.sort_by_key(|r| r.to_string());
    Value::Array(rows)
}

#[tokio::test]
async fn action_approval_answers_as_before() {
    let a = api().await;
    let login = a.admin().await;
    let write = a.propose(Risk::Write).await;
    let destructive = a.propose(Risk::Destructive).await;
    let unknown = "/api/v1/actions/0190f2a8-0000-7000-8000-000000000000";
    let hash = |x: &domovoi_core::ActionRequest| x.plan_hash_for_approval().to_string();
    let url =
        |x: &domovoi_core::ActionRequest, verb: &str| format!("/api/v1/actions/{}/{verb}", x.id());
    let all = vec![
        a.post(
            &format!("{unknown}/approve"),
            &login,
            json!({ "plan_hash": "0".repeat(64) }),
        )
        .await,
        a.post(&url(&write, "approve"), &login, json!({})).await,
        a.post(
            &url(&write, "approve"),
            &login,
            json!({ "plan_hash": "ab".repeat(32) }),
        )
        .await,
        a.post(
            &url(&write, "approve"),
            &login,
            json!({ "plan_hash": hash(&write) }),
        )
        .await,
        a.post(&url(&write, "deny"), &login, json!({})).await,
        a.post(&url(&destructive, "deny"), &login, json!({})).await,
        a.post(&url(&destructive, "deny"), &login, json!({})).await,
    ];
    let other = a.propose(Risk::Destructive).await;
    let mut all = all;
    all.push(
        a.post(
            &url(&other, "approve"),
            &login,
            json!({ "plan_hash": hash(&other) }),
        )
        .await,
    );
    all.push(
        a.post(
            &url(&other, "approve"),
            &login,
            json!({ "plan_hash": hash(&other), "confirm_text": "wrong" }),
        )
        .await,
    );
    all.push(
        a.post(
            &url(&other, "approve"),
            &login,
            json!({ "plan_hash": hash(&other), "confirm_text": other.skill().to_string() }),
        )
        .await,
    );
    insta::assert_json_snapshot!(
        "action_approval",
        json!({ "replies": replies(&all), "audit": audit_of(&a).await })
    );
}

#[tokio::test]
async fn host_and_digest_reads_answer_as_before() {
    let a = api().await;
    let login = a.admin().await;
    let all = vec![
        a.get("/api/v1/hosts", &login).await,
        a.get("/api/v1/hosts/nas", &login).await,
        a.get("/api/v1/digest/preview", &login).await,
    ];
    insta::assert_json_snapshot!("host_and_digest", replies(&all));
}

fn text_of(reply: &InteractionReply) -> String {
    match reply {
        InteractionReply::Accepted { message } => {
            format!("accepted: {}", message.clone().unwrap_or_default())
        }
        InteractionReply::Rejected { reason } => format!("rejected: {reason}"),
    }
}

#[tokio::test]
async fn chat_answers_as_before() {
    let notifier = Arc::new(RecordingNotifier::new("webhook"));
    let a = api_with_notifier(notifier.clone() as Arc<dyn Notifier>).await;
    a.admin().await;
    let c = chat(&a).await;
    let (write, write_token) = proposed(&a, Risk::Write).await;
    let (destructive, destructive_token) = proposed(&a, Risk::Destructive).await;
    let (denied, denied_token) = proposed(&a, Risk::Write).await;
    let short = domovoi_chat::discord::ids::short_id(a.incident.id());
    let command = |name: &str, args: &[&str]| InteractionKind::Command {
        name: name.to_owned(),
        args: args.iter().map(|s| (*s).to_owned()).collect(),
    };
    let button = |b: CardButton| InteractionKind::Button {
        button: b,
        incident: a.incident.id(),
    };
    let kinds = vec![
        command("status", &[]),
        command("status", &["nas"]),
        command("status", &["nowhere"]),
        command("incidents", &[]),
        button(CardButton::Details),
        button(CardButton::Ack),
        button(approve(&write, &write_token)),
        button(approve(&write, &write_token)),
        button(approve(&destructive, &destructive_token)),
        button(CardButton::Deny {
            action: denied.id(),
            token: denied_token,
        }),
        InteractionKind::ThreadMessage {
            incident: a.incident.id(),
            text: "what happened?".into(),
        },
        command("mute", &[&short, "4h"]),
        command("digest", &[]),
        button(CardButton::Mute1h),
    ];
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for kind in kinds {
        let reply = c.authorizer.deliver(interaction(&a, ALLOWED, kind)).await;
        let text = text_of(&reply)
            .replace(&a.incident.id().to_string(), "<incident>")
            .replace(&short, "<short>");
        out.push(normalise_text(&text, &mut seen));
    }
    out.push(text_of(
        &c.authorizer
            .deliver(press(&a, "999999999999999999", CardButton::Ack))
            .await,
    ));
    let sent: Vec<OutboundMessage> = notifier.sent();
    let digest_titles: Vec<String> = sent.iter().map(|m| m.title.clone()).collect();
    insta::assert_json_snapshot!(
        "chat",
        json!({ "replies": out, "digests": digest_titles, "audit": chat_audit(&a).await })
    );
}

use domovoi_core::ports::InteractionSink;

async fn chat_audit(a: &Api) -> Value {
    let page = AuditLog::list(&*a.store, &AuditFilter::default(), &Page::first(100))
        .await
        .unwrap();
    let mut seen = Vec::new();
    let mut rows: Vec<Value> = page
        .items
        .iter()
        .filter(|e| e.kind.starts_with("chat.") || e.kind.starts_with("action."))
        .map(|e| {
            json!({
                "kind": e.kind,
                "subject": normalise_text(&e.subject, &mut seen),
                "actor": normalise_text(&e.actor, &mut seen),
            })
        })
        .collect();
    rows.sort_by_key(|r| r.to_string());
    Value::Array(rows)
}

#[tokio::test]
async fn chat_cannot_approve_a_destructive_action() {
    use domovoi_core::ActionStatus;
    let a = api().await;
    a.admin().await;
    let c = chat(&a).await;
    let (action, token) = proposed(&a, Risk::Destructive).await;
    let press = InteractionKind::Button {
        button: approve(&action, &token),
        incident: a.incident.id(),
    };
    let reply = c.authorizer.deliver(interaction(&a, ALLOWED, press)).await;
    assert_eq!(text_of(&reply), "rejected: Approve this one in the web UI");
    let stored = domovoi_core::ports::ActionRepo::get(&*a.store, action.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(*stored.status(), ActionStatus::Proposed);
}
