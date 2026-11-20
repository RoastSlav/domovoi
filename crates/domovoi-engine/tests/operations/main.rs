//! The operation aggregate: the pure decision function and its properties.

#[cfg(test)]
mod chat_steps;
#[cfg(test)]
mod decide;
#[cfg(test)]
mod decide_execution;
#[cfg(test)]
mod decide_finish;
#[cfg(test)]
mod decide_turns;
#[cfg(test)]
mod drive;
#[cfg(test)]
mod effects;
#[cfg(test)]
mod execution;
#[cfg(test)]
mod harness;
#[cfg(test)]
mod inv;
#[cfg(test)]
mod inv_drive;
#[cfg(test)]
mod investigate;
#[cfg(test)]
mod investigate_effects;
#[cfg(test)]
mod investigate_limits;
#[cfg(test)]
mod legacy_parity;
#[cfg(test)]
mod meta;
#[cfg(test)]
mod plan;
#[cfg(test)]
mod policy_shadow;
#[cfg(test)]
mod properties;
#[cfg(test)]
mod queued;
#[cfg(test)]
mod restart;
#[cfg(test)]
#[path = "../support/shadow_env.rs"]
mod shadow_env;
#[cfg(test)]
mod sim;
#[cfg(test)]
mod support;
#[cfg(test)]
mod writer;
