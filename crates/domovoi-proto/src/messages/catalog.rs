//! The capability catalogue as the hub reads it.

/// Ask for one page of the agent's capability catalogue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GetCatalog {
    /// Zero-based.
    pub page: u32,
    /// At most 100; a larger value is clamped, not refused.
    pub page_size: u32,
}

/// One page of the catalogue.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CatalogPage {
    /// The entries on this page.
    pub entries: Vec<CatalogEntry>,
    /// The next page, when there is one.
    #[serde(default)]
    pub next_page: Option<u32>,
}

/// One contract in the catalogue.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CatalogEntry {
    /// Contract id, `<family>.<verb>`.
    pub contract: String,
    /// Contract version.
    pub version: u32,
    /// The descriptor object owned by the capability contract.
    pub descriptor: serde_json::Value,
}
