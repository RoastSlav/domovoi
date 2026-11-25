//! Namespace tests: a real `nft`, run only with `DOMOVOI_NS_TESTS=1` inside a network namespace
//! (`sudo unshare --net` in CI, `unshare -Urn` rootless). Each test says when it skips.
#![cfg(unix)]

#[cfg(test)]
mod basic;
#[cfg(test)]
mod load;
#[cfg(test)]
mod support;
