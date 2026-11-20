//! Resolution summaries on SQLite: appending field revisions inside the unit of work, keeping
//! the search projection in step, and reading and searching them back.

mod apply;
mod project;
mod read;
mod rows;
mod search;
mod similar;

pub(crate) use apply::apply;
