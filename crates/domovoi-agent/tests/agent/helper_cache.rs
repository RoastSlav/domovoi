//! The agent-side cache, single flight and rate limit of helper reads, on the fake clock.

#![cfg(unix)]

use domovoi_agent::helper_client::{HelperClient, ReadClientError};
use domovoi_core::helper::read::{ContainerRef, ReadToken};
use domovoi_core::helper::{HelperGroup, HelperRequest, HelperResponse};
use jiff::SignedDuration;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::harness::{clock, settle};
use crate::helper_reads::answers;
use crate::helper_stub::{serve, serve_held};

#[tokio::test]
async fn docker_list_is_cached_for_5_seconds_on_the_fake_clock() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("h.sock");
    let stub = serve(&socket, answers());
    let client = HelperClient::new(&socket);
    let clock = clock();

    client.docker_list(&*clock).await.unwrap();
    clock.advance(SignedDuration::from_secs(4));
    client.docker_list(&*clock).await.unwrap();
    assert_eq!(
        stub.count(),
        1,
        "a second list inside 5 s is answered locally"
    );

    clock.advance(SignedDuration::from_secs(1));
    client.docker_list(&*clock).await.unwrap();
    assert_eq!(stub.count(), 2, "the entry is gone after 5 s");
}

#[tokio::test]
async fn cached_reads_use_the_lifetimes_of_the_brief() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("h.sock");
    let stub = serve(&socket, answers());
    let client = HelperClient::new(&socket);
    let clock = clock();
    let sda = domovoi_core::helper::read::DeviceRef::parse("/dev/sda").unwrap();
    let sdb = domovoi_core::helper::read::DeviceRef::parse("/dev/sdb").unwrap();

    client.docker_probe(&*clock).await.unwrap();
    client.net_listeners(&*clock).await.unwrap();
    client.smart_scan(&*clock).await.unwrap();
    client.smart_report(&*clock, &sda).await.unwrap();
    client.smart_report(&*clock, &sdb).await.unwrap();
    assert_eq!(stub.count(), 5, "one report per device");

    clock.advance(SignedDuration::from_secs(29));
    client.docker_probe(&*clock).await.unwrap();
    client.smart_report(&*clock, &sda).await.unwrap();
    client.smart_scan(&*clock).await.unwrap();
    assert_eq!(stub.count(), 5, "all three are still inside their lifetime");
    client.net_listeners(&*clock).await.unwrap();
    assert_eq!(stub.count(), 6, "the listeners entry lasts 5 s");

    clock.advance(SignedDuration::from_secs(2));
    client.docker_probe(&*clock).await.unwrap();
    assert_eq!(stub.count(), 7, "the probe lasts 30 s");
    clock.advance(SignedDuration::from_secs(30));
    client.smart_report(&*clock, &sda).await.unwrap();
    assert_eq!(stub.count(), 8, "a report lasts 60 s");
    client.smart_scan(&*clock).await.unwrap();
    assert_eq!(stub.count(), 8, "a scan lasts 300 s");
    clock.advance(SignedDuration::from_secs(300));
    client.smart_scan(&*clock).await.unwrap();
    assert_eq!(stub.count(), 9);
}

#[tokio::test]
async fn docker_inspect_and_logs_are_never_cached() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("h.sock");
    let stub = serve(&socket, answers());
    let client = HelperClient::new(&socket);
    let clock = clock();
    let web = ContainerRef::parse("web").unwrap();
    for _ in 0..2 {
        client.docker_inspect(&*clock, &web).await.unwrap();
        client.docker_logs(&*clock, &web, 20).await.unwrap();
        client.kernel_log(&*clock, 10, 20).await.unwrap();
    }
    assert_eq!(stub.count(), 6);
}

#[tokio::test]
async fn identical_concurrent_requests_share_one_call() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("h.sock");
    let stub = serve_held(&socket, answers());
    let client = HelperClient::new(&socket);
    let clock = clock();
    let (a, b, c, ()) = tokio::join!(
        client.docker_list(&*clock),
        client.docker_list(&*clock),
        client.docker_list(&*clock),
        async {
            settle().await;
            stub.release(1);
        }
    );
    assert_eq!(a.unwrap().0.len(), 1);
    assert_eq!(b.unwrap().0.len(), 1);
    assert_eq!(c.unwrap().0.len(), 1);
    assert_eq!(stub.count(), 1);
}

#[tokio::test]
async fn rate_limit_refuses_the_21st_request_in_10_seconds_without_sending() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("h.sock");
    let stub = serve(&socket, answers());
    let client = HelperClient::new(&socket);
    let clock = clock();
    let web = ContainerRef::parse("web").unwrap();

    for i in 0..20 {
        client
            .docker_inspect(&*clock, &web)
            .await
            .unwrap_or_else(|e| panic!("request {i}: {e}"));
        clock.advance(SignedDuration::from_millis(100));
    }
    let err = client.docker_inspect(&*clock, &web).await.unwrap_err();
    assert_eq!(err, ReadClientError::RateLimited);
    assert_eq!(stub.count(), 20, "the 21st was not sent");

    // The window slides: once the first request is 10 s old, one more fits.
    clock.advance(SignedDuration::from_secs(8));
    client.docker_inspect(&*clock, &web).await.unwrap();
    assert_eq!(stub.count(), 21);
}

#[tokio::test]
async fn errors_are_not_cached() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("h.sock");
    let failing = Arc::new(AtomicBool::new(true));
    let switch = failing.clone();
    let healthy = answers();
    let stub = serve(
        &socket,
        Arc::new(move |request: &HelperRequest| {
            if switch.load(Ordering::SeqCst) {
                HelperResponse::ReadFailed {
                    group: HelperGroup::DockerRead,
                    token: ReadToken::EngineError,
                    detail: None,
                }
            } else {
                healthy(request)
            }
        }),
    );
    let client = HelperClient::new(&socket);
    let clock = clock();

    let err = client.docker_list(&*clock).await.unwrap_err();
    assert_eq!(err, ReadClientError::Failed(ReadToken::EngineError, None));
    failing.store(false, Ordering::SeqCst);
    assert_eq!(client.docker_list(&*clock).await.unwrap().0.len(), 1);
    assert_eq!(stub.count(), 2, "the failure was not kept");
}
