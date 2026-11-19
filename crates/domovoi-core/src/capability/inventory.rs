//! Reading the two agent tool outputs a target resolution needs, and nothing else.
//!
//! `docker_ps` on the agent prints a tab-separated table with a header and no id; the hub takes
//! only column 0. `docker_inspect` returns Docker's own document, whose top-level `Id` is the
//! full container id RW-34 binds. A short id is refused, never expanded, and a document without
//! one proves nothing, so it is an error rather than an empty answer.
//!
//! The parsers are pure and live here so both the hub's inventory and the engine's resolver can
//! read the same output with one implementation.

use serde_json::Value;

use super::{ContainerId, TargetName};

/// One container as the hub can identify it. `id` is the full 64-character Docker id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerRef {
    /// The name an owner writes in a policy rule, as the agent listed it.
    pub name: TargetName,
    /// The container's full id, as its inspect document reported it.
    pub id: ContainerId,
}

/// Why a tool output could not be read as an inventory answer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InventoryError {
    /// The inspect output is not a JSON document at all.
    #[error("the inspect output is not a JSON document")]
    NotJson,
    /// The inspect document has no top-level `Id` string.
    #[error("the inspect output has no top-level Id string")]
    MissingId,
    /// The container id is not the full 64-character lowercase hex id.
    #[error("the container id is not a full 64-character lowercase hex id")]
    InvalidId,
    /// Column 0 of a `docker_ps` row is not a valid target name.
    #[error("the container name is not a valid target name")]
    InvalidName,
}

/// Reads the container names out of a `docker_ps` `ToolOutput.text`.
///
/// The header line is skipped, each further line is split on `\t`, and column 0 becomes a name
/// in output order. A line with no tab carries no name column, so it is skipped and counted,
/// never fatal; a name that does not match the target-name grammar is refused.
pub fn parse_ps(text: &str) -> Result<Vec<TargetName>, InventoryError> {
    let mut names = Vec::new();
    for line in text.lines().skip(1) {
        let Some((name, _rest)) = line.split_once('\t') else {
            // A line with no tab carries no name column; it is not a row to read.
            continue;
        };
        match TargetName::parse(name) {
            Ok(name) => names.push(name),
            Err(_) => return Err(InventoryError::InvalidName),
        }
    }
    Ok(names)
}

/// Reads the full id out of a `docker_inspect` `ToolOutput.text`.
///
/// Only the top-level `Id` string is read; a short id fails the container-id grammar and is
/// refused, so identity is never guessed from a display form.
pub fn parse_inspect_id(text: &str) -> Result<ContainerId, InventoryError> {
    let document: Value = match serde_json::from_str(text) {
        Ok(document) => document,
        Err(_) => return Err(InventoryError::NotJson),
    };
    let Some(Value::String(id)) = document.get("Id") else {
        return Err(InventoryError::MissingId);
    };
    match ContainerId::parse(id) {
        Ok(id) => Ok(id),
        Err(_) => Err(InventoryError::InvalidId),
    }
}

#[cfg(test)]
mod tests {
    use super::{ContainerId, InventoryError, TargetName, parse_inspect_id, parse_ps};

    const ID: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const TABLE: &str = "name\timage\tstate\thealth\trestarts\nplex\tplexinc/pms\tUp 2 hours\thealthy\t0\nNAS_PORTAINER\tportainer/portainer-ce\tUp\thealthy\t1\nno tab on this line\njellyfin\tjellyfin/jellyfin\tUp\thealthy\t0";
    const INSPECT: &str = "{\n  \"Id\": \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\",\n  \"Name\": \"/plex\",\n  \"State\": { \"Running\": true }\n}";

    fn named(text: &str) -> TargetName {
        match TargetName::parse(text) {
            Ok(name) => name,
            Err(err) => panic!("{text} is not a target name: {err}"),
        }
    }

    #[test]
    fn parse_ps_reads_the_agent_table() {
        assert_eq!(
            parse_ps(TABLE),
            Ok(vec![
                named("plex"),
                named("NAS_PORTAINER"),
                named("jellyfin")
            ]),
            "the table did not read as the three listed names in order",
        );
        assert_eq!(
            parse_ps("name\timage"),
            Ok(Vec::new()),
            "a header alone read as a name"
        );
    }

    #[test]
    fn parse_inspect_id_reads_the_full_id() {
        assert_eq!(
            parse_inspect_id(INSPECT),
            Ok(ContainerId::parse(ID).unwrap()),
            "the inspect document did not yield its full id",
        );
        assert_eq!(
            parse_inspect_id("{\"Name\": \"/plex\"}"),
            Err(InventoryError::MissingId),
            "a document without Id read as an id",
        );
        assert_eq!(
            parse_inspect_id("{\"Id\": \"0123456789ab\"}"),
            Err(InventoryError::InvalidId),
            "a short id was expanded",
        );
        assert_eq!(
            parse_inspect_id("not a document"),
            Err(InventoryError::NotJson),
            "plain text read as a document",
        );
    }
}
