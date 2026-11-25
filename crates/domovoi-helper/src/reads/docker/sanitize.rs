//! Removal of secrets from inspect output.

/// Replaces every `Config.Env` entry with its variable name alone, since values often hold
/// secrets. Same behaviour as the checks crate's function of the same name.
pub fn strip_env(inspect: &mut serde_json::Value) {
    let Some(env) = inspect
        .get_mut("Config")
        .and_then(|c| c.get_mut("Env"))
        .and_then(serde_json::Value::as_array_mut)
    else {
        return;
    };
    for entry in env.iter_mut() {
        let name = entry
            .as_str()
            .map(|e| e.split('=').next().unwrap_or_default().to_owned())
            .unwrap_or_default();
        *entry = serde_json::Value::String(name);
    }
}
