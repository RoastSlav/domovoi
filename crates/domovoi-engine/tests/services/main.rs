//! Triage and actions over a running incident manager, scripted models and a fake agent
//! link.

#[cfg(test)]
mod actions;
#[cfg(test)]
mod attach;
#[cfg(test)]
mod capability;
#[cfg(test)]
mod chain;
#[cfg(test)]
mod chat;
#[cfg(test)]
mod context;
#[cfg(test)]
mod conversation;
#[cfg(test)]
mod effects_triage;
#[cfg(test)]
mod evidence;
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
mod meta;
#[cfg(test)]
mod ops_mode;
#[cfg(test)]
mod parity;
#[cfg(test)]
mod parity_limits;
#[cfg(test)]
mod plan_binding;
#[cfg(test)]
mod profile;
#[cfg(test)]
#[path = "../proposal_view.rs"]
mod proposal_view;
#[cfg(test)]
mod redact;
#[cfg(test)]
mod reliability;
#[cfg(test)]
mod report;
#[cfg(test)]
#[cfg(test)]
mod targets;
#[cfg(test)]
mod turns;
