//! Simulated helper: `safety.attest` and a `Contain` apply path, both run under the
//! `state.lock` flock for the whole sequence (RW-37). No nft, no netlink: the nft set is a
//! plain in-memory set and routes come from a `RouteSource`.

use std::collections::BTreeSet;
use std::net::IpAddr;

use crate::config::{ConfigError, ConfigLines, effective_config};
use crate::encoding::{Cidr, parse_cidr, parse_cidr_config};
use crate::hash::HASH_SENTINEL;
use crate::keys::{fingerprint, key_id, public_bytes, sign_report};
use crate::report::Report;
use crate::routes::{Endpoints, ReadError, RouteLines, RouteSource, observe};
use crate::safety::{ExpectedSafety, SafetyIdentity};
use crate::state::{FailClosed, HelperDir, HelperState, Session, Stage};
use crate::time::format_unix;

/// Per-call environment: time, boot id and the endpoints known outside `helper.toml`.
#[derive(Debug, Clone)]
pub struct Env {
    /// Unix seconds.
    pub now: u64,
    /// `/proc/sys/kernel/random/boot_id` (injected).
    pub boot_id: String,
    /// Hub addresses (from agent configuration).
    pub hub_addrs: Vec<IpAddr>,
    /// Kernel-observed link peers.
    pub link_peers: Vec<(IpAddr, u16)>,
}

/// Everything the helper reads to recompute its set.
pub struct Inputs<'a> {
    /// `helper.toml` text, or `None` when it cannot be read.
    pub config_text: Option<&'a str>,
    /// Route source.
    pub routes: &'a dyn RouteSource,
    /// Environment.
    pub env: &'a Env,
}

/// The helper's freshly computed state: either part may have failed.
#[derive(Debug)]
pub struct Computed {
    /// Config lines or why not.
    pub config: Result<ConfigLines, String>,
    /// Route lines or why not (also not attempted when the config is unusable).
    pub routes: Result<RouteLines, String>,
}

impl Computed {
    /// First `incomplete_reason`, if any part failed.
    #[must_use]
    pub fn failure(&self) -> Option<String> {
        self.config.as_ref().err().or(self.routes.as_ref().err()).cloned()
    }
}

/// Recompute the effective set from the inputs.
#[must_use]
pub fn compute(inp: &Inputs<'_>) -> Computed {
    let Some(text) = inp.config_text else {
        return Computed { config: Err("config_unreadable".into()), routes: Err("config_unreadable".into()) };
    };
    let cfg = match effective_config(text) {
        Ok(c) => c,
        Err(e @ (ConfigError::Parse(_) | ConfigError::Invalid(_))) => {
            let r = e.reason().to_owned();
            return Computed { config: Err(r.clone()), routes: Err(r) };
        }
    };
    let cidrs: Vec<Cidr> = cfg.cidr.iter().filter_map(|c| parse_cidr(c).ok()).collect();
    let ep = Endpoints { hub_addrs: inp.env.hub_addrs.clone(), link_peers: inp.env.link_peers.clone(), cidrs };
    let routes = observe(inp.routes, &ep).map_err(|e: ReadError| e.reason().to_owned());
    Computed { config: Ok(cfg), routes }
}

/// A signed attestation as the helper hands it out.
#[derive(Debug, Clone)]
pub struct SignedAttestation {
    /// 32 raw public-key bytes.
    pub public_key: [u8; 32],
    /// Report bytes.
    pub report: Vec<u8>,
    /// 64-byte signature.
    pub signature: [u8; 64],
}

/// The helper could not sign anything: no usable key exists.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unsigned: {0}")]
pub struct Unsigned(pub FailClosed);

/// Update the persisted state to the freshly computed hashes. Returns the generation to report.
fn advance(
    sess: &Session,
    st: &HelperState,
    cfg_hash: [u8; 32],
    route_hash: [u8; 32],
    boot_id: &str,
    hook: &mut dyn FnMut(Stage),
) -> Result<u64, std::io::Error> {
    let changed = st.config_hash != cfg_hash || st.route_state_hash != route_hash;
    if !changed && st.boot_id == boot_id {
        return Ok(st.generation);
    }
    let next = HelperState {
        key_fingerprint: st.key_fingerprint.clone(),
        generation: if changed { st.generation + 1 } else { st.generation },
        config_hash: cfg_hash,
        route_state_hash: route_hash,
        boot_id: boot_id.to_owned(),
    };
    sess.write(&next, hook)?;
    Ok(next.generation)
}

fn incomplete(
    key: &ed25519_dalek::SigningKey,
    nonce: [u8; 32],
    env: &Env,
    generation: u64,
    c: &Computed,
    reason: &str,
) -> SignedAttestation {
    let (config, ch) = c.config.as_ref().map_or((ConfigLines::default(), HASH_SENTINEL), |l| (l.clone(), l.hash()));
    let (routes, rh) = c.routes.as_ref().map_or((RouteLines::default(), HASH_SENTINEL), |l| (l.clone(), l.hash()));
    let pk = public_bytes(key);
    let report = Report {
        helper_key_id: key_id(&pk),
        nonce,
        boot_id: env.boot_id.clone(),
        generation,
        config_hash: ch,
        route_state_hash: rh,
        complete: false,
        incomplete_reason: Some(reason.to_owned()),
        computed_at: format_unix(env.now),
        config,
        routes,
    };
    let bytes = report.render();
    SignedAttestation { public_key: pk, signature: sign_report(key, &bytes), report: bytes }
}

/// `safety.attest`: lock, validate state, recompute, compare, advance and persist when changed,
/// then sign. `nonce` is the only caller input.
///
/// # Errors
/// [`Unsigned`] when there is no usable key (`uninitialized`, `key_missing`, `key_corrupt`).
pub fn attest(
    dir: &HelperDir,
    nonce: [u8; 32],
    inp: &Inputs<'_>,
    hook: &mut dyn FnMut(Stage),
) -> Result<SignedAttestation, Unsigned> {
    let sess = dir.begin().map_err(|_| Unsigned(FailClosed::StateCorrupt("lock".into())))?;
    let key = match &sess.key {
        Ok(k) => k.clone(),
        Err(f) => return Err(Unsigned(f.clone())),
    };
    let computed = compute(inp);
    let st = match &sess.state {
        Ok(s) => s.clone(),
        Err(f) => return Ok(incomplete(&key, nonce, inp.env, 0, &computed, f.reason())),
    };
    if let Some(reason) = computed.failure() {
        return Ok(incomplete(&key, nonce, inp.env, st.generation, &computed, &reason));
    }
    let (Ok(cfg), Ok(routes)) = (&computed.config, &computed.routes) else {
        return Ok(incomplete(&key, nonce, inp.env, st.generation, &computed, "config_invalid"));
    };
    let (ch, rh) = (cfg.hash(), routes.hash());
    let generation = match advance(&sess, &st, ch, rh, &inp.env.boot_id, hook) {
        Ok(g) => g,
        Err(_) => return Ok(incomplete(&key, nonce, inp.env, st.generation, &computed, "state_write_failed")),
    };
    let pk = public_bytes(&key);
    let report = Report {
        helper_key_id: key_id(&pk),
        nonce,
        boot_id: inp.env.boot_id.clone(),
        generation,
        config_hash: ch,
        route_state_hash: rh,
        complete: true,
        incomplete_reason: None,
        computed_at: format_unix(inp.env.now),
        config: cfg.clone(),
        routes: routes.clone(),
    };
    let bytes = report.render();
    Ok(SignedAttestation { public_key: pk, signature: sign_report(&key, &bytes), report: bytes })
}

/// Who labelled the request. The helper does not read it; the agent does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthKind {
    /// Human approval.
    Human,
    /// Policy permit.
    Policy,
}

/// A `Contain` request as the helper sees it.
#[derive(Debug, Clone)]
pub struct ContainRequest {
    /// Target prefix text.
    pub target: String,
    /// Requested TTL in seconds.
    pub ttl_s: u64,
    /// Expected safety, if the caller sent one.
    pub expected: Option<ExpectedSafety>,
    /// The agent's own (stricter-only) TTL limit.
    pub agent_max_ttl: Option<u64>,
    /// The agent's own (stricter-only) active-count limit.
    pub agent_max_active: Option<u64>,
    /// A count the agent claims; the helper must ignore it.
    pub agent_reported_active: Option<u64>,
}

/// Why the helper refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// Full identity differs from the freshly computed one.
    SafetyChanged,
    /// State could not be established; carries the underlying reason.
    SafetyUnknown(String),
    /// Target overlaps the actual exclusion set.
    ExclusionOverlap,
    /// No management CIDR is configured.
    NoManagementCidrs,
    /// TTL above the effective maximum.
    TtlAboveMaximum,
    /// Active count at the effective maximum.
    MaxActiveReached,
    /// Mutation limit reached.
    MutationLimit,
    /// Target is not a prefix.
    BadTarget,
}

/// The helper's in-memory nft set plus mutation limiter.
#[derive(Debug, Default)]
pub struct HelperRuntime {
    /// Elements currently blocked.
    pub nft: BTreeSet<String>,
    /// Mutation timestamps (unix seconds) inside the window.
    pub mutations: Vec<u64>,
    /// Allowed mutations per hour (0 = none).
    pub per_hour: u64,
}

/// Identity of the helper's freshly computed state, for comparison.
#[must_use]
pub fn current_identity(fp: &str, boot_id: &str, generation: u64, c: &ConfigLines, r: &RouteLines) -> SafetyIdentity {
    SafetyIdentity {
        fingerprint: fp.to_owned(),
        boot_id: boot_id.to_owned(),
        generation,
        config_hash: c.hash(),
        route_state_hash: r.hash(),
    }
}

/// `Contain` apply under the lock. `hook` is called with `"compared"` after the comparison and
/// `"applied"` after the set change, both inside the lock.
pub fn apply_contain(
    dir: &HelperDir,
    req: &ContainRequest,
    inp: &Inputs<'_>,
    rt: &mut HelperRuntime,
    state_hook: &mut dyn FnMut(Stage),
    hook: &mut dyn FnMut(&'static str),
) -> Result<u64, Refused> {
    let unknown = |s: &str| Refused::SafetyUnknown(s.to_owned());
    let sess = dir.begin().map_err(|_| unknown("lock"))?;
    if let Some(f) = sess.failure() {
        return Err(unknown(f.reason()));
    }
    let (Ok(key), Ok(st)) = (&sess.key, &sess.state) else { return Err(unknown("state")) };
    let computed = compute(inp);
    if let Some(r) = computed.failure() {
        return Err(unknown(&r));
    }
    let (Ok(cfg), Ok(routes)) = (&computed.config, &computed.routes) else { return Err(unknown("compute")) };
    let generation = advance(&sess, st, cfg.hash(), routes.hash(), &inp.env.boot_id, state_hook)
        .map_err(|_| unknown("state_write_failed"))?;
    let fp = fingerprint(&public_bytes(key));
    let current = current_identity(&fp, &inp.env.boot_id, generation, cfg, routes);
    if let Some(ExpectedSafety::Verified(exp)) = &req.expected {
        if *exp != current {
            return Err(Refused::SafetyChanged);
        }
    }
    hook("compared");
    // Everything below checks the target against the helper's ACTUAL set, whatever `expected`
    // said (absent, unverified or fabricated).
    let target = parse_cidr_config(&req.target).map_err(|_| Refused::BadTarget)?;
    if cfg.cidr.is_empty() {
        return Err(Refused::NoManagementCidrs);
    }
    let mut excluded: Vec<Cidr> = cfg.cidr.iter().filter_map(|c| parse_cidr(c).ok()).collect();
    excluded.extend(inp.env.hub_addrs.iter().map(|a| Cidr::host(*a)));
    excluded.extend(inp.env.link_peers.iter().map(|(a, _)| Cidr::host(*a)));
    if excluded.iter().any(|e| e.overlaps(&target)) {
        return Err(Refused::ExclusionOverlap);
    }
    let max_ttl = req.agent_max_ttl.map_or(cfg.max_ttl_s, |a| a.min(cfg.max_ttl_s));
    if req.ttl_s > max_ttl {
        return Err(Refused::TtlAboveMaximum);
    }
    let max_active = req.agent_max_active.map_or(cfg.max_active, |a| a.min(cfg.max_active));
    let active = u64::try_from(rt.nft.len()).unwrap_or(u64::MAX);
    if active >= max_active {
        return Err(Refused::MaxActiveReached);
    }
    rt.mutations.retain(|t| inp.env.now.saturating_sub(*t) < 3600);
    if u64::try_from(rt.mutations.len()).unwrap_or(u64::MAX) >= rt.per_hour {
        return Err(Refused::MutationLimit);
    }
    rt.mutations.push(inp.env.now);
    rt.nft.insert(target.network().render());
    hook("applied");
    Ok(generation)
}
