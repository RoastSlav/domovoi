//! The helper's safety identity and exclusion inputs right now, for a containment request.
//!
//! The same computation as a `safety.attest`, minus the signature: read the key and the state,
//! compute the effective sets, assign the generation, make a new state durable, and name the
//! identity. The caller already holds the state lock and passes its guard, so no second lock is
//! ever taken. Nothing here is signed or returned to the agent.

use domovoi_core::attest::{ConfigLines, RouteLines, SafetyIdentity};

use super::compute::plan;
use super::run::{AttestEnv, boot_id, commit, config_lines, open_key, route_lines};
use super::state::load_state;
use crate::state::LockGuard;

/// What the helper knows about itself at this moment.
#[derive(Debug, Clone)]
pub struct Current {
    /// The effective configuration lines, `Err` when `helper.toml` could not be read.
    pub config: Result<ConfigLines, ()>,
    /// The observed route lines, `Err` when a route read failed.
    pub routes: Result<RouteLines, ()>,
    /// The identity a verified approval must equal, `Err` when none can be named (no key, no
    /// state, a generation that cannot be assigned or a state that cannot be written).
    pub identity: Result<SafetyIdentity, ()>,
}

/// Computes [`Current`] under the held lock. Never fails as a whole: each part says for itself
/// that it could not be established.
pub async fn current(env: &AttestEnv, guard: &LockGuard<'_>) -> Current {
    let config = config_lines(env).map_err(|_| ());
    let routes = match &config {
        Ok(config) => route_lines(env, config).await.map_err(|_| ()),
        Err(()) => Err(()),
    };
    let identity = identity(env, guard, config.as_ref().ok(), routes.as_ref().ok());
    Current {
        config,
        routes,
        identity,
    }
}

fn identity(
    env: &AttestEnv,
    guard: &LockGuard<'_>,
    config: Option<&ConfigLines>,
    routes: Option<&RouteLines>,
) -> Result<SafetyIdentity, ()> {
    let (config, routes) = config.zip(routes).ok_or(())?;
    let dir = env.dirs.helper_dir.as_path();
    let key = open_key(dir).map_err(|_| ())?;
    let state = load_state(dir).map_err(|_| ())?;
    if state.key_fingerprint != key.fingerprint() {
        return Err(());
    }
    let boot = boot_id(env).map_err(|_| ())?;
    let planned = plan(&state, config.hash(), routes.hash(), &boot).map_err(|_| ())?;
    if let Some(next) = &planned.write {
        commit(env, guard, next).map_err(|err| {
            tracing::error!(%err, "the new state could not be made durable");
        })?;
    }
    Ok(SafetyIdentity {
        fingerprint: key.fingerprint(),
        boot_id: boot,
        generation: planned.generation,
        config_hash: config.hash(),
        route_state_hash: routes.hash(),
    })
}
