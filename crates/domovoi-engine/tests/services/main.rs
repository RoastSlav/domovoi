//! Triage and actions over a running incident manager, scripted models and a fake agent
//! link.

#[cfg(test)]
mod actions;
#[cfg(test)]
mod actions_harness;
#[cfg(test)]
mod approve;
#[cfg(test)]
mod attach;
#[cfg(test)]
mod execute;
#[cfg(test)]
#[path = "../gates.rs"]
mod gates;
#[cfg(test)]
mod harness;
#[cfg(test)]
mod limits;
#[cfg(test)]
mod redact;
#[cfg(test)]
mod turns;
