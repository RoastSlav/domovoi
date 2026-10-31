//! The network checks the hub runs on itself and on silent hosts.

use std::io::ErrorKind;
use std::sync::Arc;

use async_trait::async_trait;
use domovoi_checks::net::gateway_reachable;
use domovoi_checks::net::tcp::{HostPort, TCP_TIMEOUT, connect};
use domovoi_core::ports::{Clock, FsError, HostFs};

/// Network reachability as seen from the hub. Injected so tests decide the answers.
#[async_trait]
pub trait NetProbes: Send + Sync {
    /// Whether the hub's default gateway answers.
    async fn gateway_reachable(&self) -> bool;

    /// Whether something answers on `target` (`host:port`). A refused connection counts:
    /// the host is up even if nothing listens there.
    async fn tcp_reachable(&self, target: &str) -> bool;
}

/// The real probes: the routing table under `/proc` and TCP connects.
pub struct RealNetProbes {
    clock: Arc<dyn Clock>,
    fs: ProcFs,
}

impl RealNetProbes {
    /// Probes that time out on `clock`.
    #[must_use]
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self { clock, fs: ProcFs }
    }
}

#[async_trait]
impl NetProbes for RealNetProbes {
    async fn gateway_reachable(&self) -> bool {
        gateway_reachable(&self.fs, &*self.clock).await
    }

    async fn tcp_reachable(&self, target: &str) -> bool {
        let Ok(target) = HostPort::parse(target) else {
            return false;
        };
        match connect(target.host(), target.port(), &*self.clock, TCP_TIMEOUT).await {
            Ok(()) => true,
            Err(err) => err.kind() == ErrorKind::ConnectionRefused,
        }
    }
}

/// Read-only access to `/proc`, which is all the gateway check reads.
struct ProcFs;

fn allowed(path: &str) -> Result<&str, FsError> {
    if path.starts_with("/proc/") && !path.split('/').any(|part| part == "..") {
        Ok(path)
    } else {
        Err(FsError::InvalidPath)
    }
}

#[async_trait]
impl HostFs for ProcFs {
    async fn read_to_string(&self, path: &str) -> Result<String, FsError> {
        tokio::fs::read_to_string(allowed(path)?)
            .await
            .map_err(|e| match e.kind() {
                ErrorKind::NotFound => FsError::NotFound,
                ErrorKind::PermissionDenied => FsError::PermissionDenied,
                _ => FsError::Io(e.to_string()),
            })
    }

    async fn read_dir(&self, _path: &str) -> Result<Vec<String>, FsError> {
        Err(FsError::PermissionDenied)
    }

    async fn exists(&self, path: &str) -> bool {
        match allowed(path) {
            Ok(path) => tokio::fs::try_exists(path).await.unwrap_or(false),
            Err(_) => false,
        }
    }
}
