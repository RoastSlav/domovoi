//! Process start-up: the TLS crypto provider, logging, metrics, signals and the clock.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use clap::ValueEnum;
use domovoi_core::ports::Clock;
use domovoi_hub::api::MetricsRender;
use jiff::Timestamp;
use metrics_exporter_prometheus::PrometheusBuilder;
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

/// Installs ring as the process-wide rustls provider. Fails when a provider is already
/// installed, so a second, different provider can never sneak in.
pub fn install_crypto() -> anyhow::Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| {
            anyhow::anyhow!(
                "a rustls crypto provider was already installed; domovoi installs ring exactly once"
            )
        })
}

/// How log lines look.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum LogFormat {
    /// Readable lines.
    Human,
    /// One JSON object per line.
    Json,
}

/// Logging options from the command line.
pub struct Logging {
    /// Line format.
    pub format: LogFormat,
    /// `--log-level`; falls back to `RUST_LOG`, then `info`.
    pub filter: Option<String>,
    /// Commands whose output is the point (`version`, `check`) log warnings only.
    pub quiet: bool,
}

/// Sends logs to standard error. Configuration secrets are wrapped types whose `Debug`
/// and `Display` never show the value, so they cannot reach a log line.
pub fn logging(options: &Logging) {
    let default = if options.quiet { "warn" } else { "info" };
    let filter = options
        .filter
        .as_deref()
        .map(EnvFilter::new)
        .or_else(|| EnvFilter::try_from_default_env().ok())
        .unwrap_or_else(|| EnvFilter::new(default));
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()));
    let _ = match options.format {
        LogFormat::Human => builder.try_init(),
        LogFormat::Json => builder.json().try_init(),
    };
}

/// Installs the Prometheus recorder and returns what renders it for `/metrics`.
pub fn metrics() -> anyhow::Result<MetricsRender> {
    let recorder = PrometheusBuilder::new().build_recorder();
    let handle = recorder.handle();
    metrics::set_global_recorder(recorder)
        .map_err(|err| anyhow::anyhow!("could not install the metrics recorder: {err}"))?;
    Ok(Arc::new(move || handle.render()))
}

/// A token cancelled on SIGINT or SIGTERM.
pub fn shutdown_on_signals() -> CancellationToken {
    let token = CancellationToken::new();
    let cancel = token.clone();
    tokio::spawn(async move {
        wait_for_signal().await;
        tracing::info!("shutting down");
        cancel.cancel();
    });
    token
}

#[cfg(unix)]
async fn wait_for_signal() {
    use tokio::signal::unix::{SignalKind, signal};
    match signal(SignalKind::terminate()) {
        Ok(mut term) => {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = term.recv() => {}
            }
        }
        Err(err) => {
            tracing::warn!(%err, "cannot watch SIGTERM; only Ctrl-C stops domovoi");
            let _ = tokio::signal::ctrl_c().await;
        }
    }
}

#[cfg(not(unix))]
async fn wait_for_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

/// The wall clock.
pub struct SystemClock;

#[async_trait]
impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }

    async fn sleep_until(&self, t: Timestamp) {
        if let Ok(wait) = Duration::try_from(Timestamp::now().duration_until(t)) {
            tokio::time::sleep(wait).await;
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn crypto_provider_is_installed_once() {
        // Whichever test runs first may have installed it; after one more call it is
        // certainly installed, and every further call must refuse.
        let _ = super::install_crypto();
        let second = super::install_crypto();
        assert!(second.is_err());
        assert!(second.unwrap_err().to_string().contains("exactly once"));
        assert!(rustls::crypto::CryptoProvider::get_default().is_some());
    }
}
