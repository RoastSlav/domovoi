//! Integration tests of the `nft` operation group against a small stateful fake of `nft`.
#![cfg(unix)]

#[cfg(test)]
mod checks;
#[cfg(test)]
mod fake;
#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod gates;
#[cfg(test)]
mod journal;
#[cfg(test)]
mod lifecycle;
#[cfg(test)]
mod named;
#[cfg(test)]
mod probe;
#[cfg(test)]
mod selftest;
#[cfg(test)]
mod table;
#[cfg(test)]
mod world;
