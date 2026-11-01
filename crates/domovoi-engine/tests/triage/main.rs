//! Triage over a running incident manager, scripted models and a fake agent link.

#[cfg(test)]
mod actions;
#[cfg(test)]
mod attach;
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
