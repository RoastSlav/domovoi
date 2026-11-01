//! Local webhook relay over real loopback HTTP and the existing agent link.

use std::net::SocketAddr;
use std::sync::Arc;

use domovoi_agent::relay::{Relay, RelayError};
use domovoi_agent::spool::{SharedSpool, Spool};
use domovoi_config::agent::LocalWebhooks;
use domovoi_config::{Secret, SecretSource};
use domovoi_core::{EvidenceValue, HostId, SourceKind};
use secrecy::SecretString;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::harness::{clock, identity, start_link_with};
use crate::hub::{TestHub, guard};

fn config(bind: &str, token: bool) -> LocalWebhooks {
    LocalWebhooks {
        bind: bind.parse().unwrap(),
        token: token.then(|| {
            Secret::resolved(
                SecretSource::Env("RELAY_TOKEN".to_owned()),
                SecretString::from("local-secret".to_owned()),
            )
        }),
        allow_non_loopback: false,
    }
}

async fn serve(
    relay: Relay,
    spool: Arc<SharedSpool>,
    host: HostId,
) -> (
    SocketAddr,
    CancellationToken,
    JoinHandle<Result<(), RelayError>>,
) {
    let addr = relay.local_addr().unwrap();
    let stop = CancellationToken::new();
    let task = tokio::spawn(relay.serve(spool, host, clock(), stop.clone()));
    (addr, stop, task)
}

async fn post(addr: SocketAddr, token: Option<&str>) -> u16 {
    let body = r#"{"host":"payload-host","check":"backup.nightly","summary":"Backup failed"}"#;
    let auth = token.map_or_else(String::new, |token| {
        format!("Authorization: Bearer {token}\r\n")
    });
    let request = format!(
        "POST /hooks/generic HTTP/1.1\r\nHost: {addr}\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    let response = String::from_utf8(response).unwrap();
    response.split_whitespace().nth(1).unwrap().parse().unwrap()
}

fn nas() -> domovoi_core::HostName {
    domovoi_core::HostName::parse("nas").unwrap()
}

#[tokio::test]
async fn relay_refuses_non_loopback_bind_by_default() {
    let rejected = Relay::bind(&config("0.0.0.0:0", false), nas()).await;
    assert!(matches!(rejected, Err(RelayError::NonLoopback(_))));

    let mut allowed = config("0.0.0.0:0", false);
    allowed.allow_non_loopback = true;
    let relay = Relay::bind(&allowed, nas()).await.unwrap();
    assert_eq!(
        relay.local_addr().unwrap().ip(),
        std::net::IpAddr::from([0, 0, 0, 0])
    );
}

#[tokio::test]
async fn relayed_alert_is_spooled_and_sent_with_agent_host() {
    let mut hub = TestHub::start().await;
    let dir = tempfile::tempdir().unwrap();
    let identity = identity(&hub);
    let clock = clock();
    let spool = Arc::new(SharedSpool::new(Spool::open(dir.path()).unwrap()));
    let link = start_link_with(&hub, &identity, &clock, &dir, spool.clone());
    let mut conn = hub.accept().await;
    conn.welcome(&identity, 10).await;

    let relay = Relay::bind(&config("127.0.0.1:0", true), nas())
        .await
        .unwrap();
    let (addr, stop, task) = serve(relay, spool, identity.host_id).await;
    assert_eq!(post(addr, Some("local-secret")).await, 202);
    let signals = conn.signals().await;
    assert_eq!(signals.signals.len(), 1);
    let signal = &signals.signals[0];
    assert_eq!(signal.host(), identity.host_id);
    assert_eq!(
        signal.source(),
        &SourceKind::Webhook {
            kind: "generic".to_owned()
        }
    );
    assert_eq!(
        signal.evidence().get("reported_host"),
        Some(&EvidenceValue::Text("payload-host".to_owned()))
    );

    stop.cancel();
    guard(task).await.unwrap().unwrap();
    link.shutdown.cancel();
    guard(link.task).await.unwrap().unwrap();
}

#[tokio::test]
async fn relay_requires_token_when_configured() {
    let dir = tempfile::tempdir().unwrap();
    let spool = Arc::new(SharedSpool::new(Spool::open(dir.path()).unwrap()));
    let relay = Relay::bind(&config("127.0.0.1:0", true), nas())
        .await
        .unwrap();
    let (addr, stop, task) = serve(relay, spool.clone(), HostId::new()).await;

    assert_eq!(post(addr, None).await, 401);
    assert_eq!(post(addr, Some("wrong")).await, 401);
    assert!(spool.lock().await.is_empty());
    assert_eq!(post(addr, Some("local-secret")).await, 202);
    assert_eq!(spool.lock().await.len(), 1);

    stop.cancel();
    guard(task).await.unwrap().unwrap();
}
