//! Contract for [`HostRepo`].

use domovoi_core::ports::{EnrollmentToken, HostRepo, StoreError};
use domovoi_core::{
    AgentId, AgentIdentity, AuthenticatedReport, HostId, ReportOutcome, ReportedProtocol,
};

use super::{ContractError, ContractResult};
use crate::build::{T0, at, host};

fn agent(host: HostId) -> AgentIdentity {
    AgentIdentity {
        agent: AgentId::new(),
        host,
        public_key: [7; 32],
        version: "0.1.0".to_owned(),
        enrolled_at: T0,
        revoked_at: None,
    }
}

/// Exercises hosts, agents and enrollment tokens on an empty `repo`.
pub async fn host_repo_contract(repo: &dyn HostRepo) -> ContractResult {
    hosts(repo).await?;
    agents(repo).await?;
    agent_report_is_recorded_only_on_change(repo).await?;
    enrollment_token_is_single_use(repo).await?;
    expired_token_is_rejected(repo).await
}

async fn hosts(repo: &dyn HostRepo) -> ContractResult {
    let nas = host("nas")?;
    let media = host("media")?;
    assert_eq!(repo.get_host(nas.id).await?, None);
    repo.upsert_host(&nas).await?;
    repo.upsert_host(&media).await?;
    assert_eq!(repo.get_host(nas.id).await?, Some(nas.clone()));
    assert_eq!(repo.get_host_by_name(&nas.name).await?, Some(nas.clone()));

    let names: Vec<String> = repo
        .list_hosts()
        .await?
        .into_iter()
        .map(|h| h.name.to_string())
        .collect();
    assert_eq!(names, ["media", "nas"], "hosts are sorted by name");

    let mut relabelled = nas.clone();
    relabelled
        .labels
        .insert("room".to_owned(), "attic".to_owned());
    repo.upsert_host(&relabelled).await?;
    assert_eq!(repo.get_host(nas.id).await?, Some(relabelled));

    let mut clash = host("nas")?;
    clash.id = HostId::new();
    assert!(
        matches!(repo.upsert_host(&clash).await, Err(StoreError::Conflict(_))),
        "a second host may not take an existing name"
    );
    Ok(())
}

async fn agents(repo: &dyn HostRepo) -> ContractResult {
    let box1 = host("agent-box")?;
    repo.upsert_host(&box1).await?;

    let orphan = agent(HostId::new());
    assert_eq!(repo.put_agent(&orphan).await, Err(StoreError::NotFound));

    assert_eq!(repo.live_agent(box1.id).await?, None, "no agent yet");
    let first = agent(box1.id);
    repo.put_agent(&first).await?;
    assert_eq!(repo.get_agent(first.agent).await?, Some(first.clone()));
    assert_eq!(repo.live_agent(box1.id).await?, Some(first.clone()));

    let mut upgraded = first.clone();
    upgraded.version = "0.2.0".to_owned();
    repo.put_agent(&upgraded).await?;
    assert_eq!(repo.get_agent(first.agent).await?, Some(upgraded));

    // An AgentId names one enrollment for life. Its version may move; nothing else may.
    let current = repo
        .get_agent(first.agent)
        .await?
        .ok_or_else(|| ContractError::Setup("the upgraded agent is stored".to_owned()))?;
    let mut changed_key = current.clone();
    changed_key.public_key = [9; 32];
    assert!(
        matches!(
            repo.put_agent(&changed_key).await,
            Err(StoreError::Conflict(_))
        ),
        "an agent id cannot change its public key"
    );
    assert_eq!(repo.get_agent(first.agent).await?, Some(current.clone()));

    let elsewhere = host("agent-elsewhere")?;
    repo.upsert_host(&elsewhere).await?;
    let mut moved = current.clone();
    moved.host = elsewhere.id;
    assert!(
        matches!(repo.put_agent(&moved).await, Err(StoreError::Conflict(_))),
        "an agent id cannot move to another host"
    );
    assert_eq!(
        repo.live_agent(box1.id).await?,
        Some(current.clone()),
        "the refused move did not take effect"
    );
    assert_eq!(repo.live_agent(elsewhere.id).await?, None);

    let mut re_enrolled = current.clone();
    re_enrolled.enrolled_at = at(5);
    assert!(
        matches!(
            repo.put_agent(&re_enrolled).await,
            Err(StoreError::Conflict(_))
        ),
        "an agent id cannot change its enrollment time"
    );

    let mut pre_revoked = current.clone();
    pre_revoked.revoked_at = Some(at(6));
    assert!(
        matches!(
            repo.put_agent(&pre_revoked).await,
            Err(StoreError::Conflict(_))
        ),
        "put_agent cannot revoke an agent"
    );
    assert_eq!(
        repo.live_agent(box1.id).await?,
        Some(current.clone()),
        "the refused revocation did not take effect"
    );

    let second = agent(box1.id);
    assert!(
        matches!(repo.put_agent(&second).await, Err(StoreError::Conflict(_))),
        "a host has at most one active agent"
    );

    repo.revoke_agent(first.agent, at(10)).await?;
    repo.revoke_agent(first.agent, at(20)).await?;
    let revoked = repo.get_agent(first.agent).await?;
    assert_eq!(
        revoked.and_then(|a| a.revoked_at),
        Some(at(10)),
        "revoking twice keeps the first time"
    );
    assert!(
        matches!(repo.put_agent(&first).await, Err(StoreError::Conflict(_))),
        "a revoked agent never comes back"
    );
    let mut after_revoke = current.clone();
    after_revoke.version = "0.3.0".to_owned();
    assert!(
        matches!(
            repo.put_agent(&after_revoke).await,
            Err(StoreError::Conflict(_))
        ),
        "a revoked agent cannot be updated"
    );
    assert!(
        matches!(
            repo.record_agent_report(first.agent, &seen(1, 1, "0.1.0"), at(15))
                .await,
            Err(StoreError::Conflict(_))
        ),
        "a revoked agent cannot update its stored report"
    );
    assert_eq!(
        repo.live_agent(box1.id).await?,
        None,
        "a revoked agent is not live"
    );
    repo.put_agent(&second).await?;
    assert_eq!(repo.live_agent(box1.id).await?, Some(second.clone()));
    assert_eq!(
        repo.revoke_agent(AgentId::new(), T0).await,
        Err(StoreError::NotFound)
    );
    Ok(())
}

fn seen(major: u16, minor: u16, release: &str) -> AuthenticatedReport {
    AuthenticatedReport {
        protocol: ReportedProtocol { major, minor },
        release: release.to_owned(),
        compatible: major == 1 && minor == 1,
    }
}

/// A report is written when it changes, refreshed at most hourly, and a mismatch pins the
/// first time the hub rejected it until a compatible handshake clears it.
async fn agent_report_is_recorded_only_on_change(repo: &dyn HostRepo) -> ContractResult {
    let box2 = host("report-box")?;
    repo.upsert_host(&box2).await?;
    let enrolled = agent(box2.id);
    repo.put_agent(&enrolled).await?;
    assert_eq!(repo.agent_report(enrolled.agent).await?, None);

    let base = at(3600);
    let first = repo
        .record_agent_report(enrolled.agent, &seen(1, 1, "0.1.0"), base)
        .await?;
    assert_eq!(
        first,
        ReportOutcome {
            changed: true,
            previous: None,
        }
    );
    assert_eq!(
        repo.agent_report(enrolled.agent).await?,
        Some(domovoi_core::AgentReport {
            protocol: ReportedProtocol { major: 1, minor: 1 },
            release: "0.1.0".to_owned(),
            last_authenticated_at: base,
            incompatible_since: None,
        })
    );

    // Within the hour an identical report writes nothing.
    let inside = at(3600 + 60);
    let same = repo
        .record_agent_report(enrolled.agent, &seen(1, 1, "0.1.0"), inside)
        .await?;
    assert!(!same.changed);
    assert_eq!(
        repo.agent_report(enrolled.agent)
            .await?
            .map(|report| report.last_authenticated_at),
        Some(base),
        "the refresh waits an hour"
    );

    // After an hour it refreshes the timestamp without counting as a change.
    let later = at(3600 + 61 * 60);
    let refreshed = repo
        .record_agent_report(enrolled.agent, &seen(1, 1, "0.1.0"), later)
        .await?;
    assert!(!refreshed.changed);
    assert_eq!(
        repo.agent_report(enrolled.agent)
            .await?
            .map(|report| report.last_authenticated_at),
        Some(later)
    );

    // A different release is a change.
    let release_at = at(3600 + 62 * 60);
    let release = repo
        .record_agent_report(enrolled.agent, &seen(1, 1, "0.2.0"), release_at)
        .await?;
    assert!(release.changed);

    // A mismatch pins the time it was first rejected.
    let mismatch_at = at(3600 + 63 * 60);
    let mismatch = repo
        .record_agent_report(enrolled.agent, &seen(1, 0, "0.2.0"), mismatch_at)
        .await?;
    assert!(mismatch.changed);
    assert_eq!(
        repo.agent_report(enrolled.agent)
            .await?
            .and_then(|report| report.incompatible_since),
        Some(mismatch_at)
    );

    // Repeating it keeps the first time.
    let repeated = repo
        .record_agent_report(enrolled.agent, &seen(1, 0, "0.2.0"), at(3600 + 70 * 60))
        .await?;
    assert!(!repeated.changed);
    assert_eq!(
        repo.agent_report(enrolled.agent)
            .await?
            .and_then(|report| report.incompatible_since),
        Some(mismatch_at)
    );

    // A compatible handshake clears it.
    let cleared = repo
        .record_agent_report(enrolled.agent, &seen(1, 1, "0.2.0"), at(3600 + 71 * 60))
        .await?;
    assert!(cleared.changed);
    assert_eq!(
        repo.agent_report(enrolled.agent)
            .await?
            .and_then(|report| report.incompatible_since),
        None
    );

    assert_eq!(
        repo.record_agent_report(AgentId::new(), &seen(1, 1, "0.1.0"), base)
            .await,
        Err(StoreError::NotFound)
    );
    Ok(())
}

/// A token works once; the second use fails like an unknown token.
pub async fn enrollment_token_is_single_use(repo: &dyn HostRepo) -> ContractResult {
    let target = host("enroll-once")?;
    repo.upsert_host(&target).await?;
    let token = EnrollmentToken {
        host: target.id,
        token_hash: [1; 32],
        expires_at: at(3600),
    };
    repo.create_enrollment_token(&token).await?;
    assert!(
        matches!(
            repo.create_enrollment_token(&token).await,
            Err(StoreError::Conflict(_))
        ),
        "token hashes are unique"
    );
    let missing_host = EnrollmentToken {
        host: HostId::new(),
        token_hash: [2; 32],
        expires_at: at(3600),
    };
    assert_eq!(
        repo.create_enrollment_token(&missing_host).await,
        Err(StoreError::NotFound)
    );

    assert_eq!(
        repo.consume_enrollment_token(&[1; 32], at(60)).await?,
        target.id
    );
    assert_eq!(
        repo.consume_enrollment_token(&[1; 32], at(61)).await,
        Err(StoreError::NotFound),
        "a token is single use"
    );
    assert_eq!(
        repo.consume_enrollment_token(&[9; 32], at(61)).await,
        Err(StoreError::NotFound)
    );
    Ok(())
}

/// A token is valid while `now < expires_at` and never after.
pub async fn expired_token_is_rejected(repo: &dyn HostRepo) -> ContractResult {
    let target = host("enroll-late")?;
    repo.upsert_host(&target).await?;
    for (hash, expires_at) in [([3; 32], at(100)), ([4; 32], at(101))] {
        repo.create_enrollment_token(&EnrollmentToken {
            host: target.id,
            token_hash: hash,
            expires_at,
        })
        .await?;
    }
    assert_eq!(
        repo.consume_enrollment_token(&[3; 32], at(100)).await,
        Err(StoreError::NotFound),
        "a token expires at expires_at"
    );
    assert_eq!(
        repo.consume_enrollment_token(&[4; 32], at(100)).await?,
        target.id
    );
    Ok(())
}
