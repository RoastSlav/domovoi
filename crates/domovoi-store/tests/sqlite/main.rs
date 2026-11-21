//! The SQLite store: every shared repository contract, and what SQLite guarantees beyond
//! them.

#[cfg(test)]
mod actions;
#[cfg(test)]
mod audit;
#[cfg(test)]
mod audit_chain;
#[cfg(test)]
mod backup;
#[cfg(test)]
mod backup_logging;
#[cfg(test)]
mod commit_cost;
#[cfg(test)]
mod common;
#[cfg(test)]
mod compat;
#[cfg(test)]
mod concurrency;
#[cfg(test)]
mod contracts;
#[cfg(test)]
mod conversations;
#[cfg(test)]
mod durability;
#[cfg(test)]
mod experience;
#[cfg(test)]
mod install;
#[cfg(test)]
mod journal_feed;
#[cfg(test)]
mod knowledge;
#[cfg(test)]
mod metric_samples;
#[cfg(test)]
mod migrations;
#[cfg(test)]
mod operations;
#[cfg(test)]
mod paging;
#[cfg(test)]
mod policy;
#[cfg(test)]
mod purge;
#[cfg(test)]
mod restore;
#[cfg(test)]
mod search;
#[cfg(test)]
mod uow;
#[cfg(test)]
mod upgrade;
