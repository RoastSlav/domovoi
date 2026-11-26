//! Reads and writes the fake kernel's sets from a test.

use domovoi_core::containment::{Family, Net};

use super::fake::Elem;
use super::world::World;

/// How many elements `set` holds in the fake's `domovoi` table.
pub fn count(w: &World, set: &str) -> usize {
    let mut nft = w.nft.lock().expect("nft");
    nft.purge(w.now().as_second());
    nft.tables
        .get("domovoi")
        .and_then(|t| t.sets.get(set))
        .map_or(0, |s| s.elems.len())
}

/// The seconds each element of `set` has left.
pub fn left(w: &World, set: &str) -> Vec<i64> {
    let now = w.now().as_second();
    let nft = w.nft.lock().expect("nft");
    nft.tables
        .get("domovoi")
        .and_then(|t| t.sets.get(set))
        .map(|s| {
            s.elems
                .iter()
                .filter_map(|e| e.expires_at.map(|t| t - now))
                .collect()
        })
        .unwrap_or_default()
}

/// Puts an element into `set` as a foreign actor would.
pub fn seed(w: &World, set: &str, text: &str, secs: i64) {
    let now = w.now().as_second();
    let mut nft = w.nft.lock().expect("nft");
    let table = nft.table("domovoi");
    let net = Net::parse(text).expect("net");
    table.sets.get_mut(set).expect("set").elems.push(Elem {
        v6: net.family() == Family::V6,
        lo: net.lo(),
        hi: net.hi(),
        expires_at: Some(now + secs),
    });
}
