//! N1: local cost of recomputing the set per request. A measurement with stated limits, not a
//! production threshold. Fixture inputs only; no kernel reads are included.

mod common;

use std::time::Instant;

use common::{BOOT_A, Rig, T0, env, lan_snapshot};
use r10::fixture::FixtureSource;
use r10::helper::{Inputs, compute};
use r10::report::{EmptyReasonPolicy, parse_report};

fn stats(name: &str, n: usize, mut us: Vec<f64>) {
    us.sort_by(|a, b| a.partial_cmp(b).expect("no NaN"));
    println!(
        "COST {name}: n={n} median={:.1}us p90={:.1}us min={:.1}us max={:.1}us",
        us[us.len() / 2], us[us.len() * 9 / 10], us[0], us[us.len() - 1]
    );
}

fn time<F: FnMut()>(n: usize, mut f: F) -> Vec<f64> {
    (0..n).map(|_| { let t = Instant::now(); f(); t.elapsed().as_secs_f64() * 1e6 }).collect()
}

#[test]
fn cost_of_recomputing_configuration_and_route_state() {
    let toml = common::base_toml();
    let src = FixtureSource(lan_snapshot());
    let e = env(T0, BOOT_A);
    let n = 2000;
    stats("compute(config+routes, fixture, 5 rules, 5 routes)", n, time(n, || {
        let c = compute(&Inputs { config_text: Some(&toml), routes: &src, env: &e });
        assert!(c.failure().is_none());
    }));
    let rig = Rig::new();
    let bytes = rig.attest_raw([1; 32], T0, BOOT_A).unwrap().report;
    stats("parse_report+check_hashes (hub, strict)", n, time(n, || {
        let r = parse_report(&bytes, EmptyReasonPolicy::EmptyIffComplete).unwrap();
        r.check_hashes().unwrap();
    }));
    let m = 200;
    stats("full attest (lock, read state, compute, sign; unchanged state, no write)", m, time(m, || {
        rig.attest_raw([1; 32], T0, BOOT_A).unwrap();
    }));
    let mut big = lan_snapshot();
    for i in 0..2000u32 {
        big.routes.push(r10::fixture::route(254, &format!("10.{}.{}.0/24", i / 256, i % 256), None, "docker0", 0));
    }
    let bsrc = FixtureSource(big);
    stats("compute with 2000 extra unrelated routes (fixture lookup scans them)", 200, time(200, || {
        assert!(compute(&Inputs { config_text: Some(&toml), routes: &bsrc, env: &e }).failure().is_none());
    }));
}
