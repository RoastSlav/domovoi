//! Contracts for searching and comparing summarised operations.

use domovoi_core::experience::{
    ExperienceScope, MatchPass, ResolutionOutcome, SimilarQuery, SimilarReason, SummaryField,
    SummarySource,
};
use domovoi_core::ports::OperationWrite;
use domovoi_core::{CheckKey, HostId, OperationId, OperationStatus, TargetKey};
use serde_json::json;

use super::experience::{
    ExperienceSubject, Spec, bare, commit, finish, ids, operation_of, revision, search,
};
use super::{ContractError, ContractResult};

fn setup(error: impl std::fmt::Display) -> ContractError {
    ContractError::Setup(error.to_string())
}

/// A host search returns only operations whose host set is exactly that host.
pub async fn search_is_host_scoped(s: &dyn ExperienceSubject) -> ContractResult {
    let (a, b) = (HostId::new(), HostId::new());
    let mut only_a = Spec::new(a, "one", 10);
    only_a.narrative = [Some("pool degraded"), None, None];
    let mut only_b = Spec::new(b, "two", 20);
    only_b.narrative = [Some("pool degraded"), None, None];
    let mut both = Spec::new(a, "three", 30);
    both.hosts = vec![a, b];
    both.narrative = [Some("pool degraded"), None, None];
    let (ida, idb, idab) = (
        finish(s, &only_a).await?,
        finish(s, &only_b).await?,
        finish(s, &both).await?,
    );
    let on_a = search(s, ExperienceScope::Host(a), "degraded", None).await?;
    assert_eq!(ids(&on_a), [ida]);
    let on_b = search(s, ExperienceScope::Host(b), "degraded", None).await?;
    assert_eq!(ids(&on_b), [idb]);
    let all = ids(&search(s, ExperienceScope::Global, "degraded", None).await?);
    assert_eq!(all.len(), 3);
    for id in [ida, idb, idab] {
        assert!(all.contains(&id));
    }
    Ok(())
}

/// Operations matching every word come before those matching some.
pub async fn search_returns_all_word_matches_before_any_word_matches(
    s: &dyn ExperienceSubject,
) -> ContractResult {
    let host = HostId::new();
    let mut one = Spec::new(host, "g1", 10);
    one.narrative = [Some("zfs scrub stuck"), None, None];
    let mut two = Spec::new(host, "g2", 20);
    two.narrative = [Some("zfs pool"), None, None];
    let mut three = Spec::new(host, "g3", 30);
    three.narrative = [Some("plex only"), None, None];
    let (first, second, third) = (
        finish(s, &one).await?,
        finish(s, &two).await?,
        finish(s, &three).await?,
    );
    let hits = search(s, ExperienceScope::Host(host), "zfs scrub", None).await?;
    assert_eq!(
        hits,
        [(first, MatchPass::AllWords), (second, MatchPass::AnyWord)]
    );
    assert!(!ids(&hits).contains(&third));
    Ok(())
}

/// No words lists the newest operations first.
pub async fn search_with_no_words_lists_the_newest_first(
    s: &dyn ExperienceSubject,
) -> ContractResult {
    let host = HostId::new();
    let mut made = Vec::new();
    for (secs, goal) in [(10, "a1"), (20, "a2"), (30, "a3")] {
        made.push(finish(s, &Spec::new(host, goal, secs)).await?);
    }
    made.reverse();
    for text in ["", "  ", "***"] {
        let hits = search(s, ExperienceScope::Host(host), text, None).await?;
        assert_eq!(ids(&hits), made, "query {text:?}");
    }
    Ok(())
}

/// Operators in a query are plain words.
pub async fn search_never_reads_user_text_as_syntax(s: &dyn ExperienceSubject) -> ContractResult {
    let host = HostId::new();
    let mut spec = Spec::new(host, "syntax", 10);
    spec.narrative = [Some("zfs pool"), None, None];
    let id = finish(s, &spec).await?;
    for text in ["NEAR(", "a OR b", "title:x", "^x", "\"unbalanced", "x AND"] {
        search(s, ExperienceScope::Host(host), text, None).await?;
    }
    let hits = search(s, ExperienceScope::Host(host), "zfs*", None).await?;
    assert_eq!(ids(&hits), [id]);
    Ok(())
}

/// `exclude` removes that operation.
pub async fn search_excludes_the_given_operation(s: &dyn ExperienceSubject) -> ContractResult {
    let host = HostId::new();
    let mut one = Spec::new(host, "e1", 10);
    one.narrative = [Some("disk full"), None, None];
    let mut two = Spec::new(host, "e2", 20);
    two.narrative = [Some("disk full"), None, None];
    let (first, second) = (finish(s, &one).await?, finish(s, &two).await?);
    let hits = search(s, ExperienceScope::Host(host), "disk", Some(second)).await?;
    assert_eq!(ids(&hits), [first]);
    Ok(())
}

/// After an edit only the new text matches.
pub async fn search_sees_only_the_current_text(s: &dyn ExperienceSubject) -> ContractResult {
    let host = HostId::new();
    let mut spec = Spec::new(host, "current", 10);
    spec.narrative = [Some("zfs degraded"), None, None];
    let id = finish(s, &spec).await?;
    let edit = revision(
        id,
        SummaryField::Problem,
        1,
        json!("kernel panic"),
        SummarySource::User,
        40,
        None,
    );
    commit(s, vec![edit]).await?;
    assert!(
        search(s, ExperienceScope::Host(host), "zfs", None)
            .await?
            .is_empty()
    );
    let hits = search(s, ExperienceScope::Host(host), "kernel", None).await?;
    assert_eq!(ids(&hits), [id]);
    Ok(())
}

/// Outcome words are searchable.
pub async fn search_finds_outcome_tags(s: &dyn ExperienceSubject) -> ContractResult {
    let host = HostId::new();
    let mut spec = Spec::new(host, "tagged", 10);
    spec.tags = vec!["verification_failed", "retried"];
    let id = finish(s, &spec).await?;
    finish(s, &Spec::new(host, "plain", 20)).await?;
    let hits = search(s, ExperienceScope::Host(host), "retried", None).await?;
    assert_eq!(ids(&hits), [id]);
    Ok(())
}

fn similar_query(host: HostId) -> SimilarQuery {
    SimilarQuery {
        host,
        check: CheckKey::parse("container.down").ok(),
        target: TargetKey::parse("plex").ok(),
        text: "plex crashing".to_owned(),
        contracts: vec!["skill.restart_container".to_owned()],
        exclude: None,
        limit: 10,
    }
}

/// Check, target, contract and word matches score as documented and sort best first.
pub async fn similar_scores_check_target_contract_and_words(
    s: &dyn ExperienceSubject,
) -> ContractResult {
    let host = HostId::new();
    let mut best = Spec::new(host, "restart job", 10);
    best.contracts = vec!["skill.restart_container"];
    let mut check_only = Spec::new(host, "other job", 20);
    check_only.target = Some("jellyfin");
    let mut target_only = Spec::new(host, "third job", 30);
    target_only.check = "disk.full";
    let mut words_only = Spec::new(host, "fourth job", 40);
    words_only.check = "disk.full";
    words_only.target = Some("x");
    words_only.narrative = [Some("plex keeps crashing"), None, None];
    let mut unrelated = Spec::new(host, "fifth job", 50);
    unrelated.check = "disk.full";
    unrelated.target = Some("y");
    unrelated.contracts = vec!["skill.restart_container"];
    let (a, b, c, d) = (
        finish(s, &best).await?,
        finish(s, &check_only).await?,
        finish(s, &target_only).await?,
        finish(s, &words_only).await?,
    );
    let e = finish(s, &unrelated).await?;
    let hits = s.experience().similar(&similar_query(host)).await?;
    let order: Vec<_> = hits.iter().map(|h| h.summary.operation).collect();
    assert_eq!(order, [a, b, d, c]);
    assert!(!order.contains(&e));
    assert_eq!(hits[0].score, 14);
    assert_eq!(
        hits[0].reasons,
        [
            SimilarReason::SameCheck,
            SimilarReason::SameTarget,
            SimilarReason::SameContract("skill.restart_container".to_owned())
        ]
    );
    assert_eq!(hits[1].score, 8);
    assert_eq!(hits[2].reasons, [SimilarReason::SimilarWords]);
    assert_eq!(hits[3].reasons, [SimilarReason::SameTarget]);
    Ok(())
}

/// Another host's perfect match and the excluded operation never come back.
pub async fn similar_stays_on_one_host_and_never_returns_the_excluded_operation(
    s: &dyn ExperienceSubject,
) -> ContractResult {
    let (host, other) = (HostId::new(), HostId::new());
    let near = finish(s, &Spec::new(host, "near", 10)).await?;
    let near2 = finish(s, &Spec::new(host, "near two", 20)).await?;
    let far = finish(s, &Spec::new(other, "far", 30)).await?;
    let hits = s.experience().similar(&similar_query(host)).await?;
    let found: Vec<_> = hits.iter().map(|h| h.summary.operation).collect();
    assert!(found.contains(&near) && found.contains(&near2));
    assert!(!found.contains(&far));
    let mut query = similar_query(host);
    query.exclude = Some(near);
    let hits = s.experience().similar(&query).await?;
    assert!(hits.iter().all(|h| h.summary.operation != near));
    assert_eq!(hits.len(), 1);
    Ok(())
}

/// Only finished operations with no revision are listed, oldest first.
pub async fn undrafted_lists_only_finished_operations_with_no_revision(
    s: &dyn ExperienceSubject,
) -> ContractResult {
    let host = HostId::new();
    let running = operation_of(&Spec::new(host, "open", 5), OperationStatus::Running)?;
    commit(
        s,
        vec![OperationWrite::Operation {
            operation: running,
            expected: domovoi_core::ports::Expected::New,
        }],
    )
    .await?;
    let late = bare(s, &Spec::new(host, "late", 30)).await?;
    let early = bare(s, &Spec::new(host, "early", 10)).await?;
    finish(s, &Spec::new(host, "drafted", 20)).await?;
    assert_eq!(s.experience().undrafted(10).await?, [early.id, late.id]);
    assert_eq!(s.experience().undrafted(1).await?, [early.id]);
    Ok(())
}

/// Briefs cover only operations that have a summary.
pub async fn briefs_cover_only_operations_that_have_a_summary(
    s: &dyn ExperienceSubject,
) -> ContractResult {
    let host = HostId::new();
    let done = finish(s, &Spec::new(host, "done", 10)).await?;
    let bare_op = bare(s, &Spec::new(host, "bare", 20)).await?;
    let briefs = s
        .experience()
        .briefs(&[done, bare_op.id, OperationId::new()])
        .await?;
    assert_eq!(briefs.len(), 1);
    let brief = briefs.get(&done).ok_or_else(|| setup("missing brief"))?;
    assert_eq!(brief.outcome, ResolutionOutcome::Succeeded);
    assert!(brief.verified);
    assert!(s.experience().briefs(&[]).await?.is_empty());
    Ok(())
}
