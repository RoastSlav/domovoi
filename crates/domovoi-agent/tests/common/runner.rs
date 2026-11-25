//! A fake skill runner: a task over a `UnixListener` in a temporary directory that reads one
//! request frame per connection and then answers, closes or stalls as a script says, recording
//! what it saw.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use domovoi_agent::skill_runner::{ExpectedDirs, RunnerClient, RunnerRoute};
use domovoi_core::ports::Clock;
use domovoi_core::skill_runner::{
    HealthReply, MAX_REQUEST_BYTES, RUNNER_PROTOCOL, Refusal, RefusalCode, RunReply, RunnerReply,
    RunnerRequest, decode_payload, encode_frame, frame_len,
};
use futures::FutureExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

/// What the fake does with a request once it has read it.
#[derive(Clone)]
pub enum Action {
    /// Answer at once.
    Reply(RunnerReply),
    /// Answer once the gate is released.
    Gated(Arc<Notify>, RunnerReply),
    /// Close the connection without an answer.
    Close,
    /// Never answer; wait for the peer to close and count it.
    Stall,
}

/// Chooses an [`Action`] per request.
pub type Script = Arc<dyn Fn(&RunnerRequest) -> Action + Send + Sync>;

/// A script that does `action` for every request.
pub fn always(action: Action) -> Script {
    Arc::new(move |_| action.clone())
}

/// A fake runner and what it has seen.
pub struct FakeRunner {
    /// The socket it listens on.
    pub socket: PathBuf,
    /// Every request frame read, in order.
    pub requests: Arc<Mutex<Vec<RunnerRequest>>>,
    /// Connections accepted.
    pub accepted: Arc<AtomicUsize>,
    /// Stalled connections whose peer closed.
    pub peer_closed: Arc<AtomicUsize>,
    /// For each answer, whether the peer's write side was still open when it was written.
    pub open_at_reply: Arc<Mutex<Vec<bool>>>,
    task: JoinHandle<()>,
}

impl Drop for FakeRunner {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl FakeRunner {
    /// Starts a fake in `dir` that follows `script`.
    pub fn start(dir: &Path, script: Script) -> Self {
        let socket = dir.join("runner.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let accepted = Arc::new(AtomicUsize::new(0));
        let peer_closed = Arc::new(AtomicUsize::new(0));
        let open_at_reply = Arc::new(Mutex::new(Vec::new()));
        let seen = Seen {
            requests: requests.clone(),
            peer_closed: peer_closed.clone(),
            open_at_reply: open_at_reply.clone(),
        };
        let counter = accepted.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                counter.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(connection(stream, script.clone(), seen.clone()));
            }
        });
        Self {
            socket,
            requests,
            accepted,
            peer_closed,
            open_at_reply,
            task,
        }
    }

    /// The requests read so far.
    pub fn seen(&self) -> Vec<RunnerRequest> {
        self.requests.lock().unwrap().clone()
    }

    /// A client for this fake.
    pub fn client(&self, clock: Arc<dyn Clock>) -> RunnerClient {
        RunnerClient::new(self.socket.clone(), clock)
    }

    /// A route over this fake that expects `skills` and `packages` as the runner's roots.
    pub fn route(&self, clock: Arc<dyn Clock>, skills: &Path, packages: &Path) -> Arc<RunnerRoute> {
        let expected = ExpectedDirs {
            skills: skills.to_path_buf(),
            packages: packages.to_path_buf(),
        };
        Arc::new(RunnerRoute::new(
            self.client(clock.clone()),
            expected,
            clock,
        ))
    }
}

#[derive(Clone)]
struct Seen {
    requests: Arc<Mutex<Vec<RunnerRequest>>>,
    peer_closed: Arc<AtomicUsize>,
    open_at_reply: Arc<Mutex<Vec<bool>>>,
}

async fn read_request(stream: &mut UnixStream) -> Option<RunnerRequest> {
    let mut header = [0_u8; 4];
    stream.read_exact(&mut header).await.ok()?;
    let len = frame_len(header, MAX_REQUEST_BYTES).ok()?;
    let mut body = vec![0_u8; len];
    stream.read_exact(&mut body).await.ok()?;
    decode_payload(&body).ok()
}

async fn connection(mut stream: UnixStream, script: Script, seen: Seen) {
    let Some(request) = read_request(&mut stream).await else {
        return;
    };
    seen.requests.lock().unwrap().push(request.clone());
    match script(&request) {
        Action::Reply(reply) => answer(&mut stream, &reply, &seen).await,
        Action::Gated(gate, reply) => {
            gate.notified().await;
            answer(&mut stream, &reply, &seen).await;
        }
        Action::Close => {}
        Action::Stall => {
            let mut byte = [0_u8; 1];
            let _ = stream.read(&mut byte).await;
            seen.peer_closed.fetch_add(1, Ordering::SeqCst);
        }
    }
}

async fn answer(stream: &mut UnixStream, reply: &RunnerReply, seen: &Seen) {
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    let mut byte = [0_u8; 1];
    // Pending means nothing was readable: the peer's write side is still open.
    let open = stream.read(&mut byte).now_or_never().is_none();
    seen.open_at_reply.lock().unwrap().push(open);
    let frame = encode_frame(reply, 16 * 1024 * 1024).unwrap();
    let _ = stream.write_all(&frame).await;
}

/// A `health` answer naming these roots.
pub fn health(skills: &Path, packages: &Path) -> RunnerReply {
    RunnerReply::Health(HealthReply {
        protocol: RUNNER_PROTOCOL,
        version: "1.2.3".to_owned(),
        skills_root: skills.to_string_lossy().into_owned(),
        packages_root: packages.to_string_lossy().into_owned(),
    })
}

/// A successful run answer with `stdout`.
pub fn run_reply(stdout: &str) -> RunReply {
    RunReply {
        exit_code: Some(0),
        signal: None,
        stdout: stdout.to_owned(),
        stderr: String::new(),
        truncated: false,
        timed_out: false,
        duration_ms: 12,
        argv_hash: "ab".repeat(32),
    }
}

/// A refusal answer.
pub fn refused(code: RefusalCode, detail: &str) -> RunnerReply {
    RunnerReply::Refused(Refusal {
        code,
        detail: detail.to_owned(),
    })
}
