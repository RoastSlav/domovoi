//! The SQLite store: every shared repository contract, and what SQLite guarantees beyond
//! them.

#[cfg(test)]
mod actions;
#[cfg(test)]
mod audit;
#[cfg(test)]
mod audit_chain;
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
mod durability;
#[cfg(test)]
mod migrations;
#[cfg(test)]
mod paging;
#[cfg(test)]
mod purge;
#[cfg(test)]
mod search;
#[cfg(test)]
mod uow;
#[cfg(test)]
mod upgrade;
