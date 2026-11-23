//! Integration tests for the self-test, the isolation probe, the platform record and repair.
#![cfg(unix)]

#[cfg(test)]
mod agent_side;
#[cfg(test)]
mod checks;
#[cfg(test)]
mod gates;
#[cfg(test)]
mod probe;
#[cfg(test)]
mod record;
#[cfg(test)]
mod repair;
#[cfg(test)]
mod support;
#[cfg(test)]
mod units;
