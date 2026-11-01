use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use domovoi_core::ports::{FactRepo, NoteRepo, Presence};
use domovoi_core::{DigestFact, FactValue};
use domovoi_hub::api::notes_import::{MAX_FILE_BYTES, MAX_FILES};
use jiff::SignedDuration;
use serde_json::Value;

use crate::api_harness::{Api, Login, Reply, api};

const BOUNDARY: &str = "domovoi-test-boundary";

/// A `multipart/form-data` body with `files` (name, bytes) and an optional `host`.
fn form(files: &[(&str, &[u8])], host: Option<&str>) -> Vec<u8> {
    let mut body = Vec::new();
    if let Some(host) = host {
        body.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"host\"\r\n\r\n{host}\r\n"
            )
            .as_bytes(),
        );
    }
    for (name, bytes) in files {
        body.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"files\"; filename=\"{name}\"\r\nContent-Type: text/markdown\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    body
}

async fn import(a: &Api, login: &Login, files: &[(&str, &[u8])], host: Option<&str>) -> Reply {
    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/v1/notes/import")
        .header(header::COOKIE, &login.cookie)
        .header("x-csrf-token", &login.csrf)
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(Body::from(form(files, host)))
        .unwrap();
    a.raw(req).await
}

fn titles(notes: &[domovoi_core::Note]) -> Vec<&str> {
    let mut titles: Vec<&str> = notes.iter().map(|n| n.title.as_str()).collect();
    titles.sort_unstable();
    titles
}

#[tokio::test]
async fn notes_import_limits_file_count_and_size() {
    let a = api().await;
    let login = a.admin().await;

    let many: Vec<(String, &[u8])> = (0..=MAX_FILES)
        .map(|i| (format!("n{i}.md"), b"# x".as_slice()))
        .collect();
    let many: Vec<(&str, &[u8])> = many.iter().map(|(n, b)| (n.as_str(), *b)).collect();
    let too_many = import(&a, &login, &many, None).await;
    assert_eq!(
        too_many.status,
        StatusCode::BAD_REQUEST,
        "{}",
        too_many.text
    );
    assert!(too_many.body["message"].as_str().unwrap().contains("200"));
    assert!(
        NoteRepo::list(&*a.store, None).await.unwrap().is_empty(),
        "nothing is imported from a rejected upload"
    );

    let big = vec![b'a'; MAX_FILE_BYTES + 1];
    let at_limit = {
        let mut body = b"# At the limit\n".to_vec();
        body.resize(MAX_FILE_BYTES, b'a');
        body
    };
    let reply = import(
        &a,
        &login,
        &[
            ("big.md", &big),
            ("limit.md", &at_limit),
            ("photo.txt", b"not markdown"),
            ("empty.md", b"  \n"),
            ("bin.md", &[0xff, 0xfe, 0x00]),
        ],
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.body["created"], 1);
    assert_eq!(reply.body["skipped"], 4);
    let reasons: Vec<(&str, &str)> = reply.body["skipped_files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["name"].as_str().unwrap(), s["reason"].as_str().unwrap()))
        .collect();
    assert_eq!(
        reasons,
        [
            ("big.md", "larger than 256 KiB"),
            ("photo.txt", "not a .md file"),
            ("empty.md", "empty"),
            ("bin.md", "not UTF-8 text"),
        ]
    );
    let stored = NoteRepo::list(&*a.store, None).await.unwrap();
    assert_eq!(titles(&stored), ["At the limit"]);
    assert_eq!(stored[0].body_markdown.len(), MAX_FILE_BYTES);
}

#[tokio::test]
async fn notes_import_derives_titles() {
    let a = api().await;
    let login = a.admin().await;
    let files: [(&str, &[u8]); 3] = [
        (
            "runbooks/nas/zfs.md",
            b"Intro line\n# ZFS pools\n\nscrub monthly",
        ),
        ("runbooks/nas/restic-backups.md", b"no heading here"),
        ("docs\\plex.md", b"## Second level only"),
    ];
    let reply = import(&a, &login, &files, Some("nas")).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.body["created"], 3);

    let notes = NoteRepo::list(&*a.store, Some(a.nas.id)).await.unwrap();
    assert_eq!(titles(&notes), ["ZFS pools", "plex", "restic-backups"]);
    assert!(
        NoteRepo::list(&*a.store, None).await.unwrap().is_empty(),
        "scoped to the host"
    );

    let again = import(&a, &login, &files, Some("nas")).await;
    assert_eq!(again.body["created"], 0, "importing twice adds nothing");
    assert_eq!(again.body["skipped"], 3);
    assert_eq!(
        again.body["skipped_files"][0]["reason"],
        "a note titled \"ZFS pools\" already exists"
    );
    let global = import(&a, &login, &files[..1], None).await;
    assert_eq!(
        global.body["created"], 1,
        "the same title may exist globally"
    );

    let unknown = import(&a, &login, &files[..1], Some("nowhere")).await;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND);
}

fn fact(a: &Api, key: &str, value: FactValue, ago_hours: i64) -> DigestFact {
    DigestFact {
        host: a.nas.id,
        key: key.to_owned(),
        label: key.to_owned(),
        value,
        unit: Some("%".to_owned()),
        observed_at: a
            .now()
            .checked_sub(SignedDuration::from_hours(ago_hours))
            .unwrap(),
    }
}

#[tokio::test]
async fn host_detail_shows_latest_facts_of_the_last_day() {
    let a = api().await;
    let login = a.admin().await;
    for f in [
        fact(&a, "zfs.tank.used_pct", FactValue::Number(70.0), 20),
        fact(&a, "zfs.tank.used_pct", FactValue::Number(71.256), 2),
        fact(&a, "os.release", FactValue::Text("Debian 13".to_owned()), 0),
        fact(&a, "old.fact", FactValue::Number(1.0), 25),
    ] {
        FactRepo::record(&*a.store, &f).await.unwrap();
    }
    let detail = a.get("/api/v1/hosts/nas", &login).await;
    assert_eq!(detail.status, StatusCode::OK, "{}", detail.text);
    assert_eq!(detail.body["host"]["name"], "nas");
    let facts: Vec<(&str, &str)> = detail.body["facts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| (f["key"].as_str().unwrap(), f["value"].as_str().unwrap()))
        .collect();
    assert_eq!(
        facts,
        [("os.release", "Debian 13"), ("zfs.tank.used_pct", "71.26")]
    );
    assert_eq!(
        a.get("/api/v1/hosts/nowhere", &login).await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn hosts_report_open_incidents_and_outdated_agents() {
    let a = api().await;
    let login = a.admin().await;
    let nas = |reply: &Reply| -> Value { reply.body[0].clone() };

    let before = a.get("/api/v1/hosts", &login).await;
    assert_eq!(
        nas(&before)["open_incidents"],
        1,
        "the harness's open incident"
    );
    assert_eq!(nas(&before)["outdated"], false, "not connected");

    a.link.set_presence(
        a.nas.id,
        Presence::Connected {
            since: a.now(),
            version: "0.0.0".to_owned(),
        },
    );
    let old = a.get("/api/v1/hosts", &login).await;
    assert_eq!(nas(&old)["version"], "0.0.0");
    assert_eq!(nas(&old)["outdated"], true);

    a.link.set_presence(
        a.nas.id,
        Presence::Connected {
            since: a.now(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
        },
    );
    assert_eq!(
        nas(&a.get("/api/v1/hosts", &login).await)["outdated"],
        false
    );
}
