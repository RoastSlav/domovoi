//! The unit identity algorithm: twelve ordered steps, the first match decides, one trigger per
//! token. Pure code behind two read-only ports.

use std::collections::BTreeSet;

use sha2::{Digest, Sha256};

use super::identity::{FilePin, UnitIdentityV1, encode_loaded};
use super::name::{IdentityHash, UnitId, UnitName};
use super::ports::{FileKind, SystemdView, UnitFs};
use super::props::{ALLOWED_DIRS, UnitFileState, UnitPolicy, UnitProps};
use super::refusal::{UnitRefusal, UnsupportedReason};

fn has_control(path: &str) -> bool {
    path.chars().any(char::is_control)
}

fn path_shape_ok(path: &str) -> bool {
    path.starts_with('/') && !path.contains("//") && !path.split('/').any(|part| part == "..")
}

fn in_allowed_dir(path: &str) -> bool {
    ALLOWED_DIRS.iter().any(|dir| path.starts_with(dir))
}

fn file_state(props: &UnitProps) -> Result<UnitFileState, UnitRefusal> {
    use UnsupportedReason as R;
    let reason = match props.unit_file_state.as_str() {
        "enabled" => return Ok(UnitFileState::Enabled),
        "disabled" => return Ok(UnitFileState::Disabled),
        "static" => return Ok(UnitFileState::Static),
        "indirect" => return Ok(UnitFileState::Indirect),
        "masked" => R::Masked,
        "linked" | "linked-runtime" => R::Linked,
        "generated" => R::Generated,
        "enabled-runtime" => R::EnabledRuntime,
        "alias" => R::Alias,
        _ => R::UnitFileState,
    };
    Err(UnitRefusal::TargetUnsupported(reason))
}

fn protected(props: &UnitProps, policy: &UnitPolicy) -> bool {
    let named = std::iter::once(&props.id).chain(props.names.iter());
    let by_name = named.into_iter().any(|n| policy.protected.contains(n));
    // A service triggered by a protected socket is protected with it.
    let by_trigger = props
        .triggered_by
        .iter()
        .any(|t| policy.protected.contains(t));
    by_name || by_trigger
}

fn pin(fs: &dyn UnitFs, path: &str) -> Result<FilePin, UnitRefusal> {
    let bytes = fs.read_once(path).map_err(|_| UnitRefusal::ReadFailed)?;
    Ok(FilePin {
        path: path.to_owned(),
        sha256: hex::encode(Sha256::digest(&bytes)),
    })
}

/// Resolves `name` to the identity a restart binds, or the refusal that stops it.
pub async fn resolve_identity(
    name: &UnitName,
    sys: &dyn SystemdView,
    fs: &dyn UnitFs,
    policy: &UnitPolicy,
) -> Result<UnitIdentityV1, UnitRefusal> {
    // 2: the manager. A failure of any kind is "nothing is known".
    let props = sys
        .load_unit(name)
        .await
        .map_err(|_| UnitRefusal::ReadFailed)?;
    // 3: the exit status is never a signal, only `LoadState`.
    if props.load_state == "not-found" {
        return Err(UnitRefusal::TargetNotFound);
    }
    // 4
    if props.load_state != "loaded" {
        return Err(UnitRefusal::TargetUnsupported(UnsupportedReason::Masked));
    }
    if props.transient {
        return Err(UnitRefusal::TargetUnsupported(UnsupportedReason::Transient));
    }
    let unit_file_state = file_state(&props)?;
    // The drop-in directory of a template cannot be enumerated without parsing the instance
    // name, which the helper never does; an instance is refused rather than half-checked.
    if props.id.contains('@') || props.names.iter().any(|n| n.contains('@')) {
        return Err(UnitRefusal::TargetUnsupported(UnsupportedReason::Instance));
    }
    let id = UnitId::parse(&props.id).map_err(|_| UnitRefusal::ReadFailed)?;
    // 5, 6
    if protected(&props, policy) {
        return Err(UnitRefusal::TargetProtected);
    }
    if !policy.allow.contains(&id) {
        return Err(UnitRefusal::TargetNotAllowlisted);
    }
    // 7
    if props.need_daemon_reload {
        return Err(UnitRefusal::TargetStale);
    }
    // 8
    let files = std::iter::once(&props.fragment_path).chain(props.drop_in_paths.iter());
    for (index, path) in files.enumerate() {
        if has_control(path) {
            return Err(UnitRefusal::TargetUnsupported(
                UnsupportedReason::ControlCharacter,
            ));
        }
        let is_drop_in = index > 0;
        if !path_shape_ok(path)
            || !in_allowed_dir(path)
            || (is_drop_in
                && !std::path::Path::new(path)
                    .extension()
                    .is_some_and(|e| e == "conf"))
        {
            return Err(UnitRefusal::location(path));
        }
        match fs.lstat(path).map_err(|_| UnitRefusal::ReadFailed)? {
            FileKind::Regular => {}
            FileKind::Symlink => return Err(UnitRefusal::symlink(path)),
            FileKind::Other => return Err(UnitRefusal::location(path)),
        }
    }
    // 9: the disk side is the only stale check beyond `NeedDaemonReload`.
    let disk = fs
        .enumerate_dropins(&id, &props.names)
        .map_err(|_| UnitRefusal::ReadFailed)?;
    if let Some(path) = disk.iter().find(|p| !in_allowed_dir(p) || has_control(p)) {
        return Err(UnitRefusal::location(path));
    }
    let reported: BTreeSet<&String> = props.drop_in_paths.iter().collect();
    let on_disk: BTreeSet<&String> = disk.iter().collect();
    if reported != on_disk {
        return Err(UnitRefusal::TargetStale);
    }
    // 10: one open, one read each; any failure is `read_failed`, never an empty hash.
    let fragment = pin(fs, &props.fragment_path)?;
    let drop_ins = props
        .drop_in_paths
        .iter()
        .map(|path| pin(fs, path))
        .collect::<Result<Vec<_>, _>>()?;
    // 11
    let loaded = sys
        .service_props(&id)
        .await
        .map_err(|_| UnitRefusal::ReadFailed)?;
    let loaded_hash: [u8; 32] = Sha256::digest(encode_loaded(&loaded)).into();
    // 12
    let mut disk_drop_ins = disk;
    disk_drop_ins.sort();
    let mut ordered = props.drop_in_paths.clone();
    ordered.sort();
    debug_assert_eq!(disk_drop_ins, ordered);
    Ok(UnitIdentityV1 {
        id,
        unit_file_state,
        fragment,
        drop_ins,
        disk_drop_ins: ordered,
        loaded_hash,
    })
}

/// Compares the identity bound at approval with the one read now. `Err(TargetChanged)` names
/// the first differing field; it means only that and needs a new approval.
pub fn compare_identity(bound: IdentityHash, now: &UnitIdentityV1) -> Result<(), UnitRefusal> {
    if now.hash() == bound {
        Ok(())
    } else {
        Err(UnitRefusal::TargetChanged {
            field: "identity".to_owned(),
        })
    }
}

/// Like [`compare_identity`] when the bound identity itself is known: names the first
/// differing field.
pub fn compare_to_bound(bound: &UnitIdentityV1, now: &UnitIdentityV1) -> Result<(), UnitRefusal> {
    match bound.first_difference(now) {
        None => Ok(()),
        Some(field) => Err(UnitRefusal::TargetChanged { field }),
    }
}
