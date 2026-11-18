//! Triage and actions over a running incident manager, scripted models and a fake agent
//! link.

#[cfg(test)]
mod action_metrics;
#[cfg(test)]
mod actions;
#[cfg(test)]
mod actions_harness;
#[cfg(test)]
mod approve;
#[cfg(test)]
mod atomic;
#[cfg(test)]
mod attach;
#[cfg(test)]
mod capability;
#[cfg(test)]
mod chat;
#[cfg(test)]
mod conversation;
#[cfg(test)]
mod effects;
#[cfg(test)]
mod effects_triage;
#[cfg(test)]
mod evidence;
#[cfg(test)]
mod execute;
#[cfg(test)]
mod expiry;
#[cfg(test)]
mod freshness;
#[cfg(test)]
#[path = "../gates.rs"]
mod gates;
#[cfg(test)]
mod harness;
#[cfg(test)]
mod limits;
#[cfg(test)]
mod liveness;
#[cfg(test)]
mod plan_binding;
#[cfg(test)]
mod plan_v2;
#[cfg(test)]
mod profile;
#[cfg(test)]
mod reconciliation;
#[cfg(test)]
mod recovery;
#[cfg(test)]
mod redact;
#[cfg(test)]
mod reliability;
#[cfg(test)]
mod report;
#[cfg(test)]
mod targets;
#[cfg(test)]
mod turns;
#[cfg(test)]
mod uow_probe;
#[cfg(test)]
mod writer;
