//! The sandboxed executor against real binaries. Linux only.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use domovoi_core::ports::{ProcessError, ProcessExecutor};
use jiff::SignedDuration;

use crate::helpers::{SECRET, any_bin, bin, chmod, executor, install_script, spec};

#[tokio::test]
async fn arguments_with_shell_metacharacters_arrive_verbatim() {
    let Some(printf) = any_bin(&["/usr/bin/printf"]) else {
        return;
    };
    let args = [
        "; rm -rf /",
        "$(id)",
        "a b",
        "`whoami`",
        "*",
        "'\"",
        "&& true",
    ];
    let mut argv = vec!["%s\\n"];
    argv.extend(args);
    let out = executor().run(spec(&printf, &argv)).await.unwrap();
    assert_eq!(out.exit_code, Some(0), "{out:?}");
    let lines: Vec<&str> = out.stdout.lines().collect();
    assert_eq!(lines, args);
}

#[tokio::test]
async fn environment_is_cleared() {
    let Some(env) = bin("/usr/bin/env") else {
        return;
    };
    let mut spec = spec(&env, &[]);
    spec.env = BTreeMap::from([
        ("DOMOVOI_PARAM_X".to_owned(), "1".to_owned()),
        ("LD_PRELOAD".to_owned(), "/tmp/evil.so".to_owned()),
        ("lower".to_owned(), "no".to_owned()),
        ("PATH".to_owned(), "/tmp".to_owned()),
    ]);
    let out = executor().run(spec).await.unwrap();
    let mut keys: Vec<&str> = out
        .stdout
        .lines()
        .filter_map(|l| l.split_once('=').map(|(k, _)| k))
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, ["DOMOVOI_PARAM_X", "HOME", "LANG", "PATH"]);
    assert!(
        out.stdout.contains("PATH=/usr/local/sbin:"),
        "{}",
        out.stdout
    );
    assert!(out.stdout.contains("HOME=/nonexistent"));
}

fn sleepers_alive() -> bool {
    std::fs::read_dir("/proc").unwrap().flatten().any(|entry| {
        std::fs::read(entry.path().join("cmdline"))
            .map(|cmd| String::from_utf8_lossy(&cmd).contains("29.753"))
            .unwrap_or(false)
    })
}

#[tokio::test]
async fn timeout_kills_the_process_group() {
    if bin("/bin/sleep").is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let script = install_script(dir.path(), "spawn_sleeper");
    let mut spec = spec(script.to_str().unwrap(), &[]);
    spec.timeout = SignedDuration::from_millis(300);

    let started = Instant::now();
    let out = executor().run(spec).await.unwrap();
    assert!(out.timed_out, "{out:?}");
    assert_eq!(out.exit_code, None);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );

    // SIGKILL is delivered asynchronously; give the kernel a moment to reap.
    for _ in 0..50 {
        if !sleepers_alive() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("a process from the timed-out group survived");
}

#[tokio::test]
async fn output_cap_truncates_and_stops() {
    let Some(yes) = bin("/usr/bin/yes") else {
        return;
    };
    let mut spec = spec(&yes, &[]);
    spec.max_output_bytes = 4096;
    let started = Instant::now();
    let out = executor().run(spec).await.unwrap();
    assert!(out.truncated);
    assert!(!out.timed_out);
    assert_eq!(out.stdout.len(), 4096);
    assert!(out.signal.is_some(), "{out:?}");
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[tokio::test]
async fn shells_are_refused() {
    let exec = executor();
    for shell in ["/bin/sh", "/bin/bash", "/usr/bin/dash", "/bin/busybox"] {
        assert_eq!(
            exec.run(spec(shell, &["-c", "id"])).await,
            Err(ProcessError::ShellRefused),
            "{shell}"
        );
    }
    let dir = tempfile::tempdir().unwrap();
    if let Some(env) = bin("/usr/bin/env") {
        let link = dir.path().join("sh");
        std::os::unix::fs::symlink(&env, &link).unwrap();
        assert_eq!(
            exec.run(spec(link.to_str().unwrap(), &[])).await,
            Err(ProcessError::ShellRefused)
        );
    }
    if let Some(bash) = bin("/bin/bash") {
        let link = dir.path().join("innocent");
        std::os::unix::fs::symlink(&bash, &link).unwrap();
        assert_eq!(
            exec.run(spec(link.to_str().unwrap(), &["-c", "id"])).await,
            Err(ProcessError::ShellRefused)
        );
    }
}

#[tokio::test]
async fn relative_program_is_refused() {
    let exec = executor();
    for program in ["printf", "./printf", "bin/true", ""] {
        assert_eq!(
            exec.run(spec(program, &[])).await,
            Err(ProcessError::NotFound),
            "{program:?}"
        );
    }
}

#[tokio::test]
async fn missing_and_non_executable_programs_are_refused() {
    let exec = executor();
    assert_eq!(
        exec.run(spec("/nonexistent/program", &[])).await,
        Err(ProcessError::NotFound)
    );
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("data");
    std::fs::write(&file, "not a program").unwrap();
    chmod(&file, 0o644);
    assert_eq!(
        exec.run(spec(file.to_str().unwrap(), &[])).await,
        Err(ProcessError::NotExecutable)
    );
    assert_eq!(
        exec.run(spec(dir.path().to_str().unwrap(), &[])).await,
        Err(ProcessError::NotExecutable)
    );
}

#[tokio::test]
async fn output_is_redacted() {
    let Some(printf) = bin("/usr/bin/printf") else {
        return;
    };
    let line = format!("token={SECRET}");
    let out = executor()
        .run(spec(&printf, &["%s\\n", &line, ">&2"]))
        .await
        .unwrap();
    assert_eq!(out.stdout, "token=[REDACTED]\n>&2\n");
    assert!(!out.stdout.contains(SECRET));
}

#[tokio::test]
async fn stdin_is_written_then_closed() {
    let Some(cat) = any_bin(&["/bin/cat", "/usr/bin/cat"]) else {
        return;
    };
    let mut spec = spec(&cat, &[]);
    spec.stdin = Some("hello\nworld\n".to_owned());
    let out = executor().run(spec).await.unwrap();
    assert_eq!(out.stdout, "hello\nworld\n");
    assert_eq!(out.exit_code, Some(0));
}

#[tokio::test]
async fn exit_code_and_signal_are_reported() {
    let exec = executor();
    if let Some(f) = any_bin(&["/bin/false", "/usr/bin/false"]) {
        let out = exec.run(spec(&f, &[])).await.unwrap();
        assert_eq!((out.exit_code, out.signal), (Some(1), None));
    }
    if let Some(sleep) = bin("/bin/sleep") {
        let mut spec = spec(&sleep, &["30"]);
        spec.timeout = SignedDuration::from_millis(200);
        let out = exec.run(spec).await.unwrap();
        assert!(out.timed_out);
        assert_eq!((out.exit_code, out.signal), (None, Some(15)));
    }
}

#[tokio::test]
async fn working_directory_defaults_to_root() {
    let Some(pwd) = any_bin(&["/bin/pwd", "/usr/bin/pwd"]) else {
        return;
    };
    let out = executor().run(spec(&pwd, &[])).await.unwrap();
    assert_eq!(out.stdout.trim(), "/");
}

#[tokio::test]
async fn run_as_other_user_without_root_fails_clearly() {
    if nix::unistd::geteuid().is_root() {
        eprintln!("skipping: running as root");
        return;
    }
    let Some(t) = any_bin(&["/bin/true", "/usr/bin/true"]) else {
        return;
    };
    let mut spec = spec(&t, &[]);
    spec.run_as = Some("root".to_owned());
    match executor().run(spec).await {
        Err(ProcessError::UserSwitch(message)) => {
            assert!(message.contains("agent must run as root"), "{message}");
        }
        other => panic!("expected UserSwitch, got {other:?}"),
    }
}

#[tokio::test]
async fn run_as_switches_user_and_drops_groups() {
    if !nix::unistd::geteuid().is_root() {
        eprintln!("skipping: needs root");
        return;
    }
    let Some(id) = bin("/usr/bin/id") else {
        return;
    };
    let Ok(Some(nobody)) = nix::unistd::User::from_name("nobody") else {
        eprintln!("skipping: no user nobody");
        return;
    };
    let mut spec = spec(&id, &[]);
    spec.run_as = Some("nobody".to_owned());
    let out = executor().run(spec).await.unwrap();
    assert!(
        out.stdout.contains(&format!("uid={}(", nobody.uid)),
        "{}",
        out.stdout
    );
    let groups = out.stdout.trim().rsplit_once("groups=").unwrap().1;
    let ids: Vec<&str> = groups
        .split(',')
        .map(|g| g.split('(').next().unwrap())
        .collect();
    assert_eq!(
        ids,
        [nobody.gid.to_string()],
        "supplementary groups leaked: {}",
        out.stdout
    );
}

#[tokio::test]
async fn unknown_user_is_reported() {
    let Some(t) = any_bin(&["/bin/true", "/usr/bin/true"]) else {
        return;
    };
    let mut spec = spec(&t, &[]);
    spec.run_as = Some("no_such_user_domovoi".to_owned());
    assert!(matches!(
        executor().run(spec).await,
        Err(ProcessError::UserSwitch(m)) if m.contains("unknown user")
    ));
}
