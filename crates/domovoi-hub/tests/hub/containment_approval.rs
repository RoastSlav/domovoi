//! Approving and releasing a block over the API: the password, the source typed out and, when the
//! helper's coverage cannot be verified, an explicit acknowledgement. What the person is shown on
//! the step is what the helper's verified report says.

use std::collections::BTreeMap;

use axum::http::StatusCode;
use domovoi_core::attest::Cidr;
use domovoi_core::containment::{ContainCall, ContainOutputs, keys};
use domovoi_core::ports::ContainmentWrite;
use domovoi_core::ports::{BlockPhase, BlockRow, Presence, StateWrite, Transaction, UnitOfWork};
use domovoi_core::{
    Actor, AuthorizationKind, CheckKey, ContractId, ExecutionId, ExecutionIntent, JournalEpoch,
    OperationId, OperationKind, PinnedSubject, StepId, TargetKey,
};
use domovoi_engine::operations::{ExecutionRequest, OperationSeed};
use domovoi_testkit::helper_attest::{TestHelper, verified_view};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::api_harness::{Api, Login, PASSWORD, api_with_operations, t0};

const SOURCE: &str = "203.0.113.0/24";

fn block_request(a: &Api) -> ExecutionRequest {
    ExecutionRequest {
        intent: ExecutionIntent {
            contract: ContractId::parse("firewall.block_source").unwrap(),
            contract_version: 1,
            host: a.nas.id,
            check: CheckKey::parse("firewall.block").unwrap(),
            target: Some(TargetKey::parse(&format!("ipnet:{SOURCE}")).unwrap()),
            args: BTreeMap::from([
                (keys::ARG_SOURCE.to_owned(), SOURCE.to_owned()),
                (keys::ARG_TTL_S.to_owned(), "600".to_owned()),
            ]),
            retry_of: None,
        },
        parent: None,
        caused_by: None,
    }
}

/// An operation with one proposed block of `SOURCE`, waiting for a person.
async fn propose(a: &Api) -> (OperationId, StepId) {
    // A block can only be proposed for a network the owner pinned on the operation.
    let seed = OperationSeed {
        kind: OperationKind::Investigate,
        goal: "a host keeps hammering the login".to_owned(),
        pinned: vec![PinnedSubject {
            host: a.nas.id,
            check: CheckKey::parse("firewall.block").unwrap(),
            target: Some(TargetKey::parse(&format!("ipnet:{SOURCE}")).unwrap()),
        }],
        incidents: Vec::new(),
        parent: None,
        budgets: None,
        first_steps: Vec::new(),
        effects: Vec::new(),
        actor: Actor::System("tests".to_owned()),
    };
    let op = a.ops().create(seed).await.unwrap();
    let step = a
        .ops()
        .propose_execution(op, block_request(a))
        .await
        .expect("the block is proposed");
    (op, step)
}

async fn plan_hash(a: &Api, op: OperationId, step: StepId) -> String {
    let stored = a
        .state
        .deps()
        .operation_repo
        .load(op)
        .await
        .unwrap()
        .unwrap();
    stored
        .execution_of(step)
        .unwrap()
        .value
        .plan_hash
        .to_string()
}

fn approve_uri(op: OperationId, step: StepId) -> String {
    format!("/api/v1/operations/{op}/steps/{step}/approve")
}

fn step_uri(op: OperationId, step: StepId) -> String {
    format!("/api/v1/operations/{op}/steps/{step}")
}

fn release_uri(op: OperationId, step: StepId) -> String {
    format!("/api/v1/operations/{op}/steps/{step}/containment/release")
}

async fn approve(
    a: &Api,
    login: &Login,
    op: OperationId,
    step: StepId,
    body: Value,
) -> crate::api_harness::Reply {
    let mut body = body;
    body["plan_hash"] = Value::String(plan_hash(a, op, step).await);
    a.post(&approve_uri(op, step), login, body).await
}

/// The helper's key pinned and its report verified and fresh, covering the sample networks.
fn verify(a: &Api) {
    let helper = TestHelper::new(7);
    a.helper_trust
        .set(a.nas.id, verified_view(&helper, 1, 5, t0()).unwrap());
}

async fn state_of(a: &Api, login: &Login, op: OperationId, step: StepId) -> String {
    let reply = a.get(&format!("/api/v1/operations/{op}"), login).await;
    reply.body["executions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["step"] == step.to_string())
        .unwrap()["state"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn unverified_human_path_needs_password_typed_source_and_acknowledgement() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let (op, step) = propose(&a).await;

    // The step says what the person is about to accept: nothing verifies the helper's coverage.
    let detail = a.get(&step_uri(op, step), &login).await;
    assert_eq!(detail.status, StatusCode::OK, "{}", detail.text);
    assert_eq!(detail.body["containment"]["assurance"], "unverified");
    assert_eq!(detail.body["containment"]["reason"], "key_not_pinned");
    assert_eq!(detail.body["containment"]["source"], SOURCE);

    // The wrong source typed: refused before anything else is asked.
    let wrong_source = approve(
        &a,
        &login,
        op,
        step,
        json!({ "confirm_text": "203.0.113.0/25", "password": PASSWORD, "unverified_acknowledged": true }),
    )
    .await;
    assert_eq!(
        wrong_source.status,
        StatusCode::FORBIDDEN,
        "{}",
        wrong_source.text
    );

    // The source typed, but the unverified coverage not acknowledged.
    let no_ack = approve(
        &a,
        &login,
        op,
        step,
        json!({ "confirm_text": SOURCE, "password": PASSWORD }),
    )
    .await;
    assert_eq!(no_ack.status, StatusCode::CONFLICT, "{}", no_ack.text);
    assert_eq!(no_ack.body["error"], "unverified_not_acknowledged");

    // Acknowledged, but no password.
    let no_password = approve(
        &a,
        &login,
        op,
        step,
        json!({ "confirm_text": SOURCE, "unverified_acknowledged": true }),
    )
    .await;
    assert_eq!(
        no_password.status,
        StatusCode::FORBIDDEN,
        "{}",
        no_password.text
    );

    // A wrong password.
    let wrong_password = approve(
        &a,
        &login,
        op,
        step,
        json!({ "confirm_text": SOURCE, "password": "not the password", "unverified_acknowledged": true }),
    )
    .await;
    assert_eq!(
        wrong_password.status,
        StatusCode::FORBIDDEN,
        "{}",
        wrong_password.text
    );

    // None of the refusals authorized anything.
    assert_eq!(state_of(&a, &login, op, step).await, "awaiting_approval");
    assert!(a.contain_relay.dispatches().is_empty());

    // All three together.
    let approved = approve(
        &a,
        &login,
        op,
        step,
        json!({ "confirm_text": SOURCE, "password": PASSWORD, "unverified_acknowledged": true }),
    )
    .await;
    assert_eq!(approved.status, StatusCode::OK, "{}", approved.text);
    assert_ne!(state_of(&a, &login, op, step).await, "awaiting_approval");
}

#[tokio::test]
async fn verified_containment_still_needs_password_and_typed_source_but_no_acknowledgement() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    verify(&a);
    let (op, step) = propose(&a).await;
    let detail = a.get(&step_uri(op, step), &login).await;
    assert_eq!(
        detail.body["containment"]["assurance"], "verified",
        "{}",
        detail.text
    );
    assert!(
        detail.body["containment"]["unprotected_paths"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let no_password = approve(&a, &login, op, step, json!({ "confirm_text": SOURCE })).await;
    assert_eq!(
        no_password.status,
        StatusCode::FORBIDDEN,
        "{}",
        no_password.text
    );
    let approved = approve(
        &a,
        &login,
        op,
        step,
        json!({ "confirm_text": SOURCE, "password": PASSWORD }),
    )
    .await;
    assert_eq!(approved.status, StatusCode::OK, "{}", approved.text);
}

#[tokio::test]
async fn human_approved_containment_shows_uncovered_paths() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    verify(&a);
    // One of the host's management paths is outside what the verified report covers.
    a.contain_paths
        .set(vec![Cidr::host("198.51.100.9".parse().unwrap())]);
    let (op, step) = propose(&a).await;
    let detail = a.get(&step_uri(op, step), &login).await;
    assert_eq!(detail.status, StatusCode::OK, "{}", detail.text);
    let shown = &detail.body["containment"];
    assert_eq!(shown["assurance"], "unverified");
    assert_eq!(shown["reason"], "path_uncovered");
    assert_eq!(shown["unprotected_paths"], json!(["198.51.100.9/32"]));
    // The person still has to acknowledge it.
    let no_ack = approve(
        &a,
        &login,
        op,
        step,
        json!({ "confirm_text": SOURCE, "password": PASSWORD }),
    )
    .await;
    assert_eq!(no_ack.status, StatusCode::CONFLICT, "{}", no_ack.text);
    assert_eq!(no_ack.body["error"], "unverified_not_acknowledged");
}

fn connect(a: &Api) {
    a.link.set_presence(
        a.nas.id,
        Presence::Connected {
            since: t0(),
            version: "0.1.0".to_owned(),
        },
    );
    a.link.set_journal_epoch(
        a.nas.id,
        JournalEpoch::from_uuid(Uuid::from_u128(a.nas.id.as_uuid().as_u128())),
    );
}

/// Stores the block of `step` as confirmed, the way the driver would have left it.
async fn store_confirmed_block(a: &Api, op: OperationId, step: StepId) {
    let now = a.now();
    let row = BlockRow {
        block_id: ExecutionId::from_step(step),
        host_id: a.nas.id,
        operation_id: op,
        step_id: step,
        source: SOURCE.to_owned(),
        ttl_s: 600,
        plan_hash: plan_hash(a, op, step).await,
        phase: BlockPhase::Confirmed,
        authorization_kind: "human".to_owned(),
        assurance: json!({
            "kind": "unverified",
            "reason": "key_not_pinned",
            "unprotected_paths": [],
        })
        .to_string(),
        confirm_execution: None,
        rollback_execution: None,
        applied_at: Some(now),
        deadline: Some(now + jiff::SignedDuration::from_secs(600)),
        existing_flows: Some("n/a".to_owned()),
        bridge_gap: Some(false),
        failure_token: None,
        failure_message: None,
        created_at: now,
        updated_at: now,
        version: 1,
    };
    a.store
        .commit(Transaction::new().write(StateWrite::Containment(ContainmentWrite::Create(row))))
        .await
        .expect("the block is stored");
}

#[tokio::test]
async fn release_requires_step_up_and_typed_source() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    connect(&a);
    let (op, step) = propose(&a).await;
    store_confirmed_block(&a, op, step).await;
    let uri = release_uri(op, step);

    // The wrong source typed, with the right password.
    let wrong_source = a
        .post(
            &uri,
            &login,
            json!({ "password": PASSWORD, "confirm_source": "203.0.113.0/25" }),
        )
        .await;
    assert_eq!(
        wrong_source.status,
        StatusCode::FORBIDDEN,
        "{}",
        wrong_source.text
    );
    // No source typed at all.
    let no_source = a.post(&uri, &login, json!({ "password": PASSWORD })).await;
    assert_eq!(
        no_source.status,
        StatusCode::FORBIDDEN,
        "{}",
        no_source.text
    );
    // The right source, no password.
    let no_password = a
        .post(&uri, &login, json!({ "confirm_source": SOURCE }))
        .await;
    assert_eq!(
        no_password.status,
        StatusCode::FORBIDDEN,
        "{}",
        no_password.text
    );
    // The right source, a wrong password.
    let wrong_password = a
        .post(
            &uri,
            &login,
            json!({ "password": "nope", "confirm_source": SOURCE }),
        )
        .await;
    assert_eq!(
        wrong_password.status,
        StatusCode::FORBIDDEN,
        "{}",
        wrong_password.text
    );
    // Nothing was asked of the agent by any refusal.
    assert!(a.contain_relay.dispatches().is_empty());

    // Both: the block is released with a human `rollback`.
    a.contain_relay
        .queue_result(Ok(ContainOutputs(BTreeMap::new())));
    let released = a
        .post(
            &uri,
            &login,
            json!({ "password": PASSWORD, "confirm_source": SOURCE }),
        )
        .await;
    assert_eq!(released.status, StatusCode::OK, "{}", released.text);
    assert_eq!(released.body["phase"], "rolled_back");
    let sent = a.contain_relay.dispatches();
    assert_eq!(sent.len(), 1);
    assert!(matches!(sent[0].call(), ContainCall::Rollback(_)));
    assert_eq!(sent[0].authorization(), AuthorizationKind::Human);
    assert_eq!(a.audit_events("containment.released").await.len(), 1);
}
