//! Shared fixtures of the attestation tests: captured `ip -j` output and a scripted host.
//!
//! The JSON under `tests/fixtures/ip/` is real `ip -N -j` output from a Linux network namespace,
//! with documentation-range addresses only. A scripted [`Script`] answers every `ip` call the
//! helper makes for one management network and the hub's address.

use std::collections::BTreeMap;

use domovoi_core::ports::ProcessOutput;
use domovoi_testkit::FakeProcessExecutor;

/// Where the fake `ip` is.
pub const IP: &str = "/usr/sbin/ip";

/// The text of a captured fixture.
pub fn fixture(name: &str) -> String {
    let path = format!(
        "{}/tests/fixtures/ip/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"))
}

/// A process that exited 0 and printed `stdout`.
pub fn ok(stdout: impl Into<String>) -> ProcessOutput {
    ProcessOutput {
        exit_code: Some(0),
        stdout: stdout.into(),
        ..ProcessOutput::default()
    }
}

/// The kernel's answer for a routing table that does not exist: `[]`, an error and exit 2.
pub fn no_such_table(family: &str) -> ProcessOutput {
    ProcessOutput {
        exit_code: Some(2),
        stdout: "[]\n".to_owned(),
        stderr: format!("Error: {family}: FIB table does not exist.\nDump terminated\n"),
        ..ProcessOutput::default()
    }
}

/// What `ip` answers for the host: one JSON text per read.
#[derive(Debug, Clone)]
pub struct Script {
    pub rules4: String,
    pub rules6: String,
    /// `(table, v4 output, v6 output)`; a table with no entry here does not exist.
    pub tables: BTreeMap<u32, (String, String)>,
    pub addresses: String,
    /// `(endpoint, lookup output)`.
    pub lookups: BTreeMap<String, String>,
}

impl Script {
    /// A stock host with a default route via a gateway on `dummy0`, a management network
    /// `198.51.100.0/24` reached through it and a hub at `192.0.2.77` on the link.
    pub fn stock() -> Self {
        let mut tables = BTreeMap::new();
        tables.insert(254, (fixture("routes_main_v4"), fixture("routes_main_v6")));
        tables.insert(255, (fixture("routes_local_v4"), "[]".to_owned()));
        let mut lookups = BTreeMap::new();
        lookups.insert(
            "198.51.100.0".to_owned(),
            fixture("get_unicast_v4").replace("198.51.100.7", "198.51.100.0"),
        );
        lookups.insert("192.0.2.77".to_owned(), fixture("get_onlink_v4"));
        Self {
            rules4: fixture("rules_v4_default"),
            rules6: fixture("rules_v6_default"),
            tables,
            addresses: fixture("addresses"),
            lookups,
        }
    }

    /// Scripts every call into `exec`, replacing earlier answers.
    pub fn apply(&self, exec: &FakeProcessExecutor) {
        exec.expect(
            IP,
            &["-N", "-j", "-4", "rule", "show"],
            ok(self.rules4.clone()),
        );
        exec.expect(
            IP,
            &["-N", "-j", "-6", "rule", "show"],
            ok(self.rules6.clone()),
        );
        for table in [100_u32, 101, 102, 103, 104, 253, 254, 255] {
            let number = table.to_string();
            for (flag, family, pick) in [("-4", "ipv4", 0), ("-6", "ipv6", 1)] {
                let args = ["-N", "-j", flag, "route", "show", "table", number.as_str()];
                let out = self.tables.get(&table).map_or_else(
                    || no_such_table(family),
                    |(v4, v6)| ok(if pick == 0 { v4.clone() } else { v6.clone() }),
                );
                exec.expect(IP, &args, out);
            }
        }
        exec.expect(
            IP,
            &["-N", "-j", "addr", "show"],
            ok(self.addresses.clone()),
        );
        for (endpoint, json) in &self.lookups {
            let flag = if endpoint.contains(':') { "-6" } else { "-4" };
            exec.expect(
                IP,
                &["-N", "-j", "-d", flag, "route", "get", endpoint.as_str()],
                ok(json.clone()),
            );
        }
    }
}
