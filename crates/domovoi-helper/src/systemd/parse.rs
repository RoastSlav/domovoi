//! Decoding `busctl --json=short` replies into the plain data of `domovoi_core::unit`.
//!
//! Pure functions of the JSON text. Arrays arrive as arrays, so a path with an awkward name is
//! never split. Anything missing or of the wrong type is an error, never a default: a partial
//! read must not become a hash.

use std::collections::{BTreeMap, BTreeSet};

use domovoi_core::unit::{ExecCommand, LoadedProps, UnitProps, UnitRuntime, ViewError};
use serde_json::Value;

/// The properties of one interface, name to value.
pub(super) type Props = BTreeMap<String, Value>;

fn bad(what: &str) -> ViewError {
    ViewError(format!("unexpected busctl reply: {what}"))
}

/// The object path `LoadUnit` answered.
pub(super) fn object_path(reply: &str) -> Result<String, ViewError> {
    let value: Value = serde_json::from_str(reply).map_err(|_| bad("not JSON"))?;
    let path = value
        .get("data")
        .and_then(|d| d.get(0))
        .and_then(Value::as_str)
        .ok_or_else(|| bad("no object path"))?;
    if !path.starts_with("/org/freedesktop/systemd1/unit/") || path.contains("..") {
        return Err(bad("not a unit object path"));
    }
    Ok(path.to_owned())
}

/// The `a{sv}` of a `GetAll` reply, with each variant reduced to its data. A property the
/// manager reports as `null` (a type busctl cannot render) is left out.
pub(super) fn get_all(reply: &str) -> Result<Props, ViewError> {
    let value: Value = serde_json::from_str(reply).map_err(|_| bad("not JSON"))?;
    let map = value
        .get("data")
        .and_then(|d| d.get(0))
        .and_then(Value::as_object)
        .ok_or_else(|| bad("no property map"))?;
    Ok(map
        .iter()
        .filter_map(|(k, v)| v.get("data").map(|d| (k.clone(), d.clone())))
        .collect())
}

/// A required string property.
pub(super) fn string(props: &Props, key: &str) -> Result<String, ViewError> {
    props
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| bad(key))
}

fn boolean(props: &Props, key: &str) -> Result<bool, ViewError> {
    props
        .get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| bad(key))
}

fn number(props: &Props, key: &str) -> Result<u64, ViewError> {
    props
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| bad(key))
}

fn strings(value: &Value, key: &str) -> Result<Vec<String>, ViewError> {
    value
        .as_array()
        .ok_or_else(|| bad(key))?
        .iter()
        .map(|v| v.as_str().map(str::to_owned).ok_or_else(|| bad(key)))
        .collect()
}

fn string_list(props: &Props, key: &str) -> Result<Vec<String>, ViewError> {
    strings(props.get(key).ok_or_else(|| bad(key))?, key)
}

fn string_set(props: &Props, key: &str) -> Result<BTreeSet<String>, ViewError> {
    Ok(string_list(props, key)?.into_iter().collect())
}

/// `UnitProps` from the Unit interface.
pub(super) fn unit_props(props: &Props) -> Result<UnitProps, ViewError> {
    Ok(UnitProps {
        id: string(props, "Id")?,
        names: string_set(props, "Names")?,
        load_state: string(props, "LoadState")?,
        unit_file_state: string(props, "UnitFileState").unwrap_or_default(),
        transient: boolean(props, "Transient")?,
        fragment_path: string(props, "FragmentPath")?,
        source_path: string(props, "SourcePath").unwrap_or_default(),
        drop_in_paths: string_list(props, "DropInPaths")?,
        need_daemon_reload: boolean(props, "NeedDaemonReload")?,
        triggered_by: string_set(props, "TriggeredBy")?,
        triggers: string_set(props, "Triggers")?,
    })
}

fn exec_commands(props: &Props, key: &str) -> Result<Vec<ExecCommand>, ViewError> {
    props
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| bad(key))?
        .iter()
        .map(|entry| {
            let fields = entry.as_array().ok_or_else(|| bad(key))?;
            Ok(ExecCommand {
                path: fields
                    .first()
                    .and_then(Value::as_str)
                    .ok_or_else(|| bad(key))?
                    .to_owned(),
                argv: strings(fields.get(1).ok_or_else(|| bad(key))?, key)?,
                ignore_failure: fields
                    .get(2)
                    .and_then(Value::as_bool)
                    .ok_or_else(|| bad(key))?,
            })
        })
        .collect()
}

/// `LoadedProps` from the Unit and Service interfaces together. Runtime fields (start time,
/// pid, exit code and status) are never copied.
pub(super) fn loaded_props(unit: &Props, service: &Props) -> Result<LoadedProps, ViewError> {
    let environment_files = service
        .get("EnvironmentFiles")
        .and_then(Value::as_array)
        .ok_or_else(|| bad("EnvironmentFiles"))?
        .iter()
        .map(|entry| {
            entry
                .get(0)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| bad("EnvironmentFiles"))
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    let reload = exec_commands(service, "ExecReload")?
        .into_iter()
        .map(|c| c.argv)
        .collect();
    Ok(LoadedProps {
        exec_start: exec_commands(service, "ExecStart")?,
        exec_start_pre: exec_commands(service, "ExecStartPre")?,
        exec_start_post: exec_commands(service, "ExecStartPost")?,
        exec_stop: exec_commands(service, "ExecStop")?,
        exec_reload: reload,
        environment: string_list(service, "Environment")?,
        environment_files,
        user: string(service, "User")?,
        group: string(service, "Group")?,
        working_directory: string(service, "WorkingDirectory")?,
        restart: string(service, "Restart")?,
        service_type: string(service, "Type")?,
        start_limit_interval_usec: number(unit, "StartLimitIntervalUSec")?,
        start_limit_burst: u32::try_from(number(unit, "StartLimitBurst")?)
            .map_err(|_| bad("StartLimitBurst"))?,
        requires: string_set(unit, "Requires")?,
        binds_to: string_set(unit, "BindsTo")?,
        part_of: string_set(unit, "PartOf")?,
        wants: string_set(unit, "Wants")?,
    })
}

/// The runtime facts from the Unit and Service interfaces. `InvocationID` is 16 bytes, written
/// as 32 lowercase hex characters; empty when the unit never ran.
pub(super) fn runtime(unit: &Props, service: &Props) -> Result<UnitRuntime, ViewError> {
    let invocation_id = match unit.get("InvocationID").and_then(Value::as_array) {
        Some(bytes) => bytes
            .iter()
            .map(|b| {
                b.as_u64()
                    .and_then(|n| u8::try_from(n).ok())
                    .map(|n| format!("{n:02x}"))
                    .ok_or_else(|| bad("InvocationID"))
            })
            .collect::<Result<String, _>>()?,
        None => String::new(),
    };
    Ok(UnitRuntime {
        invocation_id,
        main_pid: u32::try_from(number(service, "ExecMainPID")?).map_err(|_| bad("ExecMainPID"))?,
        active_state: string(unit, "ActiveState")?,
        result: string(service, "Result")?,
        n_restarts: u32::try_from(number(service, "NRestarts")?).map_err(|_| bad("NRestarts"))?,
    })
}
