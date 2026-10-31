//! Contract for [`HostRepo`].

use domovoi_core::ports::{EnrollmentToken, HostRepo, StoreError};
use domovoi_core::{AgentId, AgentIdentity, HostId};

use super::ContractResult;
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

    let first = agent(box1.id);
    repo.put_agent(&first).await?;
    assert_eq!(repo.get_agent(first.agent).await?, Some(first.clone()));

    let mut upgraded = first.clone();
    upgraded.version = "0.2.0".to_owned();
    repo.put_agent(&upgraded).await?;
    assert_eq!(repo.get_agent(first.agent).await?, Some(upgraded));

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
    repo.put_agent(&second).await?;
    assert_eq!(
        repo.revoke_agent(AgentId::new(), T0).await,
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
