//! The default gateway from `/proc/net/route`, and garbage input for the network parsers.

use std::net::Ipv4Addr;
use std::sync::OnceLock;

use domovoi_checks::net::cert::not_after;
use domovoi_checks::net::{
    GatewayProbe, GatewayStatus, HostPort, gateway_reachable, gateway_status, parse_default_gateway,
};
use domovoi_core::Severity;
use proptest::prelude::*;

use crate::harness::{Harness, fixture, only};

#[tokio::test]
async fn gateway_is_parsed_from_proc_net_route() {
    let route = std::fs::read_to_string(fixture("proc/route/proc/net/route")).unwrap();
    let parsed = parse_default_gateway(&route).unwrap();
    assert_eq!(parsed.interface, "eth0");
    assert_eq!(parsed.gateway, Ipv4Addr::new(192, 168, 1, 1));

    let mut h = Harness::new("proc/no_route");
    assert_eq!(
        gateway_status(&h.fs, h.clock()).await,
        GatewayStatus::NoRoute
    );
    assert!(!gateway_reachable(&h.fs, h.clock()).await);
    let report = h.run(&GatewayProbe::new().unwrap()).await;
    let obs = only(&report);
    assert!(obs.firing && obs.target.is_none());
    assert_eq!(obs.severity, Severity::Warning);
    assert_eq!(obs.summary, "no default route");
}

#[test]
fn gateway_ignores_routes_without_the_gateway_flag() {
    let route = "Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\n\
                 eth0\t00000000\t0101A8C0\t0001\t0\t0\t0\n";
    assert_eq!(parse_default_gateway(route), None);
}

fn sample_certificate() -> &'static [u8] {
    static DER: OnceLock<Vec<u8>> = OnceLock::new();
    DER.get_or_init(|| {
        let params = rcgen::CertificateParams::new(vec!["localhost".to_owned()]).unwrap();
        let key = rcgen::KeyPair::generate().unwrap();
        params.self_signed(&key).unwrap().der().to_vec()
    })
}

proptest! {
    #[test]
    fn network_parsers_never_panic_on_garbage_input(input in ".{0,300}", bytes in proptest::collection::vec(any::<u8>(), 0..300)) {
        let _ = parse_default_gateway(&input);
        let _ = HostPort::parse(&input);
        let _ = not_after(&bytes);
        let _ = not_after(input.as_bytes());
    }

    #[test]
    fn der_reader_survives_corrupted_certificates(flip in 0_usize..400, value in any::<u8>()) {
        let mut der = sample_certificate().to_vec();
        if let Some(byte) = der.get_mut(flip) {
            *byte = value;
        }
        let _ = not_after(&der);
        der.truncate(flip);
        let _ = not_after(&der);
    }
}
