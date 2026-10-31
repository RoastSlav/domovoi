//! The HTTP probe against a local HTTPS server with a test certificate.

use std::collections::BTreeSet;
use std::sync::Arc;

use domovoi_checks::net::HttpProbe;
use domovoi_checks::net::cert::not_after;
use domovoi_core::Severity;
use jiff::{Timestamp, ToSpan};
use rcgen::{CertificateParams, KeyPair, date_time_ymd};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use url::Url;

use crate::harness::{Harness, number, only, text};

fn ymd(at: Timestamp) -> (i32, u8, u8) {
    let date = at.to_zoned(jiff::tz::TimeZone::UTC).date();
    (
        i32::from(date.year()),
        u8::try_from(date.month()).unwrap(),
        u8::try_from(date.day()).unwrap(),
    )
}

/// A self-signed certificate for `localhost` valid until `days` days from now.
fn certificate(days: i64) -> (CertificateDer<'static>, PrivateKeyDer<'static>) {
    let mut params = CertificateParams::new(vec!["localhost".to_owned()]).unwrap();
    let (y, m, d) = ymd(Timestamp::now() - (2 * 24).hours());
    params.not_before = date_time_ymd(y, m, d);
    let (y, m, d) = ymd(Timestamp::now() + (days * 24).hours());
    params.not_after = date_time_ymd(y, m, d);
    let key = KeyPair::generate().unwrap();
    let cert = params.self_signed(&key).unwrap();
    (
        cert.der().clone(),
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
    )
}

/// Serves HTTPS on loopback, answering every request with `status`.
async fn serve(cert: CertificateDer<'static>, key: PrivateKeyDer<'static>, status: u16) -> u16 {
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(vec![cert], key)
    .unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let Ok(mut tls) = acceptor.accept(stream).await else {
                continue;
            };
            let mut request = Vec::new();
            let mut buf = [0_u8; 1024];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                match tls.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => request.extend_from_slice(&buf[..n]),
                }
            }
            let response = format!(
                "HTTP/1.1 {status} Test\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
            );
            let _ = tls.write_all(response.as_bytes()).await;
            let _ = tls.shutdown().await;
        }
    });
    port
}

fn url(port: u16) -> Url {
    Url::parse(&format!("https://localhost:{port}/health?token=secret")).unwrap()
}

#[tokio::test]
async fn http_probe_checks_status_and_cert_expiry() {
    let (cert, key) = certificate(10);
    let port = serve(cert.clone(), key, 200).await;
    let mut h = Harness::new("proc/route");

    let probe =
        HttpProbe::with_roots(url(port), BTreeSet::from([200]), vec![cert.clone()]).unwrap();
    let report = h.run(&probe).await;
    let obs = only(&report);
    assert_eq!(
        obs.target.as_ref().unwrap().as_str(),
        format!("https://localhost:{port}/health")
    );
    assert!(obs.firing, "{obs:#?}");
    assert_eq!(obs.severity, Severity::Warning);
    assert!(
        obs.summary.contains("certificate expires"),
        "{}",
        obs.summary
    );
    assert_eq!(number(obs, "status"), 200.0);
    let days = number(obs, "cert_days_left");
    assert!((8.0..=11.0).contains(&days), "{days}");

    let wrong_status = HttpProbe::with_roots(url(port), BTreeSet::from([204]), vec![cert]).unwrap();
    let obs = h.run(&wrong_status).await;
    assert!(only(&obs).summary.contains("answered 200"));

    // A certificate with plenty of time left and the expected status is fine.
    let (cert, key) = certificate(90);
    let port = serve(cert.clone(), key, 503).await;
    let probe = HttpProbe::with_roots(url(port), BTreeSet::from([200, 503]), vec![cert]).unwrap();
    let report = h.run(&probe).await;
    let obs = only(&report);
    assert!(!obs.firing, "{obs:#?}");
    assert!(number(obs, "cert_days_left") > 80.0);

    // An untrusted certificate is a failed request.
    let (other, _) = certificate(90);
    let untrusted = HttpProbe::with_roots(url(port), BTreeSet::from([503]), vec![other]).unwrap();
    let report = h.run(&untrusted).await;
    let obs = only(&report);
    assert!(obs.firing);
    assert!(!text(obs, "error").is_empty());
}

#[test]
fn http_probe_rejects_unusable_targets() {
    let none = BTreeSet::new();
    let ok = BTreeSet::from([200]);
    let ftp = Url::parse("ftp://nas.lan/").unwrap();
    assert!(HttpProbe::with_roots(ftp, ok.clone(), Vec::new()).is_err());
    let plain = Url::parse("http://nas.lan/").unwrap();
    assert!(HttpProbe::with_roots(plain.clone(), none, Vec::new()).is_err());
    let long = Url::parse(&format!("http://nas.lan/{}", "a".repeat(200))).unwrap();
    assert!(HttpProbe::with_roots(long, ok.clone(), Vec::new()).is_err());
    assert!(HttpProbe::with_roots(plain, ok, Vec::new()).is_ok());
}

#[test]
fn cert_not_after_reads_rcgen_certificate() {
    for (y, m, d) in [(2031, 5, 6), (2049, 12, 31), (2051, 1, 2)] {
        let mut params = CertificateParams::new(vec!["localhost".to_owned()]).unwrap();
        params.not_after = date_time_ymd(y, m, d);
        let cert = params.self_signed(&KeyPair::generate().unwrap()).unwrap();
        let expected: Timestamp = format!("{y:04}-{m:02}-{d:02}T00:00:00Z").parse().unwrap();
        assert_eq!(not_after(cert.der()), Some(expected), "{y}-{m}-{d}");
    }
    assert_eq!(not_after(b""), None);
    assert_eq!(not_after(&[0x30, 0x84, 0xff, 0xff, 0xff, 0xff]), None);
}
