//! The web port's limits: half-sent and idle connections are closed, the number of open
//! connections is capped, and a normal request still works.

use std::net::SocketAddr;
use std::time::Duration;

use axum::Router;
use axum::extract::ConnectInfo;
use axum::routing::get;
use domovoi_testkit::log_capture::LogCapture;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;

use super::{WebLimits, serve};
use crate::agent_server::conn_gate::ConnLimits;

/// A server with a short header timeout, so a test waits a moment, not ten seconds.
async fn start(
    connections: ConnLimits,
) -> (SocketAddr, CancellationToken, tokio::task::JoinHandle<()>) {
    let app =
        Router::new().route(
            "/",
            get(|ConnectInfo(peer): ConnectInfo<SocketAddr>| async move {
                format!("hello {}", peer.ip())
            }),
        );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let stop = CancellationToken::new();
    let limits = WebLimits {
        connections,
        header_read: Duration::from_millis(200),
    };
    let task = tokio::spawn(serve(listener, app, stop.clone(), limits));
    (addr, stop, task)
}

const ROOMY: ConnLimits = ConnLimits {
    total: 64,
    per_source: 32,
};

/// Whether the server closes `stream` within a few seconds (a read ends with EOF or an error).
async fn closed_by_server(stream: &mut TcpStream) -> bool {
    let mut buf = [0u8; 1024];
    loop {
        match tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf)).await {
            Err(_) => return false,
            Ok(Ok(0) | Err(_)) => return true,
            Ok(Ok(_)) => {}
        }
    }
}

#[tokio::test]
async fn a_connection_that_never_sends_a_request_is_closed() {
    let (addr, stop, task) = start(ROOMY).await;
    let mut idle = TcpStream::connect(addr).await.unwrap();
    assert!(
        closed_by_server(&mut idle).await,
        "an idle connection was kept open"
    );
    stop.cancel();
    task.await.unwrap();
}

#[tokio::test]
async fn a_half_sent_request_is_closed() {
    let (addr, stop, task) = start(ROOMY).await;
    let mut half = TcpStream::connect(addr).await.unwrap();
    half.write_all(b"GET /readyz HTTP/1.1\r\nHost: x\r\n")
        .await
        .unwrap();
    assert!(
        closed_by_server(&mut half).await,
        "a half-sent request was kept open"
    );
    stop.cancel();
    task.await.unwrap();
}

#[tokio::test]
async fn a_keep_alive_connection_is_closed_when_it_goes_quiet_and_requests_still_work() {
    let (addr, stop, task) = start(ROOMY).await;
    let mut conn = TcpStream::connect(addr).await.unwrap();
    conn.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
        .await
        .unwrap();
    let mut buf = [0u8; 512];
    let n = conn.read(&mut buf).await.unwrap();
    let reply = String::from_utf8_lossy(&buf[..n]).into_owned();
    assert!(reply.starts_with("HTTP/1.1 200"), "{reply}");
    assert!(
        reply.contains("hello 127.0.0.1"),
        "the peer address reaches handlers: {reply}"
    );
    assert!(
        closed_by_server(&mut conn).await,
        "a quiet keep-alive connection stayed open"
    );
    stop.cancel();
    task.await.unwrap();
}

#[tokio::test]
async fn too_many_connections_from_one_address_are_refused_and_logged_once() {
    let (logs, _guard) = LogCapture::start();
    let (addr, stop, task) = start(ConnLimits {
        total: 10,
        per_source: 2,
    })
    .await;
    // Two sockets that stay quiet use this address's places.
    let first = TcpStream::connect(addr).await.unwrap();
    let second = TcpStream::connect(addr).await.unwrap();
    // The server takes them in order, so a third connect is judged after both.
    let mut third = TcpStream::connect(addr).await.unwrap();
    let mut fourth = TcpStream::connect(addr).await.unwrap();
    assert!(
        closed_by_server(&mut third).await,
        "the third connection was not refused"
    );
    assert!(
        closed_by_server(&mut fourth).await,
        "the fourth connection was not refused"
    );
    let lines = logs.lines_with("refused a web connection");
    assert_eq!(lines.len(), 1, "one line for two refusals: {}", logs.text());
    for field in [
        "WARN",
        "open_from_peer=2",
        "limit_per_peer=2",
        "limit_total=10",
    ] {
        assert!(lines[0].contains(field), "line lacks {field}: {}", lines[0]);
    }
    drop((first, second));
    stop.cancel();
    task.await.unwrap();
}
