//! The client's address and scheme, honouring `X-Forwarded-*` only from trusted proxies.

use std::net::{IpAddr, SocketAddr};

use axum::http::HeaderMap;

/// The address of the client behind `peer`. `X-Forwarded-For` is read only when `peer` is a
/// trusted proxy; then the right-most address that is not itself trusted wins.
#[must_use]
pub fn client_ip(peer: SocketAddr, headers: &HeaderMap, trusted: &[IpAddr]) -> IpAddr {
    let peer = peer.ip();
    if !trusted.contains(&peer) {
        return peer;
    }
    let Some(forwarded) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) else {
        return peer;
    };
    forwarded
        .split(',')
        .rev()
        .filter_map(|part| part.trim().parse::<IpAddr>().ok())
        .find(|ip| !trusted.contains(ip))
        .unwrap_or(peer)
}
