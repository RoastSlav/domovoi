//! Captures the tracing output of a test, so a test can assert on what was logged.

use std::io::Write;
use std::sync::{Arc, Mutex};

use tracing_subscriber::fmt::MakeWriter;

/// A buffer every log line goes to while a [`LogCapture`] guard is alive.
#[derive(Clone, Default)]
pub struct LogCapture(Arc<Mutex<Vec<u8>>>);

impl Write for LogCapture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if let Ok(mut inner) = self.0.lock() {
            inner.extend_from_slice(buf);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for LogCapture {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

impl LogCapture {
    /// Starts capturing every event (all levels, plain text, no colour) on this thread
    /// until the returned guard drops. Use it in a current-thread test runtime.
    #[must_use]
    pub fn start() -> (Self, tracing::subscriber::DefaultGuard) {
        let capture = Self::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(capture.clone())
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .finish();
        let guard = tracing::subscriber::set_default(subscriber);
        (capture, guard)
    }

    /// Everything captured so far.
    #[must_use]
    pub fn text(&self) -> String {
        self.0
            .lock()
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default()
    }

    /// The captured lines that contain `needle`.
    #[must_use]
    pub fn lines_with(&self, needle: &str) -> Vec<String> {
        self.text()
            .lines()
            .filter(|l| l.contains(needle))
            .map(str::to_owned)
            .collect()
    }
}
