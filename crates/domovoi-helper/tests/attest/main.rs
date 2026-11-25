//! Attestation tests that need no Unix facilities: parsers, route observation, generation
//! planning, the state format and the apply-time guard. They run on every platform.

#[cfg(test)]
#[path = "../support/mod.rs"]
mod support;

#[cfg(test)]
mod endpoints;
#[cfg(test)]
mod gates;
#[cfg(test)]
mod guard;
#[cfg(test)]
mod ip_json;
#[cfg(test)]
mod observe_support;
#[cfg(test)]
mod route_state;
#[cfg(test)]
mod state_format;
