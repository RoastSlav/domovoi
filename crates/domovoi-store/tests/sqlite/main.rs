//! The SQLite store: every shared repository contract, and what SQLite guarantees beyond
//! them.

#[cfg(test)]
mod audit;
#[cfg(test)]
mod common;
#[cfg(test)]
mod concurrency;
#[cfg(test)]
mod contracts;
#[cfg(test)]
mod migrations;
#[cfg(test)]
mod paging;
#[cfg(test)]
mod purge;
#[cfg(test)]
mod search;
