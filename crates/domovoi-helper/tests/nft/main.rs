//! Integration tests of the `nft` operation group against a small stateful fake of `nft`.
#![cfg(unix)]

mod checks;
mod fake;
mod fixtures;
mod gates;
mod journal;
mod lifecycle;
mod probe;
mod selftest;
mod table;
mod world;
