//! The crash sweep: every incident and action write, crashed at every commit, on the
//! in-memory store, plus the gates that keep the sweep covering every write.

#[cfg(test)]
mod coverage;
#[cfg(test)]
mod memory;
#[cfg(test)]
mod scenarios;
#[cfg(test)]
mod sweep;
#[cfg(test)]
mod world;
