//! The incident manager: correlation rules, the pure decision function and the task that
//! stores and publishes.

#[cfg(test)]
mod actor;
#[cfg(test)]
mod correlate;
#[cfg(test)]
mod decide;
#[cfg(test)]
mod dismiss;
#[cfg(test)]
mod harness;
#[cfg(test)]
mod journal_parity;
#[cfg(test)]
mod policy;
#[cfg(test)]
mod policy_conditions;
#[cfg(test)]
mod policy_containment;
#[cfg(test)]
mod policy_containment_ceiling;
#[cfg(test)]
mod policy_fixtures;
#[cfg(test)]
mod policy_limits;
#[cfg(test)]
mod policy_properties;
#[cfg(test)]
mod policy_publish;
#[cfg(test)]
mod policy_review;
#[cfg(test)]
mod policy_shadow_codec;
#[cfg(test)]
mod policy_shadow_inputs;
#[cfg(test)]
mod policy_shadow_replay;
#[cfg(test)]
mod policy_shadow_support;
#[cfg(test)]
mod policy_trace;
#[cfg(test)]
mod policy_validation;
#[cfg(test)]
mod properties;
#[cfg(test)]
#[path = "../support/shadow_env.rs"]
mod shadow_env;
