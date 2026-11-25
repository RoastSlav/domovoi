//! `safety.attest`: nonce in, signed report out.
//!
//! One attestation holds the state lock from the first read of the state to the signature. Under
//! the lock it reads the key and the state, computes the effective sets, assigns the generation,
//! makes the new state durable and only then signs the report that names that generation. Every
//! condition that stops this is either a signed minimal incomplete report (the key is usable and
//! the helper can say why) or an unsigned token (it cannot). Nothing resets the state or makes a
//! key.

use std::path::Path;
use std::sync::Arc;

use domovoi_config::effective_config_lines;
use domovoi_core::attest::{BootId, Cidr, ConfigLines, IncompleteReason, Nonce32, RouteLines};
use domovoi_core::helper::{SignedAttestation, UnsignedToken};
use domovoi_core::identity::{HostDirs, RefusalToken};
use domovoi_core::ports::{Clock, HostInspect, ProcessExecutor};

use super::compute::{ReportContext, at_second, complete_report, incomplete_bytes, plan};
use super::endpoints::{AgentTomlEndpoints, EndpointSource};
use super::ip_source::IpSource;
use super::keys::{Key, KeyFault, load_key};
use super::observe::{Endpoints, ReadError, observe};
use super::state::{StateFile, StateReadFault, load_state};
use super::{BOOT_ID_PATH, STATE_FILE, lock};
use crate::start_check::check_paths;
use crate::state::{WriteFaults, write_atomic_locked};

/// A moment of one attestation that a test can observe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttestStage {
    /// The new state is durable: written, synced, renamed and the directory synced.
    StateWritten,
    /// The signature was made.
    Signed,
}

/// Everything an attestation reads from or does to the host.
#[derive(Clone)]
pub struct AttestEnv {
    /// The helper's directories.
    pub dirs: HostDirs,
    /// Read-only facts of the host: file modes, `helper.toml`, `agent.toml`, the boot id.
    pub inspect: Arc<dyn HostInspect>,
    /// The time of the report, at second precision.
    pub clock: Arc<dyn Clock>,
    /// Runs `ip`.
    pub exec: Arc<dyn ProcessExecutor>,
    /// Stages of a state write a test makes fail. Always the default in production.
    pub write_faults: WriteFaults,
    /// Called at each [`AttestStage`]. Always `None` in production.
    pub stage_hook: Option<Arc<dyn Fn(AttestStage) + Send + Sync>>,
}

impl AttestEnv {
    /// An environment with no injected faults and no hook.
    #[must_use]
    pub fn new(
        dirs: HostDirs,
        inspect: Arc<dyn HostInspect>,
        clock: Arc<dyn Clock>,
        exec: Arc<dyn ProcessExecutor>,
    ) -> Self {
        Self {
            dirs,
            inspect,
            clock,
            exec,
            write_faults: WriteFaults::default(),
            stage_hook: None,
        }
    }

    fn stage(&self, stage: AttestStage) {
        if let Some(hook) = &self.stage_hook {
            hook(stage);
        }
    }
}

/// The answer to a challenge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttestOutcome {
    /// A signed report, complete or minimal incomplete.
    Signed(SignedAttestation),
    /// No signature could be made. The token is a hint, never evidence.
    Unsigned(UnsignedToken),
}

fn permissions(env: &AttestEnv) -> Result<(), UnsignedToken> {
    check_paths(&*env.inspect, &env.dirs).map_err(|refusal| match refusal.token {
        RefusalToken::Uninitialized => UnsignedToken::Uninitialized,
        _ => UnsignedToken::HelperStatePermissions,
    })
}

fn state_exists(dir: &Path) -> bool {
    std::fs::symlink_metadata(dir.join(STATE_FILE)).is_ok()
}

pub(super) fn open_key(dir: &Path) -> Result<Key, UnsignedToken> {
    load_key(dir).map_err(|fault| match fault {
        KeyFault::Missing if state_exists(dir) => UnsignedToken::KeyMissing,
        KeyFault::Missing => UnsignedToken::Uninitialized,
        KeyFault::Corrupt => UnsignedToken::KeyCorrupt,
    })
}

pub(super) fn boot_id(env: &AttestEnv) -> Result<BootId, UnsignedToken> {
    let text = env
        .inspect
        .read_text(Path::new(BOOT_ID_PATH))
        .map_err(|_| UnsignedToken::HelperStatePermissions)?;
    BootId::parse(text.trim()).map_err(|_| UnsignedToken::HelperStatePermissions)
}

/// The leftover temporary file of an interrupted write, removed under the lock.
fn remove_leftover_temp(dir: &Path) {
    let _ = std::fs::remove_file(dir.join(format!(".{STATE_FILE}.tmp")));
}

fn signed(key: &Key, report: Vec<u8>) -> SignedAttestation {
    SignedAttestation {
        public_key: key.public_key(),
        signature: key.sign_report(&report),
        report,
    }
}

pub(super) fn config_lines(env: &AttestEnv) -> Result<ConfigLines, &'static str> {
    let text = env
        .inspect
        .read_text(&env.dirs.etc_dir.join("helper.toml"))
        .map_err(|_| IncompleteReason::CONFIG_UNREADABLE)?;
    effective_config_lines(&text).map_err(|err| err.reason())
}

pub(super) async fn route_lines(
    env: &AttestEnv,
    config: &ConfigLines,
) -> Result<RouteLines, ReadError> {
    let source = AgentTomlEndpoints::load(&*env.inspect, &env.dirs.etc_dir.join("agent.toml"));
    let endpoints = Endpoints {
        hub_addrs: source.hub_addrs()?,
        link_peers: source.link_peers()?,
        cidrs: config
            .cidr
            .iter()
            .map(|text| Cidr::parse_report(text).map_err(|_| ReadError::ConfigUnreadable))
            .collect::<Result<_, _>>()?,
    };
    let ip = IpSource::new(env.exec.clone(), env.inspect.clone());
    observe(&ip, &endpoints).await
}

/// Answers `nonce` with a signed report, or says why it cannot.
///
/// See the module documentation for the order of events.
pub async fn safety_attest(env: &AttestEnv, nonce: Nonce32) -> AttestOutcome {
    match attest(env, nonce).await {
        Ok(attestation) => AttestOutcome::Signed(attestation),
        Err(token) => AttestOutcome::Unsigned(token),
    }
}

async fn attest(env: &AttestEnv, nonce: Nonce32) -> Result<SignedAttestation, UnsignedToken> {
    permissions(env)?;
    let dir = env.dirs.helper_dir.as_path();
    let lock = lock::open(dir)?;
    let guard = lock.acquire().map_err(|err| {
        tracing::error!(%err, "the state lock cannot be taken");
        UnsignedToken::LockUnavailable
    })?;
    remove_leftover_temp(dir);
    let key = open_key(dir)?;
    let ctx = ReportContext {
        helper_key_id: key.key_id(),
        nonce,
        boot_id: boot_id(env)?,
        computed_at: at_second(env.clock.now()),
    };
    let incomplete = |reason: &str| -> Result<SignedAttestation, UnsignedToken> {
        let bytes = incomplete_bytes(&ctx, reason).ok_or(UnsignedToken::HelperStatePermissions)?;
        let attestation = signed(&key, bytes);
        env.stage(AttestStage::Signed);
        Ok(attestation)
    };
    let state = match load_state(dir) {
        Ok(state) => state,
        Err(StateReadFault::Missing) => return incomplete("state_missing"),
        Err(StateReadFault::Corrupt(_)) => return incomplete("state_corrupt"),
    };
    if state.key_fingerprint != key.fingerprint() {
        return incomplete("state_key_mismatch");
    }
    let config = match config_lines(env) {
        Ok(config) => config,
        Err(reason) => return incomplete(reason),
    };
    let routes = match route_lines(env, &config).await {
        Ok(routes) => routes,
        Err(err) => return incomplete(err.token()),
    };
    let Ok(plan) = plan(&state, config.hash(), routes.hash(), &ctx.boot_id) else {
        return incomplete("state_write_failed");
    };
    let Some((_, report)) = complete_report(&ctx, plan.generation, config, routes) else {
        return incomplete("report_too_large");
    };
    if let Some(next) = &plan.write
        && let Err(err) = commit(env, &guard, next)
    {
        tracing::error!(%err, "the new state could not be made durable");
        return incomplete("state_write_failed");
    }
    env.stage(AttestStage::StateWritten);
    let attestation = signed(&key, report);
    env.stage(AttestStage::Signed);
    Ok(attestation)
}

pub(super) fn commit(
    env: &AttestEnv,
    guard: &crate::state::LockGuard<'_>,
    next: &StateFile,
) -> Result<(), crate::state::StateWriteError> {
    write_atomic_locked(
        guard,
        &env.dirs.helper_dir,
        STATE_FILE,
        &next.render(),
        0o600,
        env.write_faults,
    )
}

/// The helper's public key, read under the lock and the same permission checks.
///
/// # Errors
///
/// The unsigned token that says why no key can be read.
pub fn public_key(env: &AttestEnv) -> Result<[u8; 32], UnsignedToken> {
    permissions(env)?;
    let dir = env.dirs.helper_dir.as_path();
    let lock = lock::open(dir)?;
    let _guard = lock.acquire().map_err(|_| UnsignedToken::LockUnavailable)?;
    Ok(open_key(dir)?.public_key())
}
