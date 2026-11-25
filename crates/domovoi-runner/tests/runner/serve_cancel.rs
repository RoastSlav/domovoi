//! Cancelling a running skill through the connection. Linux only.

use std::collections::BTreeMap;

use domovoi_core::ports::ProcessExecutor;
use domovoi_core::skill_runner::{RunnerReply, RunnerRequest};
use jiff::SignedDuration;
use tokio::io::AsyncWriteExt;

use crate::helpers::{bin, executor, spec};
use crate::serve_common::{
    AGENT_UID, Conn, Env, SkillSpec, connect, finish, read_reply, roundtrip, run_request, send,
    wait_for_file, wait_gone, write_skill,
};

const LISTEN_INNER: &str = "DOMOVOI_SERVE_LISTEN_INNER";

/// A skill that records its pid and group, starts a grandchild in the same group, and waits.
struct Sleeper {
    conn: Conn,
    pid: u32,
    pgid: u32,
    grandchild: u32,
}

async fn start_sleeper(env: &Env, tag: &str) -> Sleeper {
    let grand = env.marker(&format!("{tag}.grand"));
    let ready = env.marker(&format!("{tag}.ready"));
    write_skill(
        env.skills.path(),
        tag,
        &SkillSpec::new(&format!(
            "/bin/sleep 29.7631 &\necho $! > {grand}\n\
             echo \"$$ $(cut -d' ' -f5 /proc/$$/stat)\" > {ready}\nwait",
            grand = grand.display(),
            ready = ready.display(),
        )),
    )
    .await;
    let mut conn = connect(env.ctx(), Some(AGENT_UID));
    send(
        &mut conn.client,
        &RunnerRequest::Run(run_request(env.skills.path(), tag, &[])),
    )
    .await;
    let ready_text = wait_for_file(&ready).await;
    let (pid, pgid) = ready_text.trim().split_once(' ').unwrap();
    let grandchild = wait_for_file(&grand).await;
    Sleeper {
        conn,
        pid: pid.parse().unwrap(),
        pgid: pgid.parse().unwrap(),
        grandchild: grandchild.trim().parse().unwrap(),
    }
}

#[tokio::test]
async fn runner_kills_skill_group_on_eof_and_exits_zero() {
    if bin("/bin/sleep").is_none() {
        return;
    }
    let env = Env::new();
    let sleeper = start_sleeper(&env, "eof_cancel").await;
    let Conn { client, server } = sleeper.conn;
    drop(client);
    finish(server).await;
    wait_gone(sleeper.pid).await;
    wait_gone(sleeper.grandchild).await;
}

#[tokio::test]
async fn runner_cancels_on_half_close_and_on_stray_byte() {
    if bin("/bin/sleep").is_none() {
        return;
    }
    let env = Env::new();

    let mut half = start_sleeper(&env, "half_close").await;
    half.conn.client.shutdown().await.unwrap();
    assert!(
        read_reply(&mut half.conn.client).await.is_none(),
        "a cancelled run answered"
    );
    finish(half.conn.server).await;
    wait_gone(half.pid).await;
    wait_gone(half.grandchild).await;

    let mut stray = start_sleeper(&env, "stray_byte").await;
    stray.conn.client.write_all(b"x").await.unwrap();
    assert!(read_reply(&mut stray.conn.client).await.is_none());
    finish(stray.conn.server).await;
    wait_gone(stray.pid).await;
    wait_gone(stray.grandchild).await;
}

#[tokio::test]
async fn runner_does_not_signal_its_own_group() {
    if bin("/bin/sleep").is_none() {
        return;
    }
    let env = Env::new();
    let own = nix::unistd::getpgrp().as_raw();
    let sleeper = start_sleeper(&env, "own_group").await;
    // The signalled group is the skill's own: it leads a group that is not the runner's.
    assert_eq!(sleeper.pgid, sleeper.pid);
    assert_ne!(i64::from(sleeper.pgid), i64::from(own));
    let Conn { client, server } = sleeper.conn;
    drop(client);
    finish(server).await;
    wait_gone(sleeper.pid).await;
    // This process and its group are still here.
    assert_eq!(nix::unistd::getpgrp().as_raw(), own);
}

/// A skill never inherits socket activation variables or any connection, even while the
/// runner holds both.
///
/// The test re-runs itself in a child whose environment holds `LISTEN_FDS` and `LISTEN_PID`
/// (setting the test process's own environment would need `unsafe`); the inner run serves a
/// connection and inspects what the skill sees.
#[tokio::test]
async fn serve_spawned_skill_gets_no_listen_vars_and_no_connection() {
    if bin("/usr/bin/env").is_none() || bin("/usr/bin/ls").is_none() {
        return;
    }
    if std::env::var_os(LISTEN_INNER).is_none() {
        let name = format!(
            "{}::serve_spawned_skill_gets_no_listen_vars_and_no_connection",
            module_path!().split_once("::").map_or("", |(_, rest)| rest)
        );
        let exe = std::env::current_exe().unwrap();
        let mut rerun = spec(
            exe.to_str().unwrap(),
            &["--exact", &name, "--test-threads=1"],
        );
        rerun.timeout = SignedDuration::from_secs(60);
        rerun.max_output_bytes = 1024 * 1024;
        rerun.env = BTreeMap::from([
            (LISTEN_INNER.to_owned(), "1".to_owned()),
            ("LISTEN_FDS".to_owned(), "1".to_owned()),
            ("LISTEN_PID".to_owned(), std::process::id().to_string()),
        ]);
        let out = executor().run(rerun).await.unwrap();
        let text = format!("{}{}", out.stdout, out.stderr);
        assert_eq!(out.exit_code, Some(0), "{text}");
        assert!(
            text.contains("1 passed"),
            "the inner run did not execute: {text}"
        );
        return;
    }
    assert_eq!(std::env::var("LISTEN_FDS").as_deref(), Ok("1"));
    let env = Env::new();
    write_skill(
        env.skills.path(),
        "fd_probe",
        &SkillSpec {
            max_output_bytes: 65_536,
            ..SkillSpec::new("env\nls -l /proc/self/fd")
        },
    )
    .await;
    let request = run_request(env.skills.path(), "fd_probe", &[]);
    let RunnerReply::Run(run) = roundtrip(&env, RunnerRequest::Run(request)).await else {
        panic!("no run reply");
    };
    assert_eq!(run.exit_code, Some(0), "{run:?}");
    assert!(!run.stdout.contains("LISTEN_"), "{}", run.stdout);
    assert!(!run.stdout.contains("socket:"), "{}", run.stdout);
    assert!(run.stdout.contains("0 -> /dev/null"), "{}", run.stdout);
}
