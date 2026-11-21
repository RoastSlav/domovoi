//! A hard per-call deadline on top of a provider's own idle timeout.

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::ports::{AnswerSink, Clock, LlmError, LlmProvider, LlmRequest, LlmResponse};
use jiff::SignedDuration;

/// The hard per-call deadline used when none is configured.
pub const DEFAULT_CALL_DEADLINE: SignedDuration = SignedDuration::from_secs(300);

/// Wraps `inner` so one `complete` call fails with [`LlmError::Timeout`] when it has not
/// finished `after` the call started, measured on `clock`.
///
/// Dropping the call future cancels the request (the HTTP client abandons it and a CLI
/// child is killed), so a provider that keeps streaming cannot run forever.
#[must_use]
pub fn with_deadline(
    inner: Arc<dyn LlmProvider>,
    after: SignedDuration,
    clock: Arc<dyn Clock>,
) -> Arc<dyn LlmProvider> {
    Arc::new(Deadline {
        inner,
        after,
        clock,
    })
}

struct Deadline {
    inner: Arc<dyn LlmProvider>,
    after: SignedDuration,
    clock: Arc<dyn Clock>,
}

#[async_trait]
impl LlmProvider for Deadline {
    fn id(&self) -> &str {
        self.inner.id()
    }

    fn model(&self) -> Option<&str> {
        self.inner.model()
    }

    fn supports_tools(&self) -> bool {
        self.inner.supports_tools()
    }

    fn prices_usage(&self) -> bool {
        self.inner.prices_usage()
    }

    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        self.race(self.inner.complete(req)).await
    }

    async fn complete_streaming(
        &self,
        req: LlmRequest,
        sink: &dyn AnswerSink,
    ) -> Result<LlmResponse, LlmError> {
        self.race(self.inner.complete_streaming(req, sink)).await
    }
}

impl Deadline {
    /// Races `call` against the hard deadline.
    async fn race(
        &self,
        call: impl std::future::Future<Output = Result<LlmResponse, LlmError>>,
    ) -> Result<LlmResponse, LlmError> {
        let now = self.clock.now();
        let deadline_at = now.checked_add(self.after).unwrap_or(now);
        let timer = self.clock.sleep_until(deadline_at);
        futures::pin_mut!(call, timer);
        match futures::future::select(call, timer).await {
            futures::future::Either::Left((result, _)) => result,
            futures::future::Either::Right(_) => Err(LlmError::Timeout { after: self.after }),
        }
    }
}
