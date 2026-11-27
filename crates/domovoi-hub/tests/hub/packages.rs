//! The package endpoints end to end, in-process over the router: importing a signed archive,
//! planning and creating jobs behind the password and the typed name, retrying and
//! cancelling, and what a host reports.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use domovoi_core::capability::{Availability, ContractId};
use domovoi_core::package::keys;
use domovoi_core::ports::{HostRepo, LinkError, Presence};
use domovoi_core::{HostId, JournalEpoch, PackageCall, PackageOutputs};
use domovoi_testkit::build::host;
use domovoi_testkit::{BuiltPackage, PackageFixture};
use ed25519_dalek::SigningKey;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::api_harness::{
    Api, Login, PASSWORD, Reply, api, api_with_packages, package_owner_key, t0,
};

const BOUNDARY: &str = "domovoi-package-boundary";

type Part<'a> = (Option<&'a str>, Option<&'a str>, &'a [u8]);

fn built(version: &str, key: &SigningKey) -> BuiltPackage {
    PackageFixture::new("web-tools", version)
        .unwrap()
        .sign_with(key)
        .build()
}

fn owned(version: &str) -> BuiltPackage {
    built(version, &package_owner_key())
}

/// The two files of a package as form parts.
fn parts(package: &BuiltPackage) -> Vec<Part<'_>> {
    vec![
        (
            Some("archive"),
            Some("web-tools.tar.zst"),
            package.archive.as_slice(),
        ),
        (
            Some("signature"),
            Some("web-tools.sig.toml"),
            package.sig_toml.as_bytes(),
        ),
    ]
}

fn form(parts: &[Part<'_>]) -> Vec<u8> {
    let mut body = Vec::new();
    for (name, file, bytes) in parts {
        let mut disposition = "form-data".to_owned();
        if let Some(name) = name {
            disposition.push_str(&format!("; name=\"{name}\""));
        }
        if let Some(file) = file {
            disposition.push_str(&format!("; filename=\"{file}\""));
        }
        body.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\nContent-Disposition: {disposition}\r\nContent-Type: application/octet-stream\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    body
}

async fn upload(a: &Api, login: Option<&Login>, parts: &[Part<'_>]) -> Reply {
    let mut req = Request::builder()
        .method(Method::POST)
        .uri("/api/v1/packages/import")
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={BOUNDARY}"),
        );
    if let Some(login) = login {
        req = req
            .header(header::COOKIE, &login.cookie)
            .header("x-csrf-token", &login.csrf);
    }
    a.raw(req.body(Body::from(form(parts))).unwrap()).await
}

/// A packages-enabled hub with a signed-in owner and a connected agent on `nas`.
async fn rig() -> (Api, Login) {
    let a = api_with_packages().await;
    let login = a.admin().await;
    connect(&a, a.nas.id);
    (a, login)
}

fn connect(a: &Api, host: HostId) {
    a.link.set_presence(
        host,
        Presence::Connected {
            since: t0(),
            version: "0.1.0".to_owned(),
        },
    );
    a.link.set_journal_epoch(
        host,
        JournalEpoch::from_uuid(Uuid::from_u128(host.as_uuid().as_u128())),
    );
}

async fn add_host(a: &Api, name: &str) -> HostId {
    let registered = host(name).unwrap();
    a.store.upsert_host(&registered).await.unwrap();
    connect(a, registered.id);
    registered.id
}

async fn import_ok(a: &Api, login: &Login, package: &BuiltPackage) -> Value {
    let reply = upload(a, Some(login), &parts(package)).await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.text);
    reply.body
}

fn install_outputs(package: &Value) -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            keys::OUT_PACKAGE.to_owned(),
            package["name"].as_str().unwrap().to_owned(),
        ),
        (
            keys::OUT_VERSION.to_owned(),
            package["version"].as_str().unwrap().to_owned(),
        ),
        (
            keys::OUT_KEY_ID.to_owned(),
            package["key_id"].as_str().unwrap().to_owned(),
        ),
        (
            keys::OUT_ARCHIVE_SHA256.to_owned(),
            package["archive_sha256"].as_str().unwrap().to_owned(),
        ),
        (keys::OUT_ENABLED.to_owned(), "false".to_owned()),
        (keys::OUT_SKILLS.to_owned(), "status".to_owned()),
        (
            keys::OUT_PRIVILEGE_BOUNDARY.to_owned(),
            "in_effect".to_owned(),
        ),
    ])
}

/// Every install succeeds unless `failing` is set for the host `slow`, which is refused.
fn agent_installs(a: &Api, package: &Value, failing: Option<(HostId, Arc<AtomicBool>)>) {
    let outputs = install_outputs(package);
    a.link.on_capability(move |host, dispatch| {
        if let Some((target, flag)) = &failing
            && *target == host
            && flag.load(Ordering::SeqCst)
        {
            return Err(LinkError::Rejected("rate_limited: slow down".to_owned()));
        }
        match dispatch.call {
            PackageCall::Install { .. } => Ok(PackageOutputs(outputs.clone())),
            _ => Err(LinkError::Rejected("agent_refused: unexpected".to_owned())),
        }
    });
}

/// Keeps every install of the world queued: the host is not known to the registry.
fn hold_installs(a: &Api, host: HostId) {
    a.package_registry.set_availability(
        host,
        &ContractId::parse("agent.package.install").unwrap(),
        Availability::HostUnknown,
    );
}

fn ids(hosts: &[HostId]) -> Value {
    json!(hosts.iter().map(ToString::to_string).collect::<Vec<_>>())
}

async fn plan(a: &Api, login: &Login, hosts: &[HostId], version: &str) -> Value {
    let reply = a
        .post(
            "/api/v1/packages/jobs/plan",
            login,
            json!({ "kind": "install", "name": "web-tools", "version": version, "host_ids": ids(hosts) }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    reply.body
}

fn create_body(plan: &Value, hosts: &[HostId], password: Option<&str>, confirm: &str) -> Value {
    json!({
        "kind": "install",
        "name": "web-tools",
        "version": plan["version"],
        "host_ids": ids(hosts),
        "plan_hash": plan["job_plan_hash"],
        "password": password,
        "confirm_text": confirm,
    })
}

async fn create(
    a: &Api,
    login: &Login,
    plan: &Value,
    hosts: &[HostId],
    password: Option<&str>,
    confirm: &str,
) -> Reply {
    a.post(
        "/api/v1/packages/jobs",
        login,
        create_body(plan, hosts, password, confirm),
    )
    .await
}

async fn create_ok(a: &Api, login: &Login, hosts: &[HostId], version: &str) -> Value {
    let plan = plan(a, login, hosts, version).await;
    let reply = create(a, login, &plan, hosts, Some(PASSWORD), "web-tools").await;
    assert_eq!(reply.status, StatusCode::ACCEPTED, "{}", reply.text);
    reply.body
}

async fn job(a: &Api, login: &Login, id: &Value) -> Value {
    let reply = a
        .get(
            &format!("/api/v1/packages/jobs/{}", id.as_str().unwrap()),
            login,
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    reply.body
}

/// Lets the install service run until the job is no longer running.
async fn settled(a: &Api, login: &Login, id: &Value) -> Value {
    for _ in 0..2_000 {
        let current = job(a, login, id).await;
        if current["status"] != "running" {
            return current;
        }
        for _ in 0..20 {
            tokio::task::yield_now().await;
        }
    }
    panic!("the job never settled: {}", job(a, login, id).await);
}

fn tmp_files(a: &Api) -> usize {
    std::fs::read_dir(a.dir.path().join("packages").join(".tmp"))
        .map(|entries| entries.count())
        .unwrap_or(0)
}

#[tokio::test]
async fn import_requires_login_and_csrf() {
    let (a, login) = rig().await;
    let package = owned("1.0.0");
    let anonymous = upload(&a, None, &parts(&package)).await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);

    let mut req = Request::builder()
        .method(Method::POST)
        .uri("/api/v1/packages/import")
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .header(header::COOKIE, &login.cookie);
    req = req.header("x-csrf-token", "wrong");
    let forged = a
        .raw(req.body(Body::from(form(&parts(&package)))).unwrap())
        .await;
    assert_eq!(forged.status, StatusCode::FORBIDDEN, "{}", forged.text);
    let listed = a.get("/api/v1/packages", &login).await;
    assert_eq!(listed.body, json!([]));
    assert_eq!(tmp_files(&a), 0);
}

#[tokio::test]
async fn import_multipart_end_to_end() {
    let (a, login) = rig().await;
    let package = owned("1.0.0");
    let created = import_ok(&a, &login, &package).await;
    assert_eq!(created["name"], "web-tools");
    assert_eq!(created["version"], "1.0.0");
    assert_eq!(
        created["archive_sha256"],
        package.archive_sha256.to_string()
    );
    assert_eq!(created["key_id"], package.key_id.to_string());
    assert_eq!(created["signer_label"], "Owner");
    assert_eq!(created["already_imported"], false);

    // The same pair again, files in the other order, changes nothing.
    let again = upload(
        &a,
        Some(&login),
        &[
            (
                Some("signature"),
                Some("s.sig.toml"),
                package.sig_toml.as_bytes(),
            ),
            (
                Some("archive"),
                Some("a.tar.zst"),
                package.archive.as_slice(),
            ),
        ],
    )
    .await;
    assert_eq!(again.status, StatusCode::OK, "{}", again.text);
    assert_eq!(again.body["already_imported"], true);

    let listed = a.get("/api/v1/packages", &login).await;
    assert_eq!(listed.body.as_array().unwrap().len(), 1);
    let detail = a.get("/api/v1/packages/web-tools/1.0.0", &login).await;
    assert_eq!(detail.status, StatusCode::OK, "{}", detail.text);
    assert_eq!(detail.body["file_status"], "ready");
    assert_eq!(detail.body["signature_toml"], package.sig_toml);
    assert_eq!(detail.body["hosts"], json!([]));
    let status = a.get("/api/v1/packages/status", &login).await;
    assert_eq!(status.body["enabled"], true);
    assert_eq!(status.body["artifact_count"], 1);
    assert_eq!(
        status.body["used_store_bytes"],
        package.archive.len() as u64
    );
    assert_eq!(
        status.body["owner_keys"][0]["id"],
        package.key_id.to_string()
    );
    assert!(
        !status.text.contains("public_key"),
        "the status never carries a key"
    );
    assert_eq!(a.audit_events("package.imported").await.len(), 1);

    // The name and version of another archive are refused, and the refusal is audited.
    let other = PackageFixture::new("web-tools", "1.0.0")
        .unwrap()
        .file("extra.txt", b"x")
        .sign_with(&package_owner_key())
        .build();
    let conflict = upload(&a, Some(&login), &parts(&other)).await;
    assert_eq!(conflict.status, StatusCode::CONFLICT, "{}", conflict.text);
    assert_eq!(conflict.body["error"], "package_conflict");
    assert_eq!(a.audit_events("package.import_rejected").await.len(), 1);

    let gone = a
        .send(
            Method::DELETE,
            "/api/v1/packages/web-tools/1.0.0",
            Some(&login),
            None,
        )
        .await;
    assert_eq!(gone.status, StatusCode::NO_CONTENT, "{}", gone.text);
    let missing = a.get("/api/v1/packages/web-tools/1.0.0", &login).await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert_eq!(missing.body["error"], "no_such_package");
    assert_eq!(tmp_files(&a), 0);
}

#[tokio::test]
async fn import_refusals_carry_stable_codes() {
    let (a, login) = rig().await;
    let stranger = SigningKey::from_bytes(&[9; 32]);
    let unknown = built("1.0.0", &stranger);
    let reply = upload(&a, Some(&login), &parts(&unknown)).await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(reply.body["error"], "unknown_key");

    let tampered = PackageFixture::new("web-tools", "1.0.0")
        .unwrap()
        .sign_with(&package_owner_key())
        .tamper_digest()
        .build();
    let reply = upload(&a, Some(&login), &parts(&tampered)).await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(reply.body["error"], "digest_mismatch");

    let good = owned("1.0.0");
    let reply = upload(
        &a,
        Some(&login),
        &[
            (Some("archive"), Some("a"), b"not zstd at all".as_slice()),
            (Some("signature"), Some("s"), good.sig_toml.as_bytes()),
        ],
    )
    .await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(reply.body["error"], "not_zstd");

    let reply = upload(
        &a,
        Some(&login),
        &[
            (Some("archive"), Some("a"), good.archive.as_slice()),
            (
                Some("signature"),
                Some("s"),
                b"format = 1\nsurprise = true\n".as_slice(),
            ),
        ],
    )
    .await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(reply.body["error"], "signature_file_invalid");

    // Past the archive limit the hub stops reading: first its own cap, then the body limit.
    let mut big = vec![0x28, 0xB5, 0x2F, 0xFD];
    big.resize(80 * 1024, 7);
    let reply = upload(
        &a,
        Some(&login),
        &[
            (Some("archive"), Some("a"), big.as_slice()),
            (Some("signature"), Some("s"), good.sig_toml.as_bytes()),
        ],
    )
    .await;
    assert_eq!(
        reply.status,
        StatusCode::PAYLOAD_TOO_LARGE,
        "{}",
        reply.text
    );
    assert_eq!(reply.body["error"], "too_large");
    big.resize(300 * 1024, 7);
    let reply = upload(
        &a,
        Some(&login),
        &[
            (Some("archive"), Some("a"), big.as_slice()),
            (Some("signature"), Some("s"), good.sig_toml.as_bytes()),
        ],
    )
    .await;
    assert_eq!(
        reply.status,
        StatusCode::PAYLOAD_TOO_LARGE,
        "{}",
        reply.text
    );
    assert_eq!(reply.body["error"], "too_large");

    let listed = a.get("/api/v1/packages", &login).await;
    assert_eq!(listed.body, json!([]));
    assert_eq!(tmp_files(&a), 0, "no partial upload is left behind");
}

#[tokio::test]
async fn import_rejects_second_file_field_and_unnamed_field() {
    let (a, login) = rig().await;
    let package = owned("1.0.0");
    let archive = package.archive.as_slice();
    let signature = package.sig_toml.as_bytes();
    let cases: Vec<(&str, Vec<Part<'_>>)> = vec![
        (
            "a second archive",
            vec![
                (Some("archive"), Some("a"), archive),
                (Some("archive"), Some("b"), archive),
                (Some("signature"), Some("s"), signature),
            ],
        ),
        (
            "a second signature",
            vec![
                (Some("archive"), Some("a"), archive),
                (Some("signature"), Some("s"), signature),
                (Some("signature"), Some("t"), signature),
            ],
        ),
        (
            "an unnamed field",
            vec![
                (Some("archive"), Some("a"), archive),
                (Some("signature"), Some("s"), signature),
                (None, Some("x"), b"stray".as_slice()),
            ],
        ),
        (
            "another named field",
            vec![
                (Some("archive"), Some("a"), archive),
                (Some("signature"), Some("s"), signature),
                (Some("note"), None, b"hi".as_slice()),
            ],
        ),
        ("no signature", vec![(Some("archive"), Some("a"), archive)]),
        (
            "no archive",
            vec![(Some("signature"), Some("s"), signature)],
        ),
    ];
    for (what, form) in cases {
        let reply = upload(&a, Some(&login), &form).await;
        assert_eq!(
            reply.status,
            StatusCode::BAD_REQUEST,
            "{what}: {}",
            reply.text
        );
    }
    let listed = a.get("/api/v1/packages", &login).await;
    assert_eq!(listed.body, json!([]), "nothing was imported");
    assert_eq!(tmp_files(&a), 0, "every staged archive was deleted");
}

#[tokio::test]
async fn status_and_list_work_when_disabled_but_writes_do_not() {
    let a = api().await;
    let login = a.admin().await;
    let status = a.get("/api/v1/packages/status", &login).await;
    assert_eq!(status.status, StatusCode::OK, "{}", status.text);
    assert_eq!(status.body["enabled"], false);
    assert_eq!(status.body["artifact_count"], 0);
    let listed = a.get("/api/v1/packages", &login).await;
    assert_eq!(listed.body, json!([]));
    let jobs = a.get("/api/v1/packages/jobs", &login).await;
    assert_eq!(jobs.body["items"], json!([]));
    let host_packages = a.get("/api/v1/hosts/nas/packages", &login).await;
    assert_eq!(
        host_packages.status,
        StatusCode::OK,
        "{}",
        host_packages.text
    );

    let package = owned("1.0.0");
    let import = upload(&a, Some(&login), &parts(&package)).await;
    assert_eq!(import.status, StatusCode::CONFLICT, "{}", import.text);
    assert_eq!(import.body["error"], "packages_disabled");

    let create = a
        .post(
            "/api/v1/packages/jobs",
            &login,
            json!({
                "kind": "install", "name": "web-tools", "version": "1.0.0",
                "host_ids": [a.nas.id.to_string()], "plan_hash": "ab".repeat(32),
                "password": PASSWORD, "confirm_text": "web-tools",
            }),
        )
        .await;
    assert_eq!(create.status, StatusCode::CONFLICT, "{}", create.text);
    assert_eq!(create.body["error"], "packages_disabled");
    let refresh = a
        .post("/api/v1/hosts/nas/packages/refresh", &login, json!({}))
        .await;
    assert_eq!(refresh.status, StatusCode::CONFLICT);
    assert_eq!(refresh.body["error"], "packages_disabled");
    let delete = a
        .send(
            Method::DELETE,
            "/api/v1/packages/web-tools/1.0.0",
            Some(&login),
            None,
        )
        .await;
    assert_eq!(delete.status, StatusCode::CONFLICT);
    assert_eq!(delete.body["error"], "packages_disabled");
}

#[tokio::test]
async fn create_job_requires_password_and_typed_name() {
    let (a, login) = rig().await;
    let package = import_ok(&a, &login, &owned("1.0.0")).await;
    hold_installs(&a, a.nas.id);
    agent_installs(&a, &package, None);
    let hosts = [a.nas.id];
    let planned = plan(&a, &login, &hosts, "1.0.0").await;
    assert_eq!(planned["targets"].as_array().unwrap().len(), 1);
    assert_eq!(planned["targets"][0]["host_name"], "nas");
    assert_eq!(planned["signer_label"], "Owner");
    assert_eq!(planned["archive_sha256"], package["archive_sha256"]);

    let no_password = create(&a, &login, &planned, &hosts, None, "web-tools").await;
    assert_eq!(
        no_password.status,
        StatusCode::FORBIDDEN,
        "{}",
        no_password.text
    );
    let wrong = create(
        &a,
        &login,
        &planned,
        &hosts,
        Some("not the password"),
        "web-tools",
    )
    .await;
    assert_eq!(wrong.status, StatusCode::FORBIDDEN, "{}", wrong.text);
    let wrong_name = create(&a, &login, &planned, &hosts, Some(PASSWORD), "web-tool").await;
    assert_eq!(
        wrong_name.status,
        StatusCode::FORBIDDEN,
        "{}",
        wrong_name.text
    );
    let jobs = a.get("/api/v1/packages/jobs", &login).await;
    assert_eq!(
        jobs.body["items"],
        json!([]),
        "a refused request creates nothing"
    );
    assert!(a.audit_events("package.install_requested").await.is_empty());

    // Malformed shapes are refused before the password is looked at.
    let none = a
        .post(
            "/api/v1/packages/jobs",
            &login,
            create_body(&planned, &[], Some("wrong"), "web-tools"),
        )
        .await;
    assert_eq!(none.status, StatusCode::BAD_REQUEST, "{}", none.text);
    let twice = [a.nas.id, a.nas.id];
    let dup = create(&a, &login, &planned, &twice, Some("wrong"), "web-tools").await;
    assert_eq!(dup.status, StatusCode::BAD_REQUEST, "{}", dup.text);

    let made = create(&a, &login, &planned, &hosts, Some(PASSWORD), "web-tools").await;
    assert_eq!(made.status, StatusCode::ACCEPTED, "{}", made.text);
    assert_eq!(made.body["status"], "running");
    assert_eq!(made.body["kind"], "install");
    assert_eq!(made.body["targets"][0]["status"], "queued");
    assert_eq!(made.body["targets"][0]["attempt"], 1);
    assert_eq!(
        made.body["targets"][0]["plan_hash"],
        planned["targets"][0]["plan_hash"]
    );
    let requested = a.audit_events("package.install_requested").await;
    assert_eq!(requested.len(), 1);
    assert!(
        !made.text.contains(PASSWORD),
        "a response never echoes the password"
    );
}

#[tokio::test]
async fn create_job_wrong_password_counts_toward_lockout() {
    let (a, login) = rig().await;
    import_ok(&a, &login, &owned("1.0.0")).await;
    hold_installs(&a, a.nas.id);
    let hosts = [a.nas.id];
    let planned = plan(&a, &login, &hosts, "1.0.0").await;
    for attempt in 1..=5 {
        let reply = create(&a, &login, &planned, &hosts, Some("guess"), "web-tools").await;
        assert_eq!(reply.status, StatusCode::FORBIDDEN, "attempt {attempt}");
    }
    let locked = create(&a, &login, &planned, &hosts, Some("guess"), "web-tools").await;
    assert_eq!(
        locked.status,
        StatusCode::TOO_MANY_REQUESTS,
        "{}",
        locked.text
    );
    assert!(locked.headers.contains_key(header::RETRY_AFTER));
    let right = create(&a, &login, &planned, &hosts, Some(PASSWORD), "web-tools").await;
    assert_eq!(
        right.status,
        StatusCode::TOO_MANY_REQUESTS,
        "the right password waits out the lockout too"
    );
    let jobs = a.get("/api/v1/packages/jobs", &login).await;
    assert_eq!(jobs.body["items"], json!([]));
}

#[tokio::test]
async fn create_job_with_stale_plan_hash_is_409() {
    let (a, login) = rig().await;
    import_ok(&a, &login, &owned("1.0.0")).await;
    import_ok(&a, &login, &owned("1.1.0")).await;
    hold_installs(&a, a.nas.id);
    let hosts = [a.nas.id];
    let old = plan(&a, &login, &hosts, "1.0.0").await;
    let current = plan(&a, &login, &hosts, "1.1.0").await;
    assert_ne!(old["job_plan_hash"], current["job_plan_hash"]);
    // The confirmation of one version is not valid for another.
    let reply = create(&a, &login, &old, &hosts, Some(PASSWORD), "web-tools").await;
    assert_eq!(reply.status, StatusCode::ACCEPTED, "{}", reply.text);
    let mut stale = current.clone();
    stale["version"] = old["version"].clone();
    let reply = a
        .post(
            "/api/v1/packages/jobs",
            &login,
            create_body(&stale, &hosts, Some(PASSWORD), "web-tools"),
        )
        .await;
    // The host is busy now, but the plan is checked first.
    assert_eq!(reply.status, StatusCode::CONFLICT, "{}", reply.text);
    assert_eq!(reply.body["error"], "plan_changed");
    let jobs = a.get("/api/v1/packages/jobs", &login).await;
    assert_eq!(jobs.body["items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn create_job_for_busy_host_is_409_and_creates_nothing() {
    let (a, login) = rig().await;
    import_ok(&a, &login, &owned("1.0.0")).await;
    let web = add_host(&a, "web").await;
    hold_installs(&a, a.nas.id);
    let first = create_ok(&a, &login, &[a.nas.id], "1.0.0").await;
    assert_eq!(first["status"], "running");

    let both = [web, a.nas.id];
    let planned = plan(&a, &login, &both, "1.0.0").await;
    let reply = create(&a, &login, &planned, &both, Some(PASSWORD), "web-tools").await;
    assert_eq!(reply.status, StatusCode::CONFLICT, "{}", reply.text);
    assert_eq!(reply.body["error"], "host_busy");
    let jobs = a.get("/api/v1/packages/jobs", &login).await;
    assert_eq!(
        jobs.body["items"].as_array().unwrap().len(),
        1,
        "the job for the free host was not created either"
    );
    assert_eq!(a.audit_events("package.install_requested").await.len(), 1);
}

#[tokio::test]
async fn job_status_is_derived_from_targets() {
    let (a, login) = rig().await;
    let package = import_ok(&a, &login, &owned("1.0.0")).await;
    let web = add_host(&a, "web").await;
    let fail = Arc::new(AtomicBool::new(true));
    agent_installs(&a, &package, Some((web, fail.clone())));
    let hosts = [a.nas.id, web];
    let made = create_ok(&a, &login, &hosts, "1.0.0").await;
    assert_eq!(made["status"], "running");
    let done = settled(&a, &login, &made["id"]).await;
    assert_eq!(done["status"], "partial", "{done}");
    let by_host: BTreeMap<String, &Value> = done["targets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| (t["host_name"].as_str().unwrap().to_owned(), t))
        .collect();
    assert_eq!(by_host["nas"]["status"], "succeeded");
    assert_eq!(by_host["nas"]["privilege_boundary"], "in_effect");
    assert_eq!(by_host["web"]["status"], "failed");
    assert_eq!(by_host["web"]["failure"]["code"], "rate_limited");
    assert_ne!(
        by_host["nas"]["plan_hash"], by_host["web"]["plan_hash"],
        "each host has its own plan hash"
    );
    assert_eq!(done["archive_sha256"], package["archive_sha256"]);

    let listed = a.get("/api/v1/packages/jobs?status=finished", &login).await;
    assert_eq!(listed.body["items"].as_array().unwrap().len(), 1);
    let active = a.get("/api/v1/packages/jobs?status=active", &login).await;
    assert_eq!(active.body["items"], json!([]));
    let by_name = a
        .get("/api/v1/packages/jobs?name=other-tools", &login)
        .await;
    assert_eq!(by_name.body["items"], json!([]));
    let bad = a.get("/api/v1/packages/jobs?status=sideways", &login).await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
    let missing = a
        .get(&format!("/api/v1/packages/jobs/{}", Uuid::now_v7()), &login)
        .await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert_eq!(missing.body["error"], "no_such_job");

    // The host that failed succeeds on retry: the job reads from each host's latest attempt.
    fail.store(false, Ordering::SeqCst);
    let retried = a
        .post(
            &format!(
                "/api/v1/packages/jobs/{}/targets/{web}/retry",
                made["id"].as_str().unwrap()
            ),
            &login,
            json!({
                "plan_hash": by_host["web"]["plan_hash"],
                "password": PASSWORD,
                "confirm_text": "web-tools",
            }),
        )
        .await;
    assert_eq!(retried.status, StatusCode::ACCEPTED, "{}", retried.text);
    let done = settled(&a, &login, &made["id"]).await;
    assert_eq!(done["status"], "succeeded", "{done}");
    assert_eq!(done["targets"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn retry_and_cancel_rules() {
    let (a, login) = rig().await;
    let package = import_ok(&a, &login, &owned("1.0.0")).await;
    let fail = Arc::new(AtomicBool::new(true));
    agent_installs(&a, &package, Some((a.nas.id, fail.clone())));
    let made = create_ok(&a, &login, &[a.nas.id], "1.0.0").await;
    let id = made["id"].as_str().unwrap().to_owned();
    let failed = settled(&a, &login, &made["id"]).await;
    assert_eq!(failed["status"], "failed");
    let hash = failed["targets"][0]["plan_hash"].clone();
    let retry_uri = format!("/api/v1/packages/jobs/{id}/targets/{}/retry", a.nas.id);
    let cancel_uri = format!("/api/v1/packages/jobs/{id}/targets/{}/cancel", a.nas.id);

    // A target that ran cannot be cancelled.
    let cancel = a.post(&cancel_uri, &login, json!({})).await;
    assert_eq!(cancel.status, StatusCode::CONFLICT, "{}", cancel.text);
    assert_eq!(cancel.body["error"], "package_conflict");
    assert!(
        cancel.body["message"]
            .as_str()
            .unwrap()
            .contains("already started")
    );

    // A retry asks for the same proof as a job.
    let body = |password: Option<&str>, confirm: &str| json!({ "plan_hash": hash, "password": password, "confirm_text": confirm });
    for (password, confirm) in [
        (None, "web-tools"),
        (Some("nope"), "web-tools"),
        (Some(PASSWORD), "x"),
    ] {
        let refused = a.post(&retry_uri, &login, body(password, confirm)).await;
        assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.text);
    }
    let stale = a
        .post(
            &retry_uri,
            &login,
            json!({ "plan_hash": "ab".repeat(32), "password": PASSWORD, "confirm_text": "web-tools" }),
        )
        .await;
    assert_eq!(stale.status, StatusCode::CONFLICT, "{}", stale.text);
    assert_eq!(stale.body["error"], "plan_changed");
    let unknown = a
        .post(
            &format!(
                "/api/v1/packages/jobs/{}/targets/{}/retry",
                Uuid::now_v7(),
                a.nas.id
            ),
            &login,
            body(Some(PASSWORD), "web-tools"),
        )
        .await;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND);
    assert_eq!(unknown.body["error"], "no_such_job");

    fail.store(false, Ordering::SeqCst);
    let retried = a
        .post(&retry_uri, &login, body(Some(PASSWORD), "web-tools"))
        .await;
    assert_eq!(retried.status, StatusCode::ACCEPTED, "{}", retried.text);
    let done = settled(&a, &login, &made["id"]).await;
    assert_eq!(done["status"], "succeeded");
    let attempts: Vec<u64> = done["targets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["attempt"].as_u64().unwrap())
        .collect();
    assert_eq!(attempts, vec![1, 2]);

    // A target that succeeded is not retried.
    let again = a
        .post(&retry_uri, &login, body(Some(PASSWORD), "web-tools"))
        .await;
    assert_eq!(again.status, StatusCode::CONFLICT, "{}", again.text);
    assert_eq!(again.body["error"], "package_conflict");

    // A queued target can be cancelled, once, and a job cancelled in full did not succeed.
    let web = add_host(&a, "web").await;
    hold_installs(&a, web);
    let queued = create_ok(&a, &login, &[web], "1.0.0").await;
    let queued_id = queued["id"].as_str().unwrap();
    let cancelled = a
        .post(
            &format!("/api/v1/packages/jobs/{queued_id}/targets/{web}/cancel"),
            &login,
            json!({}),
        )
        .await;
    assert_eq!(cancelled.status, StatusCode::OK, "{}", cancelled.text);
    assert_eq!(cancelled.body["targets"][0]["status"], "cancelled");
    assert_eq!(cancelled.body["status"], "failed");
    let twice = a
        .post(
            &format!("/api/v1/packages/jobs/{queued_id}/targets/{web}/cancel"),
            &login,
            json!({}),
        )
        .await;
    assert_eq!(twice.status, StatusCode::CONFLICT);
    assert_eq!(a.audit_events("package.cancelled").await.len(), 1);
}

fn list_answer(package: &Value) -> BTreeMap<String, String> {
    let entry = json!([{
        "name": package["name"],
        "version": package["version"],
        "key_id": package["key_id"],
        "archive_sha256": package["archive_sha256"],
        "installed_at": "2026-10-31T11:00:00Z",
        "skills": ["status"],
        "status": "active",
        "key_state": "trusted",
        "executable": true,
        "executable_reason": null,
    }]);
    BTreeMap::from([
        (keys::OUT_PACKAGES.to_owned(), entry.to_string()),
        (keys::OUT_COUNT.to_owned(), "1".to_owned()),
        (
            keys::OUT_PRIVILEGE_BOUNDARY.to_owned(),
            "in_effect".to_owned(),
        ),
    ])
}

#[tokio::test]
async fn host_packages_and_refresh() {
    let (a, login) = rig().await;
    let package = import_ok(&a, &login, &owned("1.0.0")).await;
    let empty = a.get("/api/v1/hosts/nas/packages", &login).await;
    assert_eq!(empty.status, StatusCode::OK, "{}", empty.text);
    assert_eq!(empty.body["entries"], json!([]));
    assert_eq!(empty.body["observed_at"], Value::Null);
    let unknown = a.get("/api/v1/hosts/nowhere/packages", &login).await;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND);

    let outputs = list_answer(&package);
    a.link
        .on_capability(move |_, dispatch| match dispatch.call {
            PackageCall::List => Ok(PackageOutputs(outputs.clone())),
            _ => Err(LinkError::Rejected("agent_refused: unexpected".to_owned())),
        });
    let refreshed = a
        .post("/api/v1/hosts/nas/packages/refresh", &login, json!({}))
        .await;
    assert_eq!(refreshed.status, StatusCode::OK, "{}", refreshed.text);
    assert_eq!(refreshed.body["entries"][0]["name"], "web-tools");
    assert_eq!(refreshed.body["entries"][0]["observed_by"], "refresh");
    assert_eq!(refreshed.body["entries"][0]["key_state"], "trusted");
    assert_eq!(
        refreshed.body["entries"][0]["privilege_boundary"],
        "in_effect"
    );
    let stored = a.get("/api/v1/hosts/nas/packages", &login).await;
    assert_eq!(stored.body["entries"], refreshed.body["entries"]);
    let detail = a.get("/api/v1/packages/web-tools/1.0.0", &login).await;
    assert_eq!(detail.body["hosts"][0]["host_name"], "nas");
    assert_eq!(detail.body["hosts"][0]["status"], "active");
    assert_eq!(a.audit_events("package.inventory_refreshed").await.len(), 1);

    // An agent that is not connected answers the way the log endpoint does.
    a.link.set_presence(
        a.nas.id,
        Presence::Disconnected {
            last_seen: Some(t0()),
        },
    );
    let offline = a
        .post("/api/v1/hosts/nas/packages/refresh", &login, json!({}))
        .await;
    assert_eq!(offline.status, StatusCode::CONFLICT, "{}", offline.text);
    assert_eq!(offline.body["error"], "conflict");
}

#[tokio::test]
async fn no_endpoint_accepts_a_client_supplied_approval() {
    let (a, login) = rig().await;
    import_ok(&a, &login, &owned("1.0.0")).await;
    hold_installs(&a, a.nas.id);
    let hosts = [a.nas.id];
    let planned = plan(&a, &login, &hosts, "1.0.0").await;
    let mut smuggled = create_body(&planned, &hosts, Some("wrong"), "web-tools");
    smuggled["approval"] = json!({
        "user": "00000000-0000-0000-0000-000000000000",
        "channel": { "type": "web", "typed_confirmation": true },
    });
    let reply = a.post("/api/v1/packages/jobs", &login, smuggled).await;
    assert!(
        reply.status.is_client_error() && reply.status != StatusCode::FORBIDDEN,
        "an unknown field is refused as malformed before any password check: {} {}",
        reply.status,
        reply.text
    );
    let mut retry =
        json!({ "plan_hash": "ab".repeat(32), "password": PASSWORD, "confirm_text": "x" });
    retry["authorization"] = json!({ "kind": "human" });
    let reply = a
        .post(
            &format!(
                "/api/v1/packages/jobs/{}/targets/{}/retry",
                Uuid::now_v7(),
                a.nas.id
            ),
            &login,
            retry,
        )
        .await;
    assert!(reply.status.is_client_error(), "{}", reply.text);

    let spec: Value = serde_json::from_str(&domovoi_hub::router::openapi_json()).unwrap();
    for (path, item) in spec["paths"].as_object().unwrap() {
        if !path.contains("/packages") {
            continue;
        }
        let text = item.to_string().to_lowercase();
        for word in ["approval", "authorization", "authorisation", "approved_by"] {
            assert!(!text.contains(word), "{path} mentions {word}");
        }
    }
    for name in ["PlanRequestDto", "CreateJobRequestDto", "RetryRequestDto"] {
        let schema = &spec["components"]["schemas"][name];
        assert_eq!(
            schema["additionalProperties"], false,
            "{name} takes no extra field"
        );
    }
    assert!(
        a.get("/api/v1/packages/jobs", &login).await.body["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn openapi_lists_the_package_routes() {
    let spec: Value = serde_json::from_str(&domovoi_hub::router::openapi_json()).unwrap();
    let expected = [
        ("get", "/api/v1/packages/status", "packages_status"),
        ("get", "/api/v1/packages", "list_packages"),
        ("post", "/api/v1/packages/import", "import_package"),
        ("get", "/api/v1/packages/{name}/{version}", "get_package"),
        (
            "delete",
            "/api/v1/packages/{name}/{version}",
            "delete_package",
        ),
        ("post", "/api/v1/packages/jobs/plan", "plan_package_job"),
        ("post", "/api/v1/packages/jobs", "create_package_job"),
        ("get", "/api/v1/packages/jobs", "list_package_jobs"),
        ("get", "/api/v1/packages/jobs/{id}", "get_package_job"),
        (
            "post",
            "/api/v1/packages/jobs/{id}/targets/{host_id}/retry",
            "retry_package_target",
        ),
        (
            "post",
            "/api/v1/packages/jobs/{id}/targets/{host_id}/cancel",
            "cancel_package_target",
        ),
        ("get", "/api/v1/hosts/{name}/packages", "list_host_packages"),
        (
            "post",
            "/api/v1/hosts/{name}/packages/refresh",
            "refresh_host_packages",
        ),
    ];
    for (method, path, operation) in expected {
        let found = &spec["paths"][path][method];
        assert_eq!(found["operationId"], operation, "{method} {path}");
        assert_eq!(found["tags"], json!(["packages"]), "{method} {path}");
    }
    let import = &spec["paths"]["/api/v1/packages/import"]["post"];
    assert!(import["requestBody"]["content"]["multipart/form-data"].is_object());
    let tags: Vec<&str> = spec["tags"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert!(tags.contains(&"packages"));
}

fn skills_body(enabled: bool, password: Option<&str>, confirm: &str) -> Value {
    json!({
        "enabled": enabled, "version": "1.0.0", "password": password, "confirm_text": confirm,
    })
}

#[tokio::test]
async fn enabling_a_stored_package_asks_for_password_and_name_and_is_audited() {
    let (a, login) = rig().await;
    let seen: Arc<std::sync::Mutex<Vec<String>>> = Arc::default();
    let log = seen.clone();
    a.link
        .on_capability(move |_host, dispatch| match dispatch.call {
            PackageCall::Enable { .. } => {
                log.lock().unwrap().push("enable".to_owned());
                Ok(PackageOutputs(std::collections::BTreeMap::from([
                    ("enabled".to_owned(), "true".to_owned()),
                    ("changed".to_owned(), "true".to_owned()),
                    ("tree_sha256".to_owned(), "ab".repeat(32)),
                ])))
            }
            PackageCall::Disable { .. } => Err(LinkError::Rejected(
                "tree_changed: the package files differ".to_owned(),
            )),
            _ => Err(LinkError::Rejected("agent_refused: unexpected".to_owned())),
        });
    let path = "/api/v1/hosts/nas/packages/web-tools/skills";
    let put = |body: Value| a.send(Method::PUT, path, Some(&login), Some(body));
    assert_eq!(
        put(skills_body(true, None, "web-tools")).await.status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        put(skills_body(true, Some("wrong"), "web-tools"))
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        put(skills_body(true, Some(PASSWORD), "web-tool"))
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    assert!(seen.lock().unwrap().is_empty(), "nothing reached the agent");
    for (enable, word) in [(true, "enabling"), (false, "disabling")] {
        let answer = put(skills_body(enable, None, "web-tools")).await;
        assert!(
            answer
                .text
                .contains(&format!("{word} a package's skills needs your password")),
            "{}",
            answer.text
        );
    }
    assert!(a.audit_events("package.enabled").await.is_empty());

    let ok = put(skills_body(true, Some(PASSWORD), "web-tools")).await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.text);
    assert_eq!(ok.body["enabled"], true);
    assert_eq!(ok.body["plan_hash"].as_str().unwrap().len(), 64);
    let rows = a.audit_events("package.enabled").await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].detail["plan_hash"], ok.body["plan_hash"]);
    assert_eq!(rows[0].detail["package"], "web-tools");

    let refused = put(skills_body(false, Some(PASSWORD), "web-tools")).await;
    assert_eq!(refused.status, StatusCode::CONFLICT, "{}", refused.text);
    assert!(
        refused
            .text
            .contains("The agent refused: the package files differ"),
        "{}",
        refused.text
    );
    assert!(!refused.text.contains("tree_changed"), "{}", refused.text);
    let audited = a.audit_events("package.enable_refused").await;
    assert_eq!(audited.len(), 1);
    assert!(
        audited[0].detail["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("tree_changed")),
        "the audit log keeps the code"
    );
}
