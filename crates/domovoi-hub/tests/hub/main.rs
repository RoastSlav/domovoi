//! The hub: the agent server against a scripted agent over real TLS on 127.0.0.1, and the
//! HTTP API over a temporary store, and the embedded web UI.

#[cfg(test)]
mod agent;
#[cfg(test)]
mod api_admin;
#[cfg(test)]
mod api_auth;
#[cfg(test)]
mod api_harness;
#[cfg(test)]
mod api_routes;
#[cfg(test)]
mod enrollment;
#[cfg(test)]
#[path = "../gates.rs"]
mod gates;
#[cfg(test)]
mod harness;
#[cfg(test)]
mod presence;
#[cfg(test)]
mod sessions;
#[cfg(test)]
mod spa;
