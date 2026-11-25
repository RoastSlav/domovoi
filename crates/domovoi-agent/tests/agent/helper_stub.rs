//! A stub helper on a unix socket: it answers each read request from a script, records the
//! requests it received and can hold answers back until a test releases them.

#![cfg(unix)]

use std::path::Path;
use std::sync::{Arc, Mutex};

use domovoi_core::helper::{
    HELPER_PROTOCOL, HelperRequest, HelperResponse, HelperResponseFrame, MAX_REQUEST_BYTES,
    decode_request, encode_frame, frame_len,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;
use tokio::sync::Semaphore;

/// What the stub answers for a request.
pub type Answer = Arc<dyn Fn(&HelperRequest) -> HelperResponse + Send + Sync>;

/// A running stub helper.
pub struct Stub {
    requests: Arc<Mutex<Vec<HelperRequest>>>,
    gate: Option<Arc<Semaphore>>,
    task: tokio::task::JoinHandle<()>,
}

impl Stub {
    /// Every request received so far, oldest first.
    pub fn requests(&self) -> Vec<HelperRequest> {
        self.requests.lock().unwrap().clone()
    }

    /// How many requests arrived.
    pub fn count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }

    /// Lets `n` held answers go.
    pub fn release(&self, n: usize) {
        if let Some(gate) = &self.gate {
            gate.add_permits(n);
        }
    }
}

impl Drop for Stub {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn serve_inner(socket: &Path, answer: Answer, gate: Option<Arc<Semaphore>>) -> Stub {
    let listener = UnixListener::bind(socket).unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let seen = requests.clone();
    let held = gate.clone();
    let task = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let (seen, answer, held) = (seen.clone(), answer.clone(), held.clone());
            tokio::spawn(async move {
                let mut header = [0_u8; 4];
                if stream.read_exact(&mut header).await.is_err() {
                    return;
                }
                let Ok(len) = frame_len(header, MAX_REQUEST_BYTES) else {
                    return;
                };
                let mut body = vec![0_u8; len];
                if stream.read_exact(&mut body).await.is_err() {
                    return;
                }
                let frame = decode_request(&body).unwrap();
                seen.lock().unwrap().push(frame.request.clone());
                if let Some(held) = held {
                    held.acquire().await.unwrap().forget();
                }
                let reply = HelperResponseFrame {
                    v: HELPER_PROTOCOL,
                    id: frame.id,
                    response: answer(&frame.request),
                };
                let _ = stream.write_all(&encode_frame(&reply).unwrap()).await;
            });
        }
    });
    Stub {
        requests,
        gate,
        task,
    }
}

/// Serves `answer` on `socket`.
pub fn serve(socket: &Path, answer: Answer) -> Stub {
    serve_inner(socket, answer, None)
}

/// Serves `answer` on `socket`, holding every answer until [`Stub::release`] is called.
pub fn serve_held(socket: &Path, answer: Answer) -> Stub {
    serve_inner(socket, answer, Some(Arc::new(Semaphore::new(0))))
}
