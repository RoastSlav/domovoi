//! Feeding stdin, capturing capped output and enforcing the timeout.

use std::os::unix::process::ExitStatusExt;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use domovoi_core::ports::{ProcessError, ProcessSpec};
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Child;
use tokio::task::JoinHandle;

use super::TERM_GRACE;

/// How long readers may keep draining after the process group is gone.
const DRAIN_LIMIT: Duration = Duration::from_secs(1);

/// What a run produced before redaction.
pub(super) struct Captured {
    pub(super) exit_code: Option<i32>,
    pub(super) signal: Option<i32>,
    pub(super) stdout: Vec<u8>,
    pub(super) stderr: Vec<u8>,
    pub(super) truncated: bool,
    pub(super) timed_out: bool,
}

#[derive(Default)]
struct Buffer {
    data: Vec<u8>,
    truncated: bool,
}

type Shared = Arc<Mutex<Buffer>>;

fn lock(buffer: &Shared) -> std::sync::MutexGuard<'_, Buffer> {
    buffer.lock().unwrap_or_else(PoisonError::into_inner)
}

fn signal_group(pgid: Pid, signal: Signal) {
    // ESRCH means the group is already gone, which is the goal.
    let _ = killpg(pgid, signal);
}

/// Runs `child` to completion under `spec`'s timeout and output caps.
pub(super) async fn drive(mut child: Child, spec: &ProcessSpec) -> Result<Captured, ProcessError> {
    let pid = child
        .id()
        .ok_or_else(|| ProcessError::Spawn("child exited before it was tracked".to_owned()))?;
    let pgid = Pid::from_raw(
        i32::try_from(pid).map_err(|_| ProcessError::Spawn("pid out of range".to_owned()))?,
    );
    let cap = usize::try_from(spec.max_output_bytes).unwrap_or(usize::MAX);

    if let (Some(mut stdin), Some(text)) = (child.stdin.take(), spec.stdin.clone()) {
        tokio::spawn(async move {
            // A program that exits without reading its input closes the pipe; that is
            // not an error for the run.
            let _ = stdin.write_all(text.as_bytes()).await;
            let _ = stdin.shutdown().await;
        });
    }
    let out = Shared::default();
    let err = Shared::default();
    let readers = [
        child
            .stdout
            .take()
            .map(|r| reader(r, cap, out.clone(), pgid)),
        child
            .stderr
            .take()
            .map(|r| reader(r, cap, err.clone(), pgid)),
    ];

    let limit = Duration::try_from(spec.timeout).unwrap_or(Duration::ZERO);
    let mut timed_out = false;
    let status = match tokio::time::timeout(limit, child.wait()).await {
        Ok(status) => status,
        Err(_) => {
            timed_out = true;
            signal_group(pgid, Signal::SIGTERM);
            match tokio::time::timeout(TERM_GRACE, child.wait()).await {
                Ok(status) => status,
                Err(_) => {
                    signal_group(pgid, Signal::SIGKILL);
                    child.wait().await
                }
            }
        }
    }
    .map_err(|e| ProcessError::Spawn(format!("waiting for the process failed: {e}")))?;

    // Nothing in the group outlives the run, including children holding the pipes.
    signal_group(pgid, Signal::SIGKILL);
    for handle in readers.into_iter().flatten() {
        if tokio::time::timeout(DRAIN_LIMIT, handle).await.is_err() {
            tracing::warn!("output reader did not finish after the process group was killed");
        }
    }

    let (stdout, out_truncated) = take(&out);
    let (stderr, err_truncated) = take(&err);
    Ok(Captured {
        exit_code: status.code(),
        signal: status.signal(),
        stdout,
        stderr,
        truncated: out_truncated || err_truncated,
        timed_out,
    })
}

fn take(buffer: &Shared) -> (Vec<u8>, bool) {
    let mut guard = lock(buffer);
    (std::mem::take(&mut guard.data), guard.truncated)
}

fn reader<R>(mut stream: R, cap: usize, buffer: Shared, pgid: Pid) -> JoinHandle<()>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut chunk = vec![0u8; 8192];
        loop {
            let n = match stream.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            let mut guard = lock(&buffer);
            let room = cap.saturating_sub(guard.data.len());
            if n > room {
                guard.data.extend_from_slice(&chunk[..room]);
                guard.truncated = true;
                drop(guard);
                signal_group(pgid, Signal::SIGKILL);
                break;
            }
            guard.data.extend_from_slice(&chunk[..n]);
        }
    })
}
