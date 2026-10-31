//! A fresh store in a temporary directory.

use domovoi_store::{Store, StoreOptions};
use domovoi_testkit::build::T0;

/// A store and the directory holding it; the directory goes when this is dropped.
pub struct Fresh {
    pub dir: tempfile::TempDir,
    pub store: Store,
}

impl Fresh {
    pub async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db").join("domovoi.db");
        let store = Store::open(&path, StoreOptions::beside(&path, T0))
            .await
            .unwrap();
        Self { dir, store }
    }

    pub fn path(&self) -> std::path::PathBuf {
        self.dir.path().join("db").join("domovoi.db")
    }
}
