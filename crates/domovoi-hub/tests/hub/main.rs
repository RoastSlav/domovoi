//! The hub: the agent server against a scripted agent over real TLS on 127.0.0.1, and the
//! HTTP API over a temporary store, and the embedded web UI.

#[cfg(test)]
mod agent;
#[cfg(test)]
mod api_admin;
#[cfg(test)]
mod api_auth;
#[cfg(test)]
mod api_chat;
#[cfg(test)]
mod api_harness;
#[cfg(test)]
mod api_logs;
#[cfg(test)]
mod api_overview;
#[cfg(test)]
mod api_routes;
#[cfg(test)]
mod api_updates;
#[cfg(test)]
mod atomic_actions;
#[cfg(test)]
mod atomic_incidents;
#[cfg(test)]
mod availability;
#[cfg(test)]
mod crash_sqlite;
#[cfg(test)]
mod csp;
#[cfg(test)]
mod dismiss;
#[cfg(test)]
mod enrollment;
#[cfg(test)]
#[path = "../gates.rs"]
mod gates;
#[cfg(test)]
mod harness;
#[cfg(test)]
mod interactions;
#[cfg(test)]
mod presence;
#[cfg(test)]
mod profile;
#[cfg(test)]
mod rules;
#[cfg(test)]
mod sessions;
#[cfg(test)]
mod settings;
#[cfg(test)]
mod skills;
#[cfg(test)]
mod spa;
#[cfg(test)]
mod sqlite;
#[cfg(test)]
mod upgrade_flow;
