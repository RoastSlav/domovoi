//! Serving one `nft` request: the common prelude, then the operation.

use std::path::Path;
use std::sync::Arc;

use domovoi_config::HelperConfig;
use domovoi_core::containment::templates::{MaxElements, TableName};
use domovoi_core::containment::{
    ExclusionRefusal, Gate, Layout, Net, Token, effective_exclusions, gate_for,
};
use domovoi_core::helper::{
    HelperGroup, HelperRequest, HelperResponse, UnavailableReason, write_tier_gate,
};
use domovoi_core::identity::RefusalToken;
use domovoi_core::isolation::PlatformTier;
use domovoi_core::ports::ProcessExecutor;

use super::exec::Runner;
use super::journal_file::Journal;
use super::limits::inputs_from;
use super::{
    Cx, Facts, Refusal, State, apply, confirm, drill, ensure, list, recover, rollback, selftest,
};
use crate::attest::AttestEnv;
use crate::attest::current::{Current, current};
use crate::reads::limits::{request_deadline, within};
use crate::serve::{ServeContext, read_platform_record};
use crate::state::StateLock;

fn op_name(request: &HelperRequest) -> &'static str {
    match request {
        HelperRequest::FirewallApply { .. } => "firewall_apply",
        HelperRequest::FirewallConfirm { .. } => "firewall_confirm",
        HelperRequest::FirewallRollback { .. } => "firewall_rollback",
        _ => "firewall_list_blocks",
    }
}

/// The effective exclusion set from what the helper observed, or the refusal that says why none
/// can be computed.
pub fn exclusions_of(current: &Current) -> Result<Vec<Net>, Refusal> {
    let incomplete = || {
        Refusal::new(
            Token::ExclusionsIncomplete,
            "an exclusion input could not be established",
        )
    };
    let (Ok(config), Ok(routes)) = (&current.config, &current.routes) else {
        return Err(incomplete());
    };
    let inputs = inputs_from(&config.cidr, routes);
    effective_exclusions(inputs.as_ref()).map_err(|e| match e {
        ExclusionRefusal::Incomplete => incomplete(),
        ExclusionRefusal::NoManagementCidrs => {
            Refusal::new(Token::NoManagementCidrs, "no management CIDR is configured")
        }
    })
}

/// Answers one `nft` request.
pub async fn serve(
    request: &HelperRequest,
    ctx: &ServeContext,
    peer_uid: Option<u32>,
) -> HelperResponse {
    let Ok(config) = &ctx.config else {
        return HelperResponse::Refused {
            token: RefusalToken::HelperConfigInvalid,
        };
    };
    let op = op_name(request);
    let unavailable = |reason| HelperResponse::Unavailable {
        group: HelperGroup::Nft,
        reason,
    };
    if !config.configured_groups().contains(&HelperGroup::Nft) {
        return unavailable(UnavailableReason::GroupDisabled);
    }
    let (record_state, record) = read_platform_record(ctx.inspect.as_ref(), &ctx.dirs);
    let tier = record.map_or(PlatformTier::Blocked, |r| r.tier);
    let tier_gate = write_tier_gate(record_state, tier, config.write_opt_in());
    let needs_tier = matches!(
        request,
        HelperRequest::FirewallApply { .. } | HelperRequest::FirewallConfirm { .. }
    );
    if needs_tier && let Err(reason) = tier_gate {
        tracing::info!(op, peer_uid, "nft operation refused by the tier gate");
        return unavailable(reason);
    }
    let Some(exec) = ctx.executor.clone() else {
        return Refusal::new(Token::HelperUnavailable, "no process executor").response();
    };
    let deadline = request_deadline(config);
    let work = run(request, ctx, config, exec, tier_gate.is_ok());
    let response = match within(ctx.clock.as_ref(), deadline, work).await {
        Some(response) => response,
        None => Refusal::new(Token::NftTimeout, "the request timed out").response(),
    };
    let outcome = match &response {
        HelperResponse::ContainRefused { token, .. } => token.as_str(),
        _ => "ok",
    };
    tracing::info!(op, outcome, peer_uid, "nft operation served");
    response
}

async fn run(
    request: &HelperRequest,
    ctx: &ServeContext,
    config: &HelperConfig,
    exec: Arc<dyn ProcessExecutor>,
    write_allowed: bool,
) -> HelperResponse {
    match prepare_and_dispatch(request, ctx, config, exec, write_allowed).await {
        Ok(response) => response,
        Err(refusal) => refusal.response(),
    }
}

async fn prepare_and_dispatch(
    request: &HelperRequest,
    ctx: &ServeContext,
    config: &HelperConfig,
    exec: Arc<dyn ProcessExecutor>,
    write_allowed: bool,
) -> Result<HelperResponse, Refusal> {
    let table = TableName::parse(&config.nft.table)
        .map_err(|_| Refusal::new(Token::InternalTemplateError, "the table name is invalid"))?;
    let max_elements = MaxElements::new(config.nft.max_elements)
        .map_err(|_| Refusal::new(Token::InternalTemplateError, "max_elements is out of range"))?;
    let lock = StateLock::open(&ctx.dirs.helper_dir)
        .map_err(|_| Refusal::new(Token::JournalCorrupt, "the state lock cannot be opened"))?;
    let guard = lock
        .acquire()
        .map_err(|_| Refusal::new(Token::JournalCorrupt, "the state lock cannot be taken"))?;
    let sys_root = Path::new("/sys");
    let cx = Cx {
        run: Runner {
            exec: exec.as_ref(),
            inspect: ctx.inspect.as_ref(),
            timeout: config.nft.call_timeout.as_signed(),
            netns: None,
        },
        clock: ctx.clock.as_ref(),
        table,
        max_elements,
        max_ttl_s: u64::try_from(config.safety.containment.max_ttl.as_signed().as_secs())
            .unwrap_or(0),
        max_active: config.safety.containment.max_active,
        mutations_per_hour: config.limits.mutations_per_hour,
        dir: &ctx.dirs.helper_dir,
        sys_root,
        journal: Journal::new(&ctx.dirs.helper_dir),
        guard: &guard,
    };
    let mut loaded = cx.journal.load().map_err(Refusal::journal)?;
    if cx
        .journal
        .compact(&guard, &loaded, cx.clock.now())
        .map_err(Refusal::journal)?
    {
        loaded = cx.journal.load().map_err(Refusal::journal)?;
    }
    let kernel = ctx.inspect.kernel_release().unwrap_or_default();
    let nft_version = match cx.run.version().await {
        Ok(run) if run.ok() => run.stdout.trim().to_owned(),
        _ => String::new(),
    };
    let key = selftest::current_key(kernel.trim(), &nft_version);
    let stored = selftest::read_result(&ctx.dirs.helper_dir);
    let gate = gate_for(stored.as_ref(), &key, write_allowed);
    let wants_checks = matches!(
        request,
        HelperRequest::FirewallApply { .. } | HelperRequest::FirewallConfirm { .. }
    );
    let observed = if wants_checks {
        let env = AttestEnv::new(
            ctx.dirs.clone(),
            ctx.inspect.clone(),
            ctx.clock.clone(),
            exec.clone(),
        );
        Some(current(&env, &guard).await)
    } else {
        None
    };
    let exclusions = observed.as_ref().map(exclusions_of);
    let wanted = match gate {
        Gate::Open {
            interval_path: true,
        } => Some(Layout::HostsAndPrefixes),
        Gate::Open {
            interval_path: false,
        } => Some(Layout::Hosts),
        _ => None,
    };
    let sync = exclusions.as_ref().and_then(|e| e.as_ref().ok());
    let ens = ensure::ensure(&cx, wanted, sync.map(Vec::as_slice), &loaded).await?;
    if recover::recover(&cx, &ens.view, &loaded)? {
        loaded = cx.journal.load().map_err(Refusal::journal)?;
    }
    let facts = Facts {
        gate,
        exclusions: exclusions.unwrap_or_else(|| Ok(Vec::new())),
        identity: observed.map_or(Err(()), |c| c.identity),
        key_text: selftest::gate_text(&gate, &key),
        drills: drill::state_text(&ctx.dirs.helper_dir, &key),
    };
    let st = State { ens, loaded, facts };
    let reply = match request {
        HelperRequest::FirewallApply {
            block,
            source,
            ttl_s,
            expected_safety,
        } => {
            apply::apply(
                &cx,
                &st,
                *block,
                source.as_str(),
                *ttl_s,
                expected_safety.as_ref(),
            )
            .await
        }
        HelperRequest::FirewallConfirm {
            block,
            source,
            ttl_s,
            expected_safety,
        } => {
            confirm::confirm(
                &cx,
                &st,
                *block,
                source.as_str(),
                *ttl_s,
                expected_safety.as_ref(),
            )
            .await
        }
        HelperRequest::FirewallRollback {
            block,
            source,
            ttl_s,
        } => rollback::rollback(&cx, &st, *block, source.as_str(), *ttl_s).await,
        _ => list::list(&cx, &st).await,
    };
    Ok(match reply {
        Ok(outputs) => HelperResponse::Outputs { outputs },
        Err(refusal) => refusal.response(),
    })
}
