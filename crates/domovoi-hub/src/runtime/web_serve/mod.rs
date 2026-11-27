//! The web port's accept loop, with the limits plain `axum::serve` does not have.
//!
//! Without them one host with a script can take the UI and the API down: a connection that
//! never sends a request, or sends half of one, is kept open for ever, and every one costs a
//! file descriptor. Here a request's headers must arrive within [`HEADER_READ_TIMEOUT`] (an
//! idle keep-alive connection is closed by the same timer, since the next request's headers
//! must arrive within it too), and the number of open connections is capped in total and per
//! source address. These are constants, not settings: they protect the hub from its own
//! descriptor limit, and a value nobody should need to change is not worth a settings key.

#[cfg(test)]
mod tests;

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::{ConnectInfo, Extension};
use domovoi_core::log_gate::{Gate, RateLimitedLog};
use hyper_util::rt::{TokioIo, TokioTimer};
use hyper_util::service::TowerToHyperService;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::agent_server::accept_log::AcceptErrors;
use crate::agent_server::conn_gate::{ConnGate, ConnLimits, ConnPermit, Refused};

/// How long a client may take to send a request's headers, and how long a keep-alive
/// connection may wait for the next request.
pub(crate) const HEADER_READ_TIMEOUT: Duration = Duration::from_secs(10);

/// Open web connections allowed at once: 512 in all, 128 from one address. A browser uses a
/// handful. Behind a reverse proxy every browser has the proxy's address, so the per-address
/// limit applies to the proxy as a whole; it is set well above what one household or team
/// opens.
pub(crate) const WEB_LIMITS: ConnLimits = ConnLimits {
    total: 512,
    per_source: 128,
};

/// How often a full port writes a line.
const LINE_EVERY: Duration = Duration::from_secs(10);

/// The limits one web server runs with.
#[derive(Debug, Clone, Copy)]
pub(crate) struct WebLimits {
    /// Open connections allowed.
    pub(crate) connections: ConnLimits,
    /// The header read and keep-alive idle timeout.
    pub(crate) header_read: Duration,
}

impl Default for WebLimits {
    fn default() -> Self {
        Self {
            connections: WEB_LIMITS,
            header_read: HEADER_READ_TIMEOUT,
        }
    }
}

/// Serves `app` on `listener` until `shutdown` fires: each connection is limited as
/// [`WebLimits`] says and finishes its running request before it closes.
pub(crate) async fn serve(
    listener: TcpListener,
    app: Router,
    shutdown: CancellationToken,
    limits: WebLimits,
) {
    let gate = ConnGate::new(limits.connections);
    let refusals = RateLimitedLog::new(LINE_EVERY);
    let mut errors = AcceptErrors::new("a web connection");
    let mut connections = JoinSet::new();
    loop {
        let accepted = tokio::select! {
            () = shutdown.cancelled() => break,
            accepted = listener.accept() => accepted,
        };
        match accepted {
            Ok((tcp, peer)) => {
                errors.succeeded();
                match gate.try_acquire(peer.ip()) {
                    Ok(permit) => {
                        connections.spawn(connection(
                            tcp,
                            peer,
                            app.clone(),
                            shutdown.clone(),
                            limits.header_read,
                            permit,
                        ));
                    }
                    Err(refused) => {
                        drop(tcp);
                        log_refused(&refusals, peer, &refused, limits.connections);
                    }
                }
            }
            Err(err) => {
                let pause = errors.failed(&err, Instant::now());
                tokio::select! {
                    () = shutdown.cancelled() => break,
                    () = tokio::time::sleep(pause) => {}
                }
            }
        }
        // Finished connections leave the set as they end, so it never grows without bound.
        while connections.try_join_next().is_some() {}
    }
    while connections.join_next().await.is_some() {}
}

/// One connection: HTTP/1 with the header timeout, until it ends or the server stops.
async fn connection(
    tcp: TcpStream,
    peer: SocketAddr,
    app: Router,
    shutdown: CancellationToken,
    header_read: Duration,
    _permit: ConnPermit,
) {
    let service = TowerToHyperService::new(app.layer(Extension(ConnectInfo(peer))));
    let conn = hyper::server::conn::http1::Builder::new()
        .timer(TokioTimer::new())
        .header_read_timeout(header_read)
        .keep_alive(true)
        .serve_connection(TokioIo::new(tcp), service)
        .with_upgrades();
    tokio::pin!(conn);
    let result = tokio::select! {
        result = conn.as_mut() => result,
        () = shutdown.cancelled() => {
            conn.as_mut().graceful_shutdown();
            conn.await
        }
    };
    if let Err(err) = result {
        // A client that vanished or sent garbage is routine; it is not a hub fault.
        tracing::debug!(%peer, %err, "web connection ended with an error");
    }
}

fn log_refused(gate: &RateLimitedLog, peer: SocketAddr, why: &Refused, limits: ConnLimits) {
    if let Gate::Log { suppressed } = gate.hit_now("web") {
        tracing::warn!(
            peer = %peer.ip(),
            open_from_peer = why.from_source,
            open_total = why.in_total,
            limit_per_peer = limits.per_source,
            limit_total = limits.total,
            suppressed,
            "refused a web connection: too many open connections{}; connections that send no complete request within {} seconds are closed, so this clears by itself (check what is connecting to the web port if the UI is unreachable)",
            if why.source_full { " from this address" } else { "" },
            HEADER_READ_TIMEOUT.as_secs(),
        );
    }
}
