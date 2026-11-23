//! Tests for the account evaluators, the path table and the tokens.

use std::path::PathBuf;

use super::*;
use crate::ports::TreeEntry;

const AGENT: (u32, u32) = (990, 990);

fn skill_user() -> UserEntry {
    UserEntry {
        name: SKILL_ACCOUNT.into(),
        uid: 991,
        gid: 991,
        shell: "/usr/sbin/nologin".into(),
    }
}

fn facts(kind: PathKind, mode: u32, uid: u32, gid: u32) -> PathFacts {
    PathFacts {
        kind: Some(kind),
        symlink: false,
        mode,
        uid,
        gid,
    }
}

fn spec(id: PathId) -> PathSpec {
    expected_paths(&HostDirs::default())
        .into_iter()
        .find(|s| s.id == id)
        .expect("row in table")
}

// covers: ST-02
// covers: agent_groups_unexpected
#[test]
fn agent_groups_exactly_primary_and_journal_pass() {
    assert_eq!(
        evaluate_agent_groups(&[990, 190], 990, Some(190), Some(999)),
        Ok(())
    );
    assert_eq!(evaluate_agent_groups(&[190], 990, Some(190), None), Ok(()));
    assert_eq!(evaluate_agent_groups(&[], 990, None, Some(999)), Ok(()));
}

#[test]
fn agent_groups_with_docker_or_root_fail() {
    assert_eq!(
        evaluate_agent_groups(&[990, 190, 999], 990, Some(190), Some(999)),
        Err(vec![999])
    );
    assert_eq!(evaluate_agent_groups(&[0], 990, None, None), Err(vec![0]));
    assert_eq!(
        evaluate_agent_groups(&[990], 0, None, None),
        Err(vec![0, 990])
    );
    assert_eq!(
        evaluate_agent_groups(&[990, 4242], 990, None, None),
        Err(vec![4242])
    );
    // The journal membership is required when the group exists.
    assert!(evaluate_agent_groups(&[990], 990, Some(190), None).is_err());
}

// covers: ST-03
// covers: skill_account_invalid
#[test]
fn skill_account_with_any_extra_group_fails() {
    assert_eq!(
        evaluate_skill_account(&skill_user(), 990, &[991], false),
        Ok(())
    );
    assert_eq!(
        evaluate_skill_account(&skill_user(), 990, &[991, 44], false),
        Err(SkillAccountFault::ExtraGroups { gids: vec![44] })
    );
    assert_eq!(
        evaluate_skill_account(&skill_user(), 990, &[991], true),
        Err(SkillAccountFault::ListedInGroup)
    );
}

#[test]
fn skill_account_with_login_shell_or_agent_uid_fails() {
    let mut user = skill_user();
    user.shell = "/bin/bash".into();
    assert!(matches!(
        evaluate_skill_account(&user, 990, &[991], false),
        Err(SkillAccountFault::LoginShell { .. })
    ));
    user.shell = "/bin/false".into();
    assert_eq!(evaluate_skill_account(&user, 990, &[991], false), Ok(()));
    user.uid = 990;
    assert_eq!(
        evaluate_skill_account(&user, 990, &[991], false),
        Err(SkillAccountFault::SharesAgentUid)
    );
    user.uid = 0;
    assert_eq!(
        evaluate_skill_account(&user, 990, &[991], false),
        Err(SkillAccountFault::IsRoot)
    );
}

// covers: ST-04
// covers: docker_group_member
#[test]
fn docker_membership_of_either_account_fails() {
    assert_eq!(evaluate_docker_membership(None), Ok(()));
    let mut group = GroupEntry {
        name: DOCKER_GROUP.into(),
        gid: 999,
        members: vec!["alice".into()],
    };
    assert_eq!(evaluate_docker_membership(Some(&group)), Ok(()));
    group.members.push(SKILL_ACCOUNT.into());
    assert_eq!(
        evaluate_docker_membership(Some(&group)),
        Err(vec![SKILL_ACCOUNT.to_owned()])
    );
    group.members.push(AGENT_ACCOUNT.into());
    assert_eq!(
        evaluate_docker_membership(Some(&group)),
        Err(vec![AGENT_ACCOUNT.to_owned(), SKILL_ACCOUNT.to_owned()])
    );
}

#[test]
fn path_table_matches_synthesis_5_7() {
    // Paths are rendered with `/` so the snapshot is the same on every platform.
    let mut table = serde_json::to_value(expected_paths(&HostDirs::default())).expect("serialises");
    for row in table.as_array_mut().expect("a list") {
        let path = row["path"].as_str().expect("a path").replace("\\", "/");
        row["path"] = path.into();
    }
    insta::assert_json_snapshot!(table);
}

// covers: ST-10
// covers: path_mode_wrong
#[test]
fn evaluate_path_reports_found_and_expected_mode_owner_and_kind() {
    let state = spec(PathId::AgentState);
    assert_eq!(
        evaluate_path(&state, Ok(facts(PathKind::Dir, 0o700, 990, 990)), AGENT),
        None
    );
    let fault = evaluate_path(&state, Ok(facts(PathKind::Dir, 0o755, 0, 0)), AGENT)
        .expect("a wrong mode is a fault");
    assert_eq!(fault.why, FaultKind::Mode);
    assert_eq!(fault.found, Some(facts(PathKind::Dir, 0o755, 0, 0)));
    assert_eq!(fault.expected, facts(PathKind::Dir, 0o700, 990, 990));
    assert_eq!(fault.check_token(), CheckToken::PathModeWrong);

    let owner =
        evaluate_path(&state, Ok(facts(PathKind::Dir, 0o700, 0, 990)), AGENT).expect("owner");
    assert_eq!(owner.why, FaultKind::Owner);
    let group =
        evaluate_path(&state, Ok(facts(PathKind::Dir, 0o700, 990, 0)), AGENT).expect("group");
    assert_eq!(group.why, FaultKind::Group);
    let kind =
        evaluate_path(&state, Ok(facts(PathKind::File, 0o700, 990, 990)), AGENT).expect("type");
    assert_eq!(kind.why, FaultKind::WrongType);

    let missing = evaluate_path(&state, Err(FactsError::NotFound), AGENT).expect("missing");
    assert_eq!((missing.why, missing.found), (FaultKind::Missing, None));
    let key = spec(PathId::HelperKey);
    assert_eq!(evaluate_path(&key, Err(FactsError::NotFound), AGENT), None);
}

// covers: ST-11
// covers: helper_state_permissions
#[test]
fn unopenable_helper_state_is_a_permission_fault_with_the_helper_token() {
    let lock = spec(PathId::HelperLock);
    let fault = evaluate_path(&lock, Err(FactsError::PermissionDenied), AGENT).expect("fault");
    assert_eq!(fault.why, FaultKind::Unreadable);
    assert_eq!(fault.check_token(), CheckToken::HelperStatePermissions);
    assert_eq!(
        PathFault::refusal_token(),
        RefusalToken::HelperStatePermissions
    );
    let key = spec(PathId::HelperKey);
    let loose = evaluate_path(&key, Ok(facts(PathKind::File, 0o640, 0, 0)), AGENT).expect("fault");
    assert_eq!(loose.why, FaultKind::Mode);
    assert_eq!(loose.check_token(), CheckToken::HelperStatePermissions);
}

// covers: ST-12
// covers: socket_mode_wrong
#[test]
fn world_connectable_socket_node_is_a_fault() {
    let socket = spec(PathId::HelperSocket);
    assert_eq!(
        evaluate_path(&socket, Ok(facts(PathKind::Socket, 0o660, 0, 990)), AGENT),
        None
    );
    let fault = evaluate_path(&socket, Ok(facts(PathKind::Socket, 0o666, 0, 990)), AGENT)
        .expect("0666 fails");
    assert_eq!(fault.why, FaultKind::Mode);
    assert_eq!(fault.check_token(), CheckToken::SocketModeWrong);
}

#[test]
fn symlink_at_any_listed_path_is_a_fault() {
    for row in expected_paths(&HostDirs::default()) {
        let mut found = row.expected_facts(AGENT);
        found.symlink = true;
        let fault = evaluate_path(&row, Ok(found), AGENT)
            .unwrap_or_else(|| panic!("{} accepted a symlink", row.id.as_str()));
        assert_eq!(fault.why, FaultKind::Symlink, "{}", row.id.as_str());
    }
}

#[test]
fn skills_tree_with_group_write_bit_is_a_fault() {
    let skills = spec(PathId::SkillsDir);
    let entry = |name: &str, kind, mode, uid| TreeEntry {
        path: PathBuf::from("/etc/domovoi/skills").join(name),
        facts: facts(kind, mode, uid, 0),
    };
    let clean = [
        entry("a", PathKind::Dir, 0o755, 0),
        entry("a/run.sh", PathKind::File, 0o755, 0),
        entry("a/skill.toml", PathKind::File, 0o644, 0),
    ];
    assert!(evaluate_tree(&skills, &clean).is_empty());
    let bad = [
        entry("g", PathKind::File, 0o664, 0),
        entry("o", PathKind::Dir, 0o757, 0),
        entry("u", PathKind::File, 0o644, 990),
        entry("m", PathKind::File, 0o600, 0),
    ];
    let faults = evaluate_tree(&skills, &bad);
    assert_eq!(faults.len(), 4);
    assert!(faults.iter().all(|f| f.why == FaultKind::WriteBitInTree));
    assert_eq!(faults[0].expected.mode, 0o644);

    let packages = spec(PathId::PackagesDir);
    let owned = [entry("p", PathKind::File, 0o640, 990)];
    assert!(evaluate_tree(&packages, &owned).is_empty());
    let writable = [entry("p", PathKind::Dir, 0o775, 990)];
    assert_eq!(evaluate_tree(&packages, &writable).len(), 1);
    let mut link = entry("l", PathKind::File, 0o644, 0);
    link.facts.symlink = true;
    assert_eq!(evaluate_tree(&skills, &[link]).len(), 1);
}

#[test]
fn refusal_and_check_token_spellings_are_stable() {
    let refusals: Vec<&str> = RefusalToken::ALL.iter().map(|t| t.as_str()).collect();
    let checks: Vec<&str> = CheckToken::ALL.iter().map(|t| t.as_str()).collect();
    insta::assert_json_snapshot!((refusals, checks));
    for token in RefusalToken::ALL {
        let json = serde_json::to_string(token).expect("serialises");
        assert_eq!(json, format!("\"{}\"", token.as_str()));
    }
    for token in CheckToken::ALL {
        let json = serde_json::to_string(token).expect("serialises");
        assert_eq!(json, format!("\"{}\"", token.as_str()));
    }
}
