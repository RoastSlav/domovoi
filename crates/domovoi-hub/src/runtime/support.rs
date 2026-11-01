//! Small pieces the runtime assembles: one notifier over many, the Prometheus recorder,
//! and the hub's public address.

use std::net::IpAddr;
use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use domovoi_config::hub::HubConfig;
use domovoi_core::ports::{ChatError, Notifier, OutboundMessage};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

use super::RuntimeError;
use crate::api::MetricsRender;

/// Every configured notifier behind one, for the API's "send the digest now". It succeeds
/// when at least one notifier accepted the message.
pub(super) struct FanOut {
    notifiers: Vec<Arc<dyn Notifier>>,
}

impl FanOut {
    /// One notifier over `notifiers`, or `None` when there are none.
    pub(super) fn over(notifiers: &[Arc<dyn Notifier>]) -> Option<Arc<dyn Notifier>> {
        match notifiers {
            [] => None,
            [one] => Some(one.clone()),
            many => Some(Arc::new(Self {
                notifiers: many.to_vec(),
            })),
        }
    }
}

#[async_trait]
impl Notifier for FanOut {
    fn id(&self) -> &str {
        "all"
    }

    async fn send(&self, msg: &OutboundMessage) -> Result<(), ChatError> {
        let results = futures::future::join_all(self.notifiers.iter().map(|n| n.send(msg))).await;
        let mut last = None;
        for result in results {
            match result {
                Ok(()) => return Ok(()),
                Err(err) => last = Some(err),
            }
        }
        Err(last.unwrap_or_else(|| ChatError::Rejected("no notifier".to_owned())))
    }
}

/// The process-wide Prometheus recorder behind `/metrics`. Installed once; `None` when
/// another recorder was installed first.
pub(super) fn metrics() -> Option<MetricsRender> {
    static HANDLE: OnceLock<Option<PrometheusHandle>> = OnceLock::new();
    let handle = HANDLE.get_or_init(|| {
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        match metrics::set_global_recorder(recorder) {
            Ok(()) => Some(handle),
            Err(err) => {
                tracing::warn!(%err, "metrics recorder already installed; /metrics stays empty");
                None
            }
        }
    });
    handle.clone().map(|handle| {
        let render: MetricsRender = Arc::new(move || handle.render());
        render
    })
}

/// Where people reach the hub: `[web] public_url`, or the listener address. A wildcard
/// listener (`0.0.0.0`, `::`) becomes loopback, since a wildcard is no address to open.
pub(super) fn public_url(config: &HubConfig) -> Result<url::Url, RuntimeError> {
    if let Some(url) = &config.web.public_url {
        return Ok(url.clone());
    }
    let mut bind = config.web.bind;
    if bind.ip().is_unspecified() {
        let loopback: IpAddr = match bind.ip() {
            IpAddr::V4(_) => IpAddr::from([127, 0, 0, 1]),
            IpAddr::V6(_) => IpAddr::from([0, 0, 0, 0, 0, 0, 0, 1]),
        };
        bind.set_ip(loopback);
        tracing::warn!(%bind, "[web] public_url is not set; links point at {bind}");
    }
    url::Url::parse(&format!("http://{bind}")).map_err(|err| RuntimeError::Url(err.to_string()))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use domovoi_core::ports::{ChatError, OutboundMessage};
    use domovoi_testkit::RecordingNotifier;

    use super::FanOut;

    fn message() -> OutboundMessage {
        OutboundMessage {
            title: "Daily digest".to_owned(),
            markdown: "all quiet".to_owned(),
            severity: None,
        }
    }

    #[tokio::test]
    async fn fan_out_reaches_every_notifier_and_needs_one_to_succeed() {
        assert!(FanOut::over(&[]).is_none());
        let up = Arc::new(RecordingNotifier::new("webhook"));
        let down = Arc::new(RecordingNotifier::new("other"));
        down.fail_next(ChatError::Transport("offline".to_owned()));
        let both = FanOut::over(&[down.clone(), up.clone()]).unwrap();
        both.send(&message()).await.unwrap();
        assert_eq!(up.sent().len(), 1, "the working one got it");

        down.fail_next(ChatError::Transport("offline".to_owned()));
        up.fail_next(ChatError::Rejected("no".to_owned()));
        assert!(
            both.send(&message()).await.is_err(),
            "fails only when every one fails"
        );
    }
}
