//! Shared fixtures of the read tests: a connection with a chosen peer uid, a healthy scripted
//! host with the read tools installed, and a one-request exchange over a socket pair.

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use domovoi_config::parse_helper;
use domovoi_core::helper::{
    HELPER_PROTOCOL, HelperRequest, HelperRequestFrame, HelperResponse, decode_response,
    encode_frame,
};
use domovoi_core::identity::{AGENT_ACCOUNT, HostDirs, PathFacts, PathKind, UserEntry};
use domovoi_core::ports::ProcessOutput;
use domovoi_helper::serve::{Connection, ServeContext, serve_connection};
use domovoi_testkit::{FakeClock, FakeHostInspect, FakeProcessExecutor};
use jiff::Timestamp;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::UnixStream;
use uuid::Uuid;

/// The scripted uid of `domovoi-agent`.
pub const AGENT_ID: u32 = 900;

pub struct TestConn {
    pub stream: UnixStream,
    pub uid: u32,
}

impl AsyncRead for TestConn {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}

impl AsyncWrite for TestConn {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}

impl Connection for TestConn {
    fn peer_uid(&self) -> std::io::Result<u32> {
        Ok(self.uid)
    }

    fn discard_unread(&mut self) {
        self.stream.discard_unread();
    }
}

pub fn facts(kind: PathKind, mode: u32, uid: u32) -> PathFacts {
    PathFacts {
        kind: Some(kind),
        symlink: false,
        mode,
        uid,
        gid: uid,
    }
}

/// A root-owned file, mode 0755: an installed tool.
pub fn tool_facts() -> PathFacts {
    facts(PathKind::File, 0o755, 0)
}

/// A host whose start checks pass, with every read tool installed at its first candidate.
pub fn healthy(dirs: &HostDirs) -> FakeHostInspect {
    let dir = |mode| facts(PathKind::Dir, mode, 0);
    let file = |mode| facts(PathKind::File, mode, 0);
    FakeHostInspect::new()
        .ids(0, 0)
        .user(
            UserEntry {
                name: AGENT_ACCOUNT.to_owned(),
                uid: AGENT_ID,
                gid: AGENT_ID,
                shell: "/usr/sbin/nologin".to_owned(),
            },
            &[AGENT_ID],
        )
        .path(&dirs.helper_dir, dir(0o700))
        .path(dirs.helper_dir.join("state.lock"), file(0o600))
        .path(dirs.helper_dir.join("attest.key"), file(0o600))
        .path(dirs.helper_dir.join("state"), file(0o600))
        .path(dirs.etc_dir.join("helper.toml"), file(0o600))
        .text(dirs.etc_dir.join("helper.toml"), "")
        .path("/usr/sbin/smartctl", tool_facts())
        .path("/usr/bin/dmesg", tool_facts())
        .path("/usr/bin/ss", tool_facts())
}

pub fn ok(stdout: impl Into<String>) -> ProcessOutput {
    ProcessOutput {
        exit_code: Some(0),
        stdout: stdout.into(),
        ..ProcessOutput::default()
    }
}

pub fn exited(code: i32, stdout: &str, stderr: &str) -> ProcessOutput {
    ProcessOutput {
        exit_code: Some(code),
        stdout: stdout.to_owned(),
        stderr: stderr.to_owned(),
        ..ProcessOutput::default()
    }
}

pub fn context(
    inspect: FakeHostInspect,
    toml: &str,
    clock: Arc<FakeClock>,
    exec: Arc<FakeProcessExecutor>,
) -> ServeContext {
    ServeContext::new(
        parse_helper(toml).map(|(config, _)| config),
        Arc::new(inspect),
        clock,
        HostDirs::default(),
        "test-build",
    )
    .with_process_executor(exec)
}

pub fn clock() -> Arc<FakeClock> {
    Arc::new(FakeClock::new(
        "2026-05-01T12:00:00Z".parse::<Timestamp>().expect("time"),
    ))
}

pub fn request_bytes(request: HelperRequest) -> Vec<u8> {
    let frame = HelperRequestFrame {
        v: HELPER_PROTOCOL,
        id: Uuid::now_v7(),
        request,
    };
    encode_frame(&frame).expect("encode")
}

/// Starts the exchange; the caller drives `serve` and the answer reader together.
pub async fn ask(ctx: &ServeContext, request: HelperRequest) -> HelperResponse {
    ask_raw(ctx, &request_bytes(request)).await
}

pub async fn ask_raw(ctx: &ServeContext, bytes: &[u8]) -> HelperResponse {
    let (mut client, server) = UnixStream::pair().expect("pair");
    client.write_all(bytes).await.expect("send");
    let mut answer = Vec::new();
    let serving = serve_connection(
        TestConn {
            stream: server,
            uid: AGENT_ID,
        },
        ctx,
    );
    // Read while serving: a large answer does not fit the socket buffer.
    let (_, read) = tokio::join!(serving, client.read_to_end(&mut answer));
    read.expect("read answer");
    decode_response(&answer[4..]).expect("decodes").response
}
