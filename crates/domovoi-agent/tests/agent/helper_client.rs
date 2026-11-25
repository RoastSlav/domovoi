//! The helper client, the state hand-over and the `agent.toml` mode, against a stub helper
//! on a socket in a temporary directory.

#![cfg(unix)]

use std::path::Path;
use std::sync::{Arc, Mutex};

use domovoi_agent::helper_client::{HelperClient, HelperClientError, probe_status};
use domovoi_agent::link::STATUS_PROBE_INTERVAL;
use domovoi_agent::link::handshake::AgentStatusSource;
use domovoi_agent::state_owner::{StateOwner, StateOwnerError, hand_over, hand_over_with};
use domovoi_core::helper::{
    HELPER_PROTOCOL, HelperGroup, HelperRequest, HelperResponse, HelperResponseFrame,
    decode_request, encode_frame, frame_len,
};
use domovoi_core::identity::RefusalToken;
use domovoi_core::isolation::{PlatformRecordState, PlatformTier};
use domovoi_proto::messages::HelperStatus;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;
use tokio_util::sync::CancellationToken;

use crate::harness::{clock, settle};

/// What the stub answers for a request, and the protocol number it puts on the frame.
type Script = Arc<dyn Fn(&HelperRequest) -> (u16, HelperResponse) + Send + Sync>;

fn health(protocol: u16) -> HelperResponse {
    HelperResponse::Health {
        protocol,
        build: "test".to_owned(),
        pid: 7,
    }
}

fn groups(enabled: Vec<HelperGroup>) -> HelperResponse {
    HelperResponse::Groups {
        enabled,
        ignored: Vec::new(),
        write_opt_in: false,
        platform_tier: PlatformTier::Blocked,
        platform_record: PlatformRecordState::Missing,
    }
}

fn healthy(enabled: Vec<HelperGroup>) -> Script {
    Arc::new(move |request| match request {
        HelperRequest::Groups => (HELPER_PROTOCOL, groups(enabled.clone())),
        _ => (HELPER_PROTOCOL, health(HELPER_PROTOCOL)),
    })
}

/// Serves one request per connection on `socket` until the test ends.
fn serve_stub(socket: &Path, script: Script) -> tokio::task::JoinHandle<()> {
    let listener = UnixListener::bind(socket).unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let mut header = [0_u8; 4];
            if stream.read_exact(&mut header).await.is_err() {
                continue;
            }
            let mut body =
                vec![0_u8; frame_len(header, domovoi_core::helper::MAX_REQUEST_BYTES).unwrap()];
            if stream.read_exact(&mut body).await.is_err() {
                continue;
            }
            let frame = decode_request(&body).unwrap();
            let (v, response) = script(&frame.request);
            let answer = HelperResponseFrame {
                v,
                id: frame.id,
                response,
            };
            let _ = stream.write_all(&encode_frame(&answer).unwrap()).await;
        }
    })
}

#[tokio::test]
async fn helper_status_is_absent_without_a_socket() {
    let dir = tempfile::tempdir().unwrap();
    let client = HelperClient::new(dir.path().join("none.sock"));
    assert_eq!(probe_status(&client, &*clock()).await, HelperStatus::Absent);
}

#[tokio::test]
async fn helper_status_is_ok_with_enabled_group_names() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("h.sock");
    let _stub = serve_stub(
        &socket,
        healthy(vec![HelperGroup::DockerRead, HelperGroup::SystemdRead]),
    );
    let status = probe_status(&HelperClient::new(&socket), &*clock()).await;
    assert_eq!(
        status,
        HelperStatus::Ok {
            ops: vec![
                "docker.read".to_owned(),
                "systemd.read".to_owned(),
                // An enabled `systemd.read` also offers the identity read as an operation.
                "unit.identity".to_owned()
            ]
        }
    );
}

#[tokio::test]
async fn unit_restart_is_offered_only_when_the_write_group_is_named_and_the_tier_allows() {
    let offered = |tier: PlatformTier, opt_in: bool, record: PlatformRecordState, named: bool| {
        let script: Script = Arc::new(move |request| match request {
            HelperRequest::Groups => (
                HELPER_PROTOCOL,
                HelperResponse::Groups {
                    enabled: vec![HelperGroup::SystemdRead],
                    ignored: if named {
                        vec![HelperGroup::SystemdWrite]
                    } else {
                        Vec::new()
                    },
                    write_opt_in: opt_in,
                    platform_tier: tier,
                    platform_record: record,
                },
            ),
            _ => (HELPER_PROTOCOL, health(HELPER_PROTOCOL)),
        });
        async move {
            let dir = tempfile::tempdir().unwrap();
            let socket = dir.path().join("h.sock");
            let _stub = serve_stub(&socket, script);
            match probe_status(&HelperClient::new(&socket), &*clock()).await {
                HelperStatus::Ok { ops } => ops.contains(&"unit.restart".to_owned()),
                other => panic!("{other:?}"),
            }
        }
    };
    use PlatformRecordState::{Missing, Ok as Sound};
    assert!(offered(PlatformTier::Supported, false, Sound, true).await);
    assert!(offered(PlatformTier::Untested, true, Sound, true).await);
    assert!(!offered(PlatformTier::Untested, false, Sound, true).await);
    assert!(!offered(PlatformTier::Blocked, true, Sound, true).await);
    assert!(!offered(PlatformTier::Supported, true, Missing, true).await);
    assert!(!offered(PlatformTier::Supported, true, Sound, false).await);
}

#[tokio::test]
async fn helper_status_is_error_with_token_on_refusal() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("h.sock");
    let _stub = serve_stub(
        &socket,
        Arc::new(|_| {
            (
                HELPER_PROTOCOL,
                HelperResponse::Refused {
                    token: RefusalToken::PeerNotAgent,
                },
            )
        }),
    );
    let client = HelperClient::new(&socket);
    let status = probe_status(&client, &*clock()).await;
    assert_eq!(
        status,
        HelperStatus::Error {
            reason: "peer_not_agent".to_owned()
        }
    );

    let other = dir.path().join("s.sock");
    let _stub = serve_stub(
        &other,
        Arc::new(|_| {
            (
                HELPER_PROTOCOL,
                HelperResponse::StartRefused {
                    token: RefusalToken::HelperStatePermissions,
                    faults: Vec::new(),
                },
            )
        }),
    );
    let status = probe_status(&HelperClient::new(&other), &*clock()).await;
    assert_eq!(
        status,
        HelperStatus::Error {
            reason: "helper_state_permissions".to_owned()
        }
    );
}

#[tokio::test]
async fn helper_status_changes_within_one_probe_interval_on_the_fake_clock() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("h.sock");
    let clock = clock();
    let status = AgentStatusSource::default();
    let stop = CancellationToken::new();
    let probe = tokio::spawn(domovoi_agent::helper_client::keep_status(
        HelperClient::new(&socket),
        clock.clone(),
        status.clone(),
        stop.clone(),
    ));
    settle().await;
    assert_eq!(
        status.current().unwrap().helper_status,
        HelperStatus::Absent
    );

    // The helper starts after the agent.
    let _stub = serve_stub(&socket, healthy(vec![HelperGroup::KernelRead]));
    clock.advance(STATUS_PROBE_INTERVAL);
    for _ in 0..20 {
        settle().await;
        if status.current().unwrap().helper_status != HelperStatus::Absent {
            break;
        }
    }
    assert_eq!(
        status.current().unwrap().helper_status,
        HelperStatus::Ok {
            ops: vec!["kernel.read".to_owned()]
        }
    );
    stop.cancel();
    probe.await.unwrap();
}

#[tokio::test]
async fn helper_client_refuses_another_protocol_number() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("frame.sock");
    let _stub = serve_stub(&socket, Arc::new(|_| (2, health(2))));
    let client = HelperClient::new(&socket);
    assert_eq!(
        client.health(&*clock()).await.unwrap_err(),
        HelperClientError::Protocol(2)
    );
    assert_eq!(
        probe_status(&client, &*clock()).await,
        HelperStatus::Error {
            reason: "protocol".to_owned()
        }
    );

    // A frame at version 1 whose health body names another protocol is refused too.
    let body = dir.path().join("body.sock");
    let _stub = serve_stub(&body, Arc::new(|_| (HELPER_PROTOCOL, health(2))));
    assert_eq!(
        probe_status(&HelperClient::new(&body), &*clock()).await,
        HelperStatus::Error {
            reason: "protocol".to_owned()
        }
    );
}

fn current_owner(path: &Path) -> StateOwner {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(path).unwrap();
    StateOwner {
        uid: meta.uid(),
        gid: meta.gid(),
    }
}

fn mode_of(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::symlink_metadata(path)
        .unwrap()
        .permissions()
        .mode()
        & 0o7777
}

fn state_tree(root: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(root.join("journal/sub")).unwrap();
    std::fs::write(root.join("identity.json"), "{}").unwrap();
    std::fs::write(root.join("journal/sub/log"), "x").unwrap();
    std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::set_permissions(
        root.join("identity.json"),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();
}

#[test]
fn hand_over_chowns_only_what_differs_and_never_changes_modes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("state");
    state_tree(&root);
    let here = current_owner(&root);
    let modes = [
        mode_of(&root),
        mode_of(&root.join("identity.json")),
        mode_of(&root.join("journal/sub/log")),
    ];

    // Already owned by the target: nothing is touched, however often it runs.
    for _ in 0..2 {
        let report = hand_over(&root, here).unwrap();
        assert_eq!(report.visited, 5);
        assert_eq!(report.changed, 0);
    }

    // A different target: every entry differs, each is changed once, no mode moves.
    let other = StateOwner {
        uid: here.uid + 1,
        gid: here.gid + 1,
    };
    let touched = Mutex::new(Vec::new());
    let report = hand_over_with(&root, other, &mut |path, owner| {
        assert_eq!(owner, other);
        touched.lock().unwrap().push(path.to_path_buf());
        Ok(())
    })
    .unwrap();
    assert_eq!(report.changed, 5);
    assert_eq!(touched.lock().unwrap().len(), 5);
    assert_eq!(
        modes,
        [
            mode_of(&root),
            mode_of(&root.join("identity.json")),
            mode_of(&root.join("journal/sub/log")),
        ]
    );
}

#[test]
fn hand_over_refuses_a_symlink_anywhere_in_the_tree() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("state");
    state_tree(&root);
    let other = StateOwner {
        uid: current_owner(&root).uid + 1,
        gid: 0,
    };
    std::os::unix::fs::symlink("/etc/passwd", root.join("journal/sub/link")).unwrap();
    let touched = Mutex::new(0_usize);
    let err = hand_over_with(&root, other, &mut |_, _| {
        *touched.lock().unwrap() += 1;
        Ok(())
    })
    .unwrap_err();
    assert!(matches!(err, StateOwnerError::Symlink(path) if path.ends_with("journal/sub/link")));
    assert_eq!(
        *touched.lock().unwrap(),
        0,
        "nothing changes before the refusal"
    );

    // The directory itself may not be a symlink either.
    let alias = dir.path().join("alias");
    std::os::unix::fs::symlink(&root, &alias).unwrap();
    assert!(matches!(
        hand_over(&alias, other),
        Err(StateOwnerError::Symlink(_))
    ));
}

#[cfg(target_os = "linux")]
#[test]
fn enroll_as_root_hands_the_state_dir_to_the_agent_account() {
    let Some(owner) = domovoi_agent::state_owner::resolve_agent_owner().unwrap() else {
        eprintln!("skipped: not root, or no domovoi-agent account");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("state");
    state_tree(&root);
    let report = hand_over(&root, owner).unwrap();
    assert_eq!(report.changed, report.visited);
    assert_eq!(current_owner(&root), owner);
    assert_eq!(current_owner(&root.join("journal/sub/log")), owner);
    assert_eq!(mode_of(&root), 0o700);
}

#[test]
fn agent_toml_is_written_0644() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("etc/agent.toml");
    let hub = url::Url::parse("wss://hub.lan:7421/").unwrap();
    assert!(domovoi_agent::config_file::write_if_absent(&path, "nas", &hub, "AB:CD").unwrap());
    // Created under the process umask, so compare the bits the file asked for.
    assert_eq!(mode_of(&path) & !0o022, 0o644 & !0o022);
    let new = url::Url::parse("wss://hub.lan:9999/").unwrap();
    assert!(domovoi_agent::config_file::set_hub_url(&path, &new).unwrap());
    assert_eq!(
        mode_of(&path),
        0o644,
        "the rewrite keeps it readable by the agent group"
    );
}
