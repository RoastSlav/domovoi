//! Shared test support: a rig with a temp helper dir, a hub and synthetic route fixtures.

use std::net::IpAddr;
use std::path::PathBuf;

use r10::encoding::Family;
use r10::fixture::{FixtureSource, Snapshot, addr, base_rules, route, rule};
use r10::helper::{Env, Inputs, SignedAttestation, attest};
use r10::hub::{Attestation, Hub};
use r10::keys::{b64, fingerprint};
use r10::lines::Selector;
use r10::state::HelperDir;
use tempfile::TempDir;

pub const HOST: &str = "host-1";
pub const BOOT_A: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
pub const BOOT_B: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
pub const T0: u64 = 1_800_000_000;

pub fn base_toml() -> String {
    "[safety]\nmanagement_cidrs = [\"192.168.1.0/24\", \"2001:db8::/32\"]\nprotected_units = [\"plex\"]\nprotected_containers = [\"homeassistant\"]\n[safety.containment]\nmax_ttl = \"1h\"\nmax_active = 5\n".to_owned()
}

pub fn ip(s: &str) -> IpAddr {
    s.parse().expect("test ip")
}

/// LAN host with a WireGuard tunnel: eth0 192.168.1.20/24, wg0 10.8.0.2/24, table 100 default
/// via wg0 selected by fwmark, IPv6 on eth0.
pub fn lan_snapshot() -> Snapshot {
    let mut rules = base_rules(Family::Inet);
    rules.extend(base_rules(Family::Inet6));
    rules.push(rule(Family::Inet, 100, Selector { fwmark: Some((1, 0xffff_ffff)), ..Selector::default() }, 100));
    Snapshot {
        rules,
        routes: vec![
            route(254, "0.0.0.0/0", Some("192.168.1.1"), "eth0", 100),
            route(254, "192.168.1.0/24", None, "eth0", 100),
            route(100, "0.0.0.0/0", Some("10.8.0.1"), "wg0", 0),
            route(254, "::/0", Some("2001:db8::1"), "eth0", 1024),
            route(254, "2001:db8::/64", None, "eth0", 256),
        ],
        addrs: vec![
            addr("10.8.0.2/24", "wg0"),
            addr("192.168.1.20/24", "eth0"),
            addr("2001:db8::20/64", "eth0"),
            addr("172.17.0.1/16", "docker0"),
        ],
        ..Snapshot::default()
    }
}

pub fn env(now: u64, boot: &str) -> Env {
    Env { now, boot_id: boot.to_owned(), hub_addrs: vec![ip("192.168.1.10")], link_peers: vec![(ip("192.168.1.10"), 7443)] }
}

/// A helper directory (temp), its config, a hub that pinned the helper's key, a snapshot.
pub struct Rig {
    pub tmp: TempDir,
    pub dir: HelperDir,
    pub hub: Hub,
    pub toml: String,
    pub snap: Snapshot,
    pub pk: [u8; 32],
}

impl Rig {
    /// `init` with computed hashes, owner pin through the fingerprint comparison.
    pub fn new() -> Self {
        Self::with_init(true)
    }

    pub fn with_init(computed_hashes: bool) -> Self {
        let tmp = TempDir::new().expect("tempdir");
        let dir = HelperDir(tmp.path().join("domovoi-helper"));
        let toml = base_toml();
        let snap = lan_snapshot();
        let mut rig = Self { tmp, dir, hub: Hub::default(), toml, snap, pk: [0; 32] };
        let hashes = if computed_hashes { rig.hashes() } else { None };
        rig.pk = rig.dir.init(hashes, BOOT_A).expect("init");
        let fp = rig.dir.fingerprint().expect("fingerprint");
        assert_eq!(fp, fingerprint(&rig.pk));
        rig.hub.enroll.pin(HOST, &rig.pk, &fp, true).expect("pin");
        rig
    }

    pub fn hashes(&self) -> Option<([u8; 32], [u8; 32])> {
        let src = FixtureSource(self.snap.clone());
        let e = env(T0, BOOT_A);
        let c = r10::helper::compute(&Inputs { config_text: Some(&self.toml), routes: &src, env: &e });
        Some((c.config.ok()?.hash(), c.routes.ok()?.hash()))
    }

    pub fn state_path(&self) -> PathBuf {
        self.dir.0.join("state")
    }

    pub fn attest_raw(&self, nonce: [u8; 32], now: u64, boot: &str) -> Result<SignedAttestation, r10::helper::Unsigned> {
        let src = FixtureSource(self.snap.clone());
        let e = env(now, boot);
        attest(&self.dir, nonce, &Inputs { config_text: Some(&self.toml), routes: &src, env: &e }, &mut |_| {})
    }

    /// Hub issues a nonce, the helper attests, the message is wrapped for the wire.
    pub fn attest_wire(&mut self, now: u64, boot: &str) -> Attestation {
        let n = self.hub.nonces.issue(HOST, now);
        wire(&self.attest_raw(n, now, boot).expect("signed"))
    }
}

pub fn wire(a: &SignedAttestation) -> Attestation {
    Attestation { public_key: b64(&a.public_key), report: b64(&a.report), signature: b64(&a.signature) }
}

/// Describe the first differing byte of two byte strings (for vector disagreements).
pub fn first_diff(a: &[u8], b: &[u8]) -> String {
    match a.iter().zip(b).position(|(x, y)| x != y) {
        Some(i) => format!("first difference at byte {i}: produced 0x{:02x}, expected 0x{:02x}", a[i], b[i]),
        None => format!("common prefix equal; lengths {} vs {}", a.len(), b.len()),
    }
}

pub fn vec_file(name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/fixtures/vectors/{name}", env!("CARGO_MANIFEST_DIR"))).expect("fixture")
}

pub fn vec_text(name: &str) -> String {
    String::from_utf8(vec_file(name)).expect("utf8 fixture")
}
