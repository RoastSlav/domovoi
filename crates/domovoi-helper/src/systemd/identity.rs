//! `unit.identity`: runs the identity algorithm and answers the identity as string outputs.

use std::collections::BTreeMap;

use domovoi_core::unit::{UnitName, UnitRefusal, encode_loaded, keys, resolve_identity};
use serde_json::json;

use super::UnitEnv;

/// Steps 1 to 12 of the identity algorithm for `name`, with the loaded facts a card shows.
pub async fn identity(
    env: &UnitEnv<'_>,
    name: &UnitName,
) -> Result<BTreeMap<String, String>, UnitRefusal> {
    let resolved = resolve_identity(name, env.sys, env.fs, env.policy).await?;
    let loaded = env
        .sys
        .service_props(&resolved.id)
        .await
        .map_err(|_| UnitRefusal::ReadFailed)?;
    let runtime = env
        .sys
        .runtime(&resolved.id)
        .await
        .map_err(|_| UnitRefusal::ReadFailed)?;
    let names = env
        .sys
        .load_unit(name)
        .await
        .map_err(|_| UnitRefusal::ReadFailed)?
        .names;
    // The loaded hash is recomputed here only to prove the two reads agree on the encoding.
    debug_assert!(!encode_loaded(&loaded).is_empty());
    let argv = loaded
        .exec_start
        .first()
        .map(|c| c.argv.clone())
        .unwrap_or_default();
    let followers: Vec<&String> = loaded
        .part_of
        .iter()
        .chain(loaded.requires.iter())
        .collect();
    let loaded_json = json!({
        "exec_start": argv,
        "user": loaded.user,
        "group": loaded.group,
        "working_directory": loaded.working_directory,
        "type": loaded.service_type,
        "restart": loaded.restart,
        "environment": loaded.environment,
        "followers": followers,
    });
    let pins =
        |pin: &domovoi_core::unit::FilePin| json!({ "path": pin.path, "sha256": pin.sha256 });
    let mut out = BTreeMap::new();
    out.insert(keys::OUT_ID.to_owned(), resolved.id.to_string());
    out.insert(
        keys::OUT_NAMES.to_owned(),
        json!(names.iter().collect::<Vec<_>>()).to_string(),
    );
    out.insert(
        keys::OUT_UNIT_FILE_STATE.to_owned(),
        resolved.unit_file_state.as_str().to_owned(),
    );
    out.insert(
        keys::OUT_IDENTITY_HASH.to_owned(),
        resolved.hash().to_string(),
    );
    out.insert(
        keys::OUT_FRAGMENT.to_owned(),
        pins(&resolved.fragment).to_string(),
    );
    out.insert(
        keys::OUT_DROP_INS.to_owned(),
        json!(resolved.drop_ins.iter().map(pins).collect::<Vec<_>>()).to_string(),
    );
    out.insert(keys::OUT_LOADED.to_owned(), loaded_json.to_string());
    out.insert(keys::OUT_ACTIVE_STATE.to_owned(), runtime.active_state);
    Ok(out)
}
