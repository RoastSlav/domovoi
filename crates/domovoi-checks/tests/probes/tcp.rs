//! The TCP probe against loopback listeners.

use domovoi_checks::net::{HostPort, TcpProbe};
use domovoi_core::{Criticality, Severity};
use tokio::net::TcpListener;

use crate::harness::{Harness, only, text};

#[tokio::test]
async fn tcp_probe_reports_down_on_refused_connection() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let target = HostPort::parse(&format!("127.0.0.1:{port}")).unwrap();
    let probe = TcpProbe::new(target).unwrap();
    let mut h = Harness::new("proc/route");

    let up = h.run(&probe).await;
    let obs = only(&up);
    assert!(!obs.firing, "{obs:#?}");
    assert_eq!(
        obs.target.as_ref().unwrap().as_str(),
        format!("127.0.0.1:{port}")
    );

    drop(listener);
    let down = h.run(&probe).await;
    let obs = only(&down);
    assert!(obs.firing);
    assert_eq!(obs.severity, Severity::Warning);
    assert!(!text(obs, "error").is_empty());

    h.settings.criticality = Some(Criticality::Critical);
    assert_eq!(only(&h.run(&probe).await).severity, Severity::Critical);
}

#[test]
fn host_port_accepts_names_and_addresses_only() {
    for good in ["nas.lan:445", "10.0.0.2:22", "[::1]:8080", "router:53"] {
        assert!(HostPort::parse(good).is_ok(), "{good}");
    }
    assert_eq!(
        HostPort::parse("[::1]:8080").unwrap().to_string(),
        "[::1]:8080"
    );
    for bad in [
        "nas.lan",
        "nas:0",
        "nas:70000",
        ":22",
        "-x:22",
        "a b:22",
        "nas;id:22",
    ] {
        assert!(HostPort::parse(bad).is_err(), "{bad}");
    }
}
