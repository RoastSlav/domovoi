//! A minimal hub for tests: TLS with a self-signed certificate, WebSocket, and connections
//! the test drives frame by frame.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use domovoi_agent::identity::Identity;
use domovoi_agent::link::frame;
use domovoi_agent::link::tls::{CHANNEL_BINDING_LABEL, fingerprint};
use domovoi_agent::link::ws::{WsTransport, ws_config};
use domovoi_core::RequestId;
use domovoi_proto::messages::{
    AgentSettings, Challenge, Hello, Reject, RejectReason, SessionId, Signals, Welcome,
};
use domovoi_proto::{Decoded, Message, Nonce, Transport, challenge_message};
use ed25519_dalek::{Signature, VerifyingKey};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_rustls::TlsAcceptor;
use url::Url;

/// A real-time guard so a broken test fails instead of hanging. Never a sleep.
pub const GUARD: std::time::Duration = std::time::Duration::from_secs(10);

pub async fn guard<F: std::future::Future>(fut: F) -> F::Output {
    tokio::time::timeout(GUARD, fut)
        .await
        .expect("the test hub waited too long")
}

/// One accepted WebSocket connection.
pub struct HubConn {
    pub transport: Box<dyn Transport>,
    /// The TLS channel binding on the hub's side.
    pub session_hint: String,
}

impl HubConn {
    pub async fn recv(&mut self) -> Option<Decoded> {
        guard(frame::recv(self.transport.as_mut()))
            .await
            .ok()
            .flatten()
    }

    pub async fn expect(&mut self) -> Decoded {
        self.recv().await.expect("the agent closed the connection")
    }

    pub async fn send(&mut self, message: Message, reply_to: Option<RequestId>) {
        frame::send(self.transport.as_mut(), &message, reply_to)
            .await
            .unwrap();
    }

    /// Receives `hello`, checks it names `identity`, and sends a challenge. Returns the nonce.
    pub async fn challenge(&mut self, identity: &Identity) -> (Hello, Nonce) {
        let hello = match self.expect().await.message {
            Message::Hello(hello) => hello,
            other => panic!("expected hello, got {other:?}"),
        };
        assert_eq!(hello.agent_id, identity.agent_id);
        let nonce = Nonce(rand::random());
        self.send(Message::Challenge(Challenge { nonce }), None)
            .await;
        (hello, nonce)
    }

    /// Verifies the challenge response against `identity`'s public key and this TLS session.
    pub async fn verify(&mut self, identity: &Identity, nonce: &Nonce) -> bool {
        let response = match self.expect().await.message {
            Message::ChallengeResponse(response) => response,
            other => panic!("expected challenge_response, got {other:?}"),
        };
        let key = VerifyingKey::from_bytes(identity.public_key().as_bytes()).unwrap();
        let message = challenge_message(nonce, identity.agent_id, &self.session_hint);
        key.verify_strict(
            &message,
            &Signature::from_bytes(response.signature.as_bytes()),
        )
        .is_ok()
    }

    /// The whole handshake, ending in `welcome` with `heartbeat_seconds`.
    pub async fn welcome(&mut self, identity: &Identity, heartbeat_seconds: u32) -> Hello {
        let (hello, nonce) = self.challenge(identity).await;
        assert!(
            self.verify(identity, &nonce).await,
            "bad challenge signature"
        );
        let welcome = Welcome {
            session_id: SessionId::new(),
            hub_version: "test".to_owned(),
            config: AgentSettings {
                heartbeat_seconds,
                check_overrides: Default::default(),
            },
        };
        self.send(Message::Welcome(welcome), None).await;
        hello
    }

    pub async fn reject(&mut self, identity: &Identity, reason: RejectReason) {
        let (_, nonce) = self.challenge(identity).await;
        assert!(self.verify(identity, &nonce).await);
        self.send(Message::Reject(Reject { reason }), None).await;
    }

    /// The next `signals` batch, skipping heartbeats.
    pub async fn signals(&mut self) -> Signals {
        loop {
            match self.expect().await.message {
                Message::Signals(signals) => return signals,
                Message::Ping(_) | Message::DigestFacts(_) => {}
                other => panic!("expected signals, got {other:?}"),
            }
        }
    }
}

/// The test hub's listener.
pub struct TestHub {
    pub url: Url,
    pub fingerprint: [u8; 32],
    accepted: Arc<AtomicUsize>,
    conns: mpsc::UnboundedReceiver<HubConn>,
}

fn certificate() -> (CertificateDer<'static>, PrivateKeyDer<'static>) {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()]).unwrap();
    (
        cert.cert.der().clone(),
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der())),
    )
}

impl TestHub {
    pub async fn start() -> Self {
        let (cert, key) = certificate();
        let fingerprint = fingerprint(cert.as_ref());
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .unwrap();
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let accepted = Arc::new(AtomicUsize::new(0));
        let (tx, conns) = mpsc::unbounded_channel();
        let counter = accepted.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let acceptor = acceptor.clone();
                let tx = tx.clone();
                let counter = counter.clone();
                tokio::spawn(async move {
                    let Ok(tls) = acceptor.accept(stream).await else {
                        return;
                    };
                    let mut binding = [0u8; 32];
                    tls.get_ref()
                        .1
                        .export_keying_material(&mut binding, CHANNEL_BINDING_LABEL, None)
                        .unwrap();
                    let Ok(ws) =
                        tokio_tungstenite::accept_async_with_config(tls, Some(ws_config())).await
                    else {
                        return;
                    };
                    counter.fetch_add(1, Ordering::SeqCst);
                    let _ = tx.send(HubConn {
                        transport: Box::new(WsTransport::new(ws)),
                        session_hint: hex::encode(binding),
                    });
                });
            }
        });
        Self {
            url: format!("wss://127.0.0.1:{port}/agent").parse().unwrap(),
            fingerprint,
            accepted,
            conns,
        }
    }

    /// The next WebSocket connection.
    pub async fn accept(&mut self) -> HubConn {
        guard(self.conns.recv()).await.expect("the hub stopped")
    }

    /// How many WebSocket connections were accepted.
    pub fn accepted(&self) -> usize {
        self.accepted.load(Ordering::SeqCst)
    }
}
