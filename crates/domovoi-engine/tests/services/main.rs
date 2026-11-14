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
mod chat;
#[cfg(test)]
mod conversation;
#[cfg(test)]
mod execute;
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
mod profile;
#[cfg(test)]
mod recovery;
#[cfg(test)]
mod redact;
#[cfg(test)]
mod report;
#[cfg(test)]
mod turns;
#[cfg(test)]
mod uow_probe;
#[cfg(test)]
mod writer;
