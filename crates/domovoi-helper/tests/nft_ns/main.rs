//! Namespace tests: a real `nft`, run only with `DOMOVOI_NS_TESTS=1` inside a network namespace
//! (`sudo unshare --net` in CI, `unshare -Urn` rootless). Each test says when it skips.
#![cfg(unix)]

mod basic;
mod load;
mod support;
