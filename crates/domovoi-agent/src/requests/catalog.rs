//! The agent's capability catalogue: which implementation ids it offers, paged by index.
//!
//! Two halves of one list. The built-in half is derived, never written out here: every
//! contract in the built-in table whose execution is `ExecutionPath::Agent` — the ones this
//! agent runs itself, as opposed to the two the hub answers from its own store. Listing the
//! ids by hand would let the table and the catalogue drift apart. The skill half is one
//! `skill.<name>` echo per *loaded* skill and nothing else: the description, parameters and
//! script digests belong to `hello.skills`, so a page carries ids and versions only. A skill
//! that failed a load check is not in `summaries()` and is therefore never listed, which is
//! why the announced hash moves when a skill appears, disappears or is replaced.
//!
//! Every entry's descriptor is `{}`: the hub ignores that field, and an agent that described
//! its own capability would be describing its own authority, which RW-07 rejects.
//!
//! Paging is by index over the sorted map, so the same `(page, page_size)` always answers
//! the same slice and the last page ends with `next_page: None`.

use std::collections::BTreeMap;

use domovoi_core::capability::{ContractId, ContractVersion, ExecutionPath, builtin_contracts};
use domovoi_proto::messages::{CatalogEntry, CatalogPage, GetCatalog};
use domovoi_runner::SkillCatalog;
use serde_json::json;

/// Most entries one page carries, the wire cap.
pub const MAX_PAGE_ENTRIES: u32 = 100;
/// Largest page index whose first entry can be addressed at the widest page. Past this the
/// entry offset cannot be computed, so the handler refuses the request rather than answering
/// an empty page: an empty page ends the hub's walk, and ending a walk that never read the
/// list would let the hub report a catalogue it verified without reading.
pub const MAX_PAGE_INDEX: u32 = u32::MAX / MAX_PAGE_ENTRIES;

/// Every implementation this agent offers, keyed by contract id and ordered by it: the
/// built-in contracts it runs itself, plus one echo per loaded skill.
#[must_use]
pub fn catalog_entries(catalog: &SkillCatalog) -> BTreeMap<ContractId, ContractVersion> {
    builtin_contracts()
        .iter()
        .filter(|contract| contract.execution == ExecutionPath::Agent)
        .map(|contract| (contract.id.clone(), contract.version))
        .chain(
            catalog
                .summaries()
                .iter()
                .map(|summary| {
                    (
                        ContractId::skill(&summary.name),
                        ContractVersion::new(1).ok(),
                    )
                })
                .filter_map(|(id, version)| version.map(|loaded| (id.clone(), loaded))),
        )
        .collect::<BTreeMap<ContractId, ContractVersion>>()
}

/// One page of `entries`: the same slice for the same `(page, page_size)`, empty past the
/// end, and `next_page` only while entries remain.
#[must_use]
pub fn page(entries: &BTreeMap<ContractId, ContractVersion>, query: &GetCatalog) -> CatalogPage {
    let size = query.page_size.clamp(1, MAX_PAGE_ENTRIES);
    let width = usize::try_from(size).unwrap_or(0);
    let start = usize::try_from(query.page.saturating_mul(size)).unwrap_or(usize::MAX);
    let served = entries
        .iter()
        .skip(start)
        .take(width)
        .map(|(contract, version)| CatalogEntry {
            contract: contract.as_str().to_owned(),
            version: version.get(),
            descriptor: json!({}),
        })
        .collect::<Vec<CatalogEntry>>();
    let remaining = entries.len().saturating_sub(start);
    let next_page = if remaining > served.len() {
        Some(query.page.saturating_add(1))
    } else {
        None
    };
    CatalogPage {
        entries: served,
        next_page,
    }
}

#[cfg(test)]
mod tests {
    use super::MAX_PAGE_ENTRIES;
    use super::catalog_entries;
    use super::page;
    use domovoi_core::capability::{ContractId, ContractVersion, catalog_hash};
    use domovoi_proto::messages::GetCatalog;
    use domovoi_runner::SkillCatalog;
    use std::collections::BTreeMap;
    use std::path::Path;
    use tempfile::TempDir;

    /// Writes one skill folder `name` that passes every load check.
    fn install_skill(dir: &Path, name: &str) {
        let folder = dir.join(name);
        std::fs::create_dir_all(&folder).unwrap();
        let manifest = format!(
            "name = \"{name}\"\ndescription = \"Catalogue fixture\"\nrisk = \"read\"\nentry = \"run\"\ntimeout = \"10s\"\nmax_output_bytes = 4096\n"
        );
        std::fs::write(folder.join("skill.toml"), manifest).unwrap();
        std::fs::write(folder.join("run"), "#!/bin/sh\necho ran\n").unwrap();
        make_executable(&folder.join("run"));
    }

    /// Writes one folder that must fail a load check: the manifest names another skill.
    fn install_broken_skill(dir: &Path, name: &str) {
        let folder = dir.join(name);
        std::fs::create_dir_all(&folder).unwrap();
        let manifest =
            "name = \"other_name\"\ndescription = \"Catalogue fixture\"\nrisk = \"read\"\nentry = \"run\"\ntimeout = \"10s\"\nmax_output_bytes = 4096\n".to_string();
        std::fs::write(folder.join("skill.toml"), manifest).unwrap();
        std::fs::write(folder.join("run"), "#!/bin/sh\necho ran\n").unwrap();
        make_executable(&folder.join("run"));
    }

    /// Makes `entry` executable where the platform has a mode: a skill whose entry is not
    /// executable is refused by the loader on Unix.
    fn make_executable(entry: &Path) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mut permissions = std::fs::metadata(entry).unwrap().permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(entry, permissions).unwrap();
        }
        #[cfg(not(unix))]
        {
            let _ = entry;
        }
    }

    /// A skills directory holding `names` valid folders plus one broken one, and the
    /// catalogue read from it; the temporary directory is removed when the test ends.
    fn loaded(names: &[&str]) -> (TempDir, SkillCatalog) {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("skills");
        std::fs::create_dir_all(&dir).unwrap();
        for name in names {
            install_skill(&dir, name);
        }
        install_broken_skill(&dir, "broken");
        let (catalog, _issues) = SkillCatalog::load(&dir);
        (root, catalog)
    }

    /// The contract ids `catalog` offers, as text.
    fn listed(catalog: &SkillCatalog) -> Vec<String> {
        catalog_entries(catalog)
            .keys()
            .map(|id| id.as_str().to_owned())
            .collect::<Vec<String>>()
    }

    /// Whether the catalogue offers a contract id, as text.
    fn offers(catalog: &SkillCatalog, id: &str) -> bool {
        catalog_entries(catalog)
            .iter()
            .any(|(contract, _version)| contract.as_str() == id)
    }

    /// No skills: the fixture directory still gets the one broken folder.
    const NO_SKILLS: [&str; 0] = [];

    #[test]
    fn catalog_lists_exactly_the_eight_agent_builtins() {
        let (root, catalog) = loaded(&NO_SKILLS);
        let ids = listed(&catalog);
        assert_eq!(ids.len(), 8, "an empty skill catalogue answers {ids:?}");
        for name in [
            "container.events",
            "container.inspect",
            "container.list",
            "container.logs",
            "disk.smart_report",
            "host.overview",
            "network.listening_ports",
            "system.journal",
        ] {
            assert!(
                ids.iter().any(|id| id == name),
                "the agent catalogue misses {name}: {ids:?}"
            );
        }
        assert!(
            ids.iter().all(|id| !id.starts_with("skill.")),
            "a catalogue with no skills lists skills: {ids:?}"
        );
        let versions = catalog_entries(&catalog)
            .values()
            .map(|version| version.get())
            .collect::<Vec<u32>>();
        assert!(
            versions.iter().all(|version| *version == 1),
            "the built-ins are listed at {versions:?}, not at version 1"
        );
        assert!(
            !offers(&catalog, "incident.history") && !offers(&catalog, "notes.search"),
            "a hub-executed contract is in the agent catalogue: {ids:?}"
        );
        assert!(
            !offers(&catalog, "run.read_skill"),
            "the skill entry point is listed as a contract: {ids:?}"
        );
        let _ = root;
    }

    #[cfg(unix)]
    #[test]
    fn catalog_lists_every_loaded_skill() {
        let (root, catalog) = loaded(&["beta_skill", "alpha_skill", "gamma_skill"]);
        let size = catalog_entries(&catalog).len();
        assert_eq!(size, 11, "the fixture answers {size} entries not 11");
        for name in ["alpha_skill", "beta_skill", "gamma_skill"] {
            let wanted = &format!("skill.{name}");
            let shown = listed(&catalog);
            assert!(
                offers(&catalog, wanted),
                "the loaded catalogue hides {name}: {shown:?}"
            );
        }
        let shown = listed(&catalog);
        assert!(
            !offers(&catalog, "skill.broken"),
            "a folder that failed a load check is listed: {shown:?}"
        );
        let _ = root;
    }

    #[cfg(not(unix))]
    #[test]
    fn catalog_lists_no_skill_echoes_on_windows() {
        let (root, catalog) = loaded(&["beta_skill"]);
        let size = catalog_entries(&catalog).len();
        assert_eq!(
            size, 8,
            "the Windows fixture answers {size} entries, the 8 built-ins only"
        );
        let shown = listed(&catalog);
        assert!(
            shown.iter().all(|id| !id.starts_with("skill.")),
            "a catalogue on Windows lists skill echoes: {shown:?}"
        );
        let _ = root;
    }

    #[test]
    fn catalog_pages_are_deterministic_and_end_with_none() {
        let (root, catalog) = loaded(&NO_SKILLS);
        let entries = catalog_entries(&catalog);
        let size = entries.len();
        assert_eq!(size, 8, "the fixture has {size} entries");
        let first = page(
            &entries,
            &GetCatalog {
                page: 0,
                page_size: 4,
            },
        );
        let second = page(
            &entries,
            &GetCatalog {
                page: 1,
                page_size: 4,
            },
        );
        let first_count = first.entries.len();
        let second_count = second.entries.len();
        assert_eq!(first_count, 4, "the first page holds {first_count} entries");
        assert_eq!(
            second_count, 4,
            "the second page holds {second_count} entries"
        );
        let first_next = first.next_page;
        let second_next = second.next_page;
        assert_eq!(first_next, Some(1), "the first page answers {first_next:?}");
        assert_eq!(second_next, None, "the second page answers {second_next:?}");
        let again = page(
            &entries,
            &GetCatalog {
                page: 0,
                page_size: 4,
            },
        );
        assert_eq!(
            again.entries, first.entries,
            "the same request answers another slice"
        );
        let _ = root;
    }

    #[test]
    fn catalog_page_beyond_the_end_is_empty_not_an_error() {
        let (root, catalog) = loaded(&NO_SKILLS);
        let entries = catalog_entries(&catalog);
        let beyond = page(
            &entries,
            &GetCatalog {
                page: 9,
                page_size: MAX_PAGE_ENTRIES,
            },
        );
        let beyond_count = beyond.entries.len();
        let beyond_next = beyond.next_page;
        assert_eq!(beyond_count, 0, "page 9 serves {beyond_count} entries");
        assert_eq!(beyond_next, None, "page 9 answers {beyond_next:?}");
        let _ = root;
    }

    #[test]
    fn catalog_page_size_is_clamped() {
        let (root, catalog) = loaded(&NO_SKILLS);
        let entries = catalog_entries(&catalog);
        let zero = page(
            &entries,
            &GetCatalog {
                page: 0,
                page_size: 0,
            },
        );
        let huge = page(
            &entries,
            &GetCatalog {
                page: 0,
                page_size: 1_000,
            },
        );
        let zero_count = zero.entries.len();
        let zero_next = zero.next_page;
        let huge_count = huge.entries.len();
        let huge_next = huge.next_page;
        assert_eq!(
            zero_count, 1,
            "page_size 0 clamps to one entry, serves {zero_count}"
        );
        assert_eq!(zero_next, Some(1), "page_size 0 answers {zero_next:?}");
        let last_single = page(
            &entries,
            &GetCatalog {
                page: 7,
                page_size: 0,
            },
        );
        let last_count = last_single.entries.len();
        let last_next = last_single.next_page;
        assert_eq!(
            last_count, 1,
            "page 7 of a clamped single-entry page serves {last_count}"
        );
        assert_eq!(
            last_next, None,
            "the last single-entry page answers {last_next:?}"
        );
        assert_eq!(
            huge_count, 8,
            "page_size 1000 clamps to the wire cap and serves {huge_count} of 8"
        );
        assert_eq!(huge_next, None, "a clamped page answers {huge_next:?}");
        let _ = root;
    }

    #[test]
    fn catalog_hash_matches_the_served_entries() {
        let (root, catalog) = loaded(&NO_SKILLS);
        let entries = catalog_entries(&catalog);
        let announced = catalog_hash(&entries);
        let announced_len = announced.len();
        assert_eq!(
            announced_len, 64,
            "the hash is {announced_len} characters, not 64"
        );
        let served = page(
            &entries,
            &GetCatalog {
                page: 0,
                page_size: MAX_PAGE_ENTRIES,
            },
        );
        let rebuilt = served
            .entries
            .iter()
            .map(|entry| {
                (
                    ContractId::parse(&entry.contract).unwrap(),
                    ContractVersion::new(entry.version).unwrap(),
                )
            })
            .collect::<BTreeMap<ContractId, ContractVersion>>();
        let rebuilt_again = catalog_hash(&rebuilt);
        assert_eq!(
            rebuilt, entries,
            "serving one page changes the list it covers"
        );
        assert_eq!(
            rebuilt_again, announced,
            "the served pages hash to {rebuilt_again} not {announced}"
        );
        let _ = root;
    }

    #[cfg(unix)]
    #[test]
    fn catalog_hash_matches_the_served_entries_with_skills() {
        let (root, catalog) = loaded(&["prune_logs"]);
        let entries = catalog_entries(&catalog);
        let announced = catalog_hash(&entries);
        let served = page(
            &entries,
            &GetCatalog {
                page: 0,
                page_size: MAX_PAGE_ENTRIES,
            },
        );
        let rebuilt = served
            .entries
            .iter()
            .map(|entry| {
                (
                    ContractId::parse(&entry.contract).unwrap(),
                    ContractVersion::new(entry.version).unwrap(),
                )
            })
            .collect::<BTreeMap<ContractId, ContractVersion>>();
        let rebuilt_again = catalog_hash(&rebuilt);
        assert_eq!(
            rebuilt, entries,
            "serving one page changes the skill-backed list"
        );
        assert_eq!(
            rebuilt_again, announced,
            "the served skill pages hash to {rebuilt_again} not {announced}"
        );
        let _ = root;
    }

    #[cfg(unix)]
    #[test]
    fn catalog_hash_changes_when_a_skill_is_added() {
        let (root, catalog) = loaded(&["prune_logs"]);
        let without = catalog_hash(&catalog_entries(&catalog));
        let (second_root, second) = loaded(&["prune_logs", "extra_skill"]);
        let with = catalog_hash(&catalog_entries(&second));
        assert_ne!(without, with, "adding a skill leaves the hash at {without}");
        let again = catalog_hash(&catalog_entries(&second));
        assert_eq!(
            again, with,
            "the same set hashes twice as {again} and {with}"
        );
        let _ = root;
        let _ = second_root;
    }
}
