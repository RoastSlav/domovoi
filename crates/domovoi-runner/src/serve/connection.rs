//! One connection: read the request, decide, answer once.

use domovoi_core::identity::AGENT_ACCOUNT;
use domovoi_core::skill_runner::{
    FrameError, HealthReply, MAX_FRAME_BYTES, MAX_REQUEST_BYTES, ProbeRequest, RUNNER_PROTOCOL,
    Refusal, RefusalCode, RunnerReply, RunnerRequest, decode_payload, encode_frame, frame_len,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::UnixStream;

use super::peer::{RequestKind, peer_allowed};
use super::{ServeContext, ServeError, run, start_checks};

/// What serving a request decided.
pub(super) enum Answer {
    /// Write this reply.
    Reply(RunnerReply),
    /// Write nothing: the peer cancelled.
    Silent,
}

pub(super) fn refused(code: RefusalCode, detail: &str) -> Answer {
    Answer::Reply(RunnerReply::Refused(Refusal {
        code,
        detail: detail.to_owned(),
    }))
}

/// Serves one request on `stream`; see [`super::serve_connection_with_peer`].
pub(super) async fn serve(
    stream: UnixStream,
    peer_uid: Option<u32>,
    ctx: ServeContext,
) -> Result<(), ServeError> {
    let (mut reader, mut writer) = stream.into_split();
    let answer = answer(&mut reader, peer_uid, &ctx).await?;
    if let Answer::Reply(reply) = answer {
        write_reply(&mut writer, &reply).await?;
    }
    Ok(())
}

async fn answer<R>(
    reader: &mut R,
    peer_uid: Option<u32>,
    ctx: &ServeContext,
) -> Result<Answer, ServeError>
where
    R: AsyncRead + Unpin + Send,
{
    let request = match read_request(reader, ctx.request_timeout).await {
        Ok(request) => request,
        Err(detail) => return Ok(refused(RefusalCode::BadRequest, detail)),
    };
    if let Err(refusal) = start_checks::run(&*ctx.inspect) {
        return Ok(Answer::Reply(RunnerReply::Refused(refusal)));
    }
    if request.protocol() != RUNNER_PROTOCOL {
        return Ok(refused(
            RefusalCode::ProtocolMismatch,
            "the request speaks another protocol version",
        ));
    }
    // Resolved by name at this moment, never cached.
    let agent_uid = match ctx.inspect.user(AGENT_ACCOUNT) {
        Ok(Some(account)) => Some(account.uid),
        Ok(None) | Err(_) => None,
    };
    if !peer_allowed(peer_uid, RequestKind::of(&request), agent_uid) {
        tracing::warn!(peer = ?peer_uid, "refusing a request from a peer that may not make it");
        return Ok(refused(
            RefusalCode::PeerNotAllowed,
            "this peer may not make this request",
        ));
    }
    match request {
        RunnerRequest::Health(_) => Ok(Answer::Reply(RunnerReply::Health(HealthReply {
            protocol: RUNNER_PROTOCOL,
            version: env!("CARGO_PKG_VERSION").to_owned(),
            skills_root: ctx.skills_root.to_string_lossy().into_owned(),
            packages_root: ctx.packages_root.to_string_lossy().into_owned(),
        }))),
        RunnerRequest::Probe(ProbeRequest { helper_pid, .. }) => {
            let collector = ctx.probe.clone();
            let report = tokio::task::spawn_blocking(move || collector.collect(helper_pid))
                .await
                .map_err(|err| ServeError::Probe(err.to_string()))?;
            Ok(Answer::Reply(RunnerReply::Probe(Box::new(report))))
        }
        RunnerRequest::Run(run_request) => Ok(run::handle(run_request, reader, ctx).await),
    }
}

/// Reads one frame within `timeout`; the error is the sentence for a `bad_request` refusal.
async fn read_request<R>(
    reader: &mut R,
    timeout: std::time::Duration,
) -> Result<RunnerRequest, &'static str>
where
    R: AsyncRead + Unpin,
{
    match tokio::time::timeout(timeout, read_frame(reader)).await {
        Ok(Ok(request)) => Ok(request),
        Ok(Err(detail)) => Err(detail),
        Err(_) => Err("the request was not received in time"),
    }
}

async fn read_frame<R>(reader: &mut R) -> Result<RunnerRequest, &'static str>
where
    R: AsyncRead + Unpin,
{
    let mut header = [0_u8; 4];
    reader
        .read_exact(&mut header)
        .await
        .map_err(|_| "the request frame header is missing")?;
    let len = frame_len(header, MAX_REQUEST_BYTES).map_err(|err| match err {
        FrameError::Empty => "the request frame is empty",
        FrameError::TooLarge { .. } => "the request frame is too large",
        FrameError::Malformed(_) => "the request frame is malformed",
    })?;
    let mut body = vec![0_u8; len];
    reader
        .read_exact(&mut body)
        .await
        .map_err(|_| "the request frame is shorter than it announced")?;
    decode_payload(&body).map_err(|_| "the request is not a valid runner request")
}

/// Writes `reply`; a peer that has gone is not an error.
async fn write_reply<W>(writer: &mut W, reply: &RunnerReply) -> Result<(), ServeError>
where
    W: AsyncWrite + Unpin,
{
    let frame = match encode_frame(reply, MAX_FRAME_BYTES) {
        Ok(frame) => frame,
        Err(err) => {
            let fallback = RunnerReply::Refused(Refusal {
                code: RefusalCode::SpawnFailed,
                detail: "the reply could not be encoded".to_owned(),
            });
            tracing::error!("cannot encode a reply: {err}");
            encode_frame(&fallback, MAX_FRAME_BYTES).map_err(ServeError::Encode)?
        }
    };
    if writer.write_all(&frame).await.is_ok() {
        let _ = writer.shutdown().await;
    }
    Ok(())
}
