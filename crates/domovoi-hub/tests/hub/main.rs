//! The hub: the agent server against a scripted agent over real TLS on 127.0.0.1, and the
//! HTTP API over a temporary store, and the embedded web UI.

#[cfg(test)]
mod agent;
#[cfg(test)]
mod api_admin;
#[cfg(test)]
mod api_audit;
#[cfg(test)]
mod api_auth;
#[cfg(test)]
mod api_events;
#[cfg(test)]
mod api_harness;
#[cfg(test)]
mod api_logs;
#[cfg(test)]
mod api_operator;
#[cfg(test)]
mod api_operator_live;
#[cfg(test)]
mod api_overview;
#[cfg(test)]
mod api_routes;
#[cfg(test)]
mod api_updates;
#[cfg(test)]
mod atomic_incidents;
#[cfg(test)]
mod autonomy_api;
#[cfg(test)]
mod availability;
#[cfg(test)]
mod capability;
#[cfg(test)]
mod capability_ceiling;
#[cfg(test)]
mod crash_sqlite;
#[cfg(test)]
mod csp;
#[cfg(test)]
mod dismiss;
#[cfg(test)]
mod enrollment;
#[cfg(test)]
mod evidence;
#[cfg(test)]
mod experience_api;
#[cfg(test)]
#[path = "../gates.rs"]
mod gates;
#[cfg(test)]
mod harness;
#[cfg(test)]
mod helper_trust;
#[cfg(test)]
mod interactions;
#[cfg(test)]
mod knowledge_api;
#[cfg(test)]
mod legacy_retirement;
#[cfg(test)]
mod operation_controls;
#[cfg(test)]
mod operation_history;
#[cfg(test)]
mod operations_api;
#[cfg(test)]
mod operations_steps;
#[cfg(test)]
mod ops_harness;
#[cfg(test)]
mod packages;
#[cfg(test)]
mod packages_import;
#[cfg(test)]
mod permit;
#[cfg(test)]
mod plan_binding;
#[cfg(test)]
mod policy_api;
#[cfg(test)]
mod policy_env;
#[cfg(test)]
mod preferences;
#[cfg(test)]
mod presence;
#[cfg(test)]
mod profile;
mod profile_backfill;
#[cfg(test)]
mod readiness;
#[cfg(test)]
mod reconciliation;
#[cfg(test)]
mod rules;
#[cfg(test)]
mod service_parity;
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
mod step_interactions;
#[cfg(test)]
mod upgrade_flow;
