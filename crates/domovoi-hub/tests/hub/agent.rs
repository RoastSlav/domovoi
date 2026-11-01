//! A scripted agent: TLS that trusts any certificate (and remembers it), a WebSocket,
//! and frames the test sends one by one.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use domovoi_core::{AgentId, HostId, HostName, RequestId};
use domovoi_hub::agent_server::CHANNEL_BINDING_LABEL;
use domovoi_proto::messages::{
    Challenge, ChallengeResponse, Enroll, Enrolled, Hello, HostFacts, RejectReason, Welcome,
};
use domovoi_proto::{
    Decoded, EnrollToken, Message, PROTOCOL_VERSION, ProtocolVersion, PublicKey, Signature,
    challenge_message, decode, encode,
};
use ed25519_dalek::{Signer, SigningKey};
use futures::{SinkExt, StreamExt};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::WebPkiSupportedAlgorithms;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, Error, SignatureScheme};
use sha2::{Digest, Sha256};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message as WsMessage;

use crate::harness::guard;

/// Accepts any certificate and records the SHA-256 of the one it saw.
#[derive(Debug)]
struct AnyCert {
    seen: Mutex<Option<[u8; 32]>>,
    algorithms: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for AnyCert {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        *self.seen.lock().unwrap() = Some(Sha256::digest(end_entity.as_ref()).into());
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

/// What an enrolled fake agent keeps.
#[derive(Clone)]
pub struct Ident {
    pub key: SigningKey,
    pub agent: AgentId,
    pub host: HostId,
}

pub fn new_key() -> SigningKey {
    SigningKey::from_bytes(&rand::random())
}

pub fn facts() -> HostFacts {
    HostFacts {
        hostname: "nas".to_owned(),
        os: "Debian GNU/Linux 13".to_owned(),
        kernel: "6.12.0".to_owned(),
        arch: "x86_64".to_owned(),
        cpu_count: 4,
        mem_total_bytes: 8 << 30,
        boot_time: crate::harness::t0(),
        boot_id: "boot-1".to_owned(),
    }
}

pub fn hello(agent: AgentId, protocol: ProtocolVersion) -> Message {
    Message::Hello(Hello {
        agent_id: agent,
        protocol,
        agent_version: "0.1.0".to_owned(),
        facts: facts(),
        capabilities: Vec::new(),
        skills: Vec::new(),
    })
}

/// One connection to the hub.
pub struct FakeAgent {
    ws: WebSocketStream<TlsStream<TcpStream>>,
    /// The TLS channel binding on the agent's side.
    pub binding: String,
    /// SHA-256 of the certificate the hub presented.
    pub cert: [u8; 32],
}

impl FakeAgent {
    pub async fn connect(addr: SocketAddr) -> Self {
        let verifier = Arc::new(AnyCert {
            seen: Mutex::new(None),
            algorithms: rustls::crypto::ring::default_provider().signature_verification_algorithms,
        });
        let config =
            ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_protocol_versions(&[&rustls::version::TLS13])
                .unwrap()
                .dangerous()
                .with_custom_certificate_verifier(verifier.clone())
                .with_no_client_auth();
        let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
        let tcp = guard(TcpStream::connect(addr)).await.unwrap();
        let name = ServerName::try_from("testhub").unwrap();
        let tls = guard(connector.connect(name, tcp)).await.unwrap();
        let mut binding = [0u8; 32];
        tls.get_ref()
            .1
            .export_keying_material(&mut binding, CHANNEL_BINDING_LABEL, None)
            .unwrap();
        let url = format!("wss://testhub:{}/", addr.port());
        let (ws, _) = guard(tokio_tungstenite::client_async(url, tls))
            .await
            .unwrap();
        let cert = verifier.seen.lock().unwrap().unwrap();
        Self {
            ws,
            binding: hex::encode(binding),
            cert,
        }
    }

    pub async fn send(&mut self, message: Message, reply_to: Option<RequestId>) -> RequestId {
        let id = RequestId::new();
        let frame = encode(&message, id, reply_to).unwrap();
        self.send_raw(frame).await;
        id
    }

    pub async fn send_raw(&mut self, frame: String) {
        guard(self.ws.send(WsMessage::text(frame))).await.unwrap();
    }

    /// Sends a frame; false once the hub has closed the connection.
    pub async fn try_send_raw(&mut self, frame: String) -> bool {
        guard(self.ws.send(WsMessage::text(frame))).await.is_ok()
    }

    /// The next message, or `None` once the hub closed the connection.
    pub async fn recv(&mut self) -> Option<Decoded> {
        loop {
            match guard(self.ws.next()).await {
                Some(Ok(WsMessage::Text(text))) => return Some(decode(text.as_str()).unwrap()),
                Some(Ok(WsMessage::Ping(_) | WsMessage::Pong(_) | WsMessage::Frame(_))) => {}
                _ => return None,
            }
        }
    }

    pub async fn expect(&mut self) -> Decoded {
        self.recv().await.expect("the hub closed the connection")
    }

    /// Reads until the hub closes the connection.
    pub async fn closed(&mut self) {
        while self.recv().await.is_some() {}
    }

    pub async fn enroll(&mut self, token: &str, key: &SigningKey) -> Message {
        let enroll = Message::Enroll(Enroll {
            token: EnrollToken::new(token),
            public_key: PublicKey(key.verifying_key().to_bytes()),
            host_name: HostName::parse("nas").unwrap(),
            facts: facts(),
            agent_version: "0.1.0".to_owned(),
        });
        self.send(enroll, None).await;
        self.expect().await.message
    }

    /// Sends `hello` and returns the challenge, or the message the hub sent instead.
    pub async fn hello(&mut self, agent: AgentId, protocol: ProtocolVersion) -> Message {
        self.send(hello(agent, protocol), None).await;
        self.expect().await.message
    }

    /// Answers a challenge, signing with `key` over this connection's binding.
    pub async fn answer(
        &mut self,
        challenge: &Challenge,
        agent: AgentId,
        key: &SigningKey,
    ) -> Message {
        let message = challenge_message(&challenge.nonce, agent, &self.binding);
        let signature = Signature(key.sign(&message).to_bytes());
        self.send(
            Message::ChallengeResponse(ChallengeResponse { signature }),
            None,
        )
        .await;
        self.expect().await.message
    }

    /// The whole handshake for an enrolled agent.
    pub async fn authenticate(&mut self, ident: &Ident) -> Welcome {
        let Message::Challenge(challenge) = self.hello(ident.agent, PROTOCOL_VERSION).await else {
            panic!("expected a challenge");
        };
        match self.answer(&challenge, ident.agent, &ident.key).await {
            Message::Welcome(welcome) => welcome,
            other => panic!("expected welcome, got {other:?}"),
        }
    }
}

pub fn rejected(message: &Message) -> Option<RejectReason> {
    match message {
        Message::Reject(reject) => Some(reject.reason),
        _ => None,
    }
}

/// Enrolls a new agent for `host` through a fresh connection.
pub async fn enroll(hub: &crate::harness::Hub, host: &str) -> Ident {
    let (_, token) = hub.token(host).await;
    let key = new_key();
    let mut agent = FakeAgent::connect(hub.addr).await;
    let Message::Enrolled(Enrolled {
        agent_id, host_id, ..
    }) = agent.enroll(&token, &key).await
    else {
        panic!("enrollment failed");
    };
    Ident {
        key,
        agent: agent_id,
        host: host_id,
    }
}

/// Enrolls and connects a new agent for `host`.
pub async fn online(hub: &crate::harness::Hub, host: &str) -> (Ident, FakeAgent) {
    let ident = enroll(hub, host).await;
    let mut agent = FakeAgent::connect(hub.addr).await;
    agent.authenticate(&ident).await;
    (ident, agent)
}
