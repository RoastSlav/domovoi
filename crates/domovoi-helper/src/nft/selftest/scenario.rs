//! The self-test's world and its scenarios: two throwaway namespaces joined by a veth pair, a
//! table of the production layout under another name, and a replacer that loops the exact
//! `confirm` template through the same executor path the helper uses.
//!
//! Nothing here touches the default namespace, a physical interface, a bridge or the production
//! table. The namespaces are always deleted, whether the scenarios finish or not.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use domovoi_core::containment::templates::{MaxElements, Seconds, TableName, delete_own_table};
use domovoi_core::containment::{Layout, Net, templates};
use domovoi_core::ports::{Clock, HostInspect, ProcessExecutor, ProcessSpec};
use jiff::SignedDuration;

use super::verdict::{Counts, MIN_REPLACEMENTS, MIN_RUNS, RUN_SECONDS, SCENARIO_CAP_SECONDS};
use crate::nft::exec::Runner;
use crate::nft::view::parse;
use crate::reads::tools::{Tool, locate_explained};

/// The table of the self-test. Never the production name.
pub const TABLE: &str = "domovoi_selftest";
/// The receiver's address, inside the namespace that holds the table.
const RX_ADDR: &str = "10.200.0.1";
/// The sender's address; the element under test is this host or a prefix around it.
const TX_ADDR: &str = "10.200.0.100";
/// The TTL the elements are given, long enough that the kernel never removes them mid-run.
const HOLD_SECONDS: u32 = 3600;

/// One scenario: which set type and how many neighbours the element has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scenario {
    /// The name.
    pub name: &'static str,
    /// Whether the element is a prefix in the interval set (else a host in the plain set).
    pub prefixes: bool,
    /// Neighbouring elements below the target.
    pub below: u8,
    /// Neighbouring elements above the target.
    pub above: u8,
}

/// The scenarios of one set type: a lone element, one neighbour above, one below, ten on each
/// side.
#[must_use]
pub fn scenarios(prefixes: bool) -> [Scenario; 4] {
    let (a, b, c, d) = if prefixes {
        (
            "prefix_lone",
            "prefix_above",
            "prefix_below",
            "prefix_both_sides",
        )
    } else {
        ("host_lone", "host_above", "host_below", "host_both_sides")
    };
    [
        Scenario {
            name: a,
            prefixes,
            below: 0,
            above: 0,
        },
        Scenario {
            name: b,
            prefixes,
            below: 0,
            above: 1,
        },
        Scenario {
            name: c,
            prefixes,
            below: 1,
            above: 0,
        },
        Scenario {
            name: d,
            prefixes,
            below: 10,
            above: 10,
        },
    ]
}

/// Why a scenario could not be run.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct WorldError(pub String);

fn fail(what: &str) -> WorldError {
    WorldError(what.to_owned())
}

/// What the world needs from the host.
pub struct Host<'a> {
    /// Runs every process.
    pub exec: &'a dyn ProcessExecutor,
    /// Vets the programs.
    pub inspect: &'a dyn HostInspect,
    /// The clock the caps run on.
    pub clock: &'a dyn Clock,
    /// This binary, re-invoked as the sender.
    pub exe: String,
    /// The suffix of the namespace names, such as the process id.
    pub id: String,
}

impl Host<'_> {
    fn rx(&self) -> String {
        format!("domovoi-st-{}-rx", self.id)
    }

    fn tx(&self) -> String {
        format!("domovoi-st-{}-tx", self.id)
    }

    fn runner(&self) -> Runner<'_> {
        Runner {
            exec: self.exec,
            inspect: self.inspect,
            timeout: SignedDuration::from_secs(5),
            netns: None,
        }
    }

    async fn ip(&self, args: &[&str]) -> Result<(), WorldError> {
        let program = locate_explained(self.inspect, Tool::Ip)
            .map_err(|why| fail(&format!("ip is unusable: {why}")))?;
        let spec = ProcessSpec {
            program: program.to_owned(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            env: BTreeMap::new(),
            cwd: None,
            stdin: None,
            timeout: SignedDuration::from_secs(10),
            max_output_bytes: 65_536,
            run_as: None,
        };
        match self.exec.run(spec).await {
            Ok(out) if out.exit_code == Some(0) => Ok(()),
            Ok(out) => Err(WorldError(format!(
                "ip {} failed: {}",
                args.join(" "),
                out.stderr.lines().next().unwrap_or_default()
            ))),
            Err(e) => Err(WorldError(format!("ip cannot run: {e}"))),
        }
    }

    /// Creates both namespaces and the veth pair.
    pub async fn build(&self) -> Result<(), WorldError> {
        let (rx, tx) = (self.rx(), self.tx());
        let (vrx, vtx) = (format!("vr{}", self.id), format!("vt{}", self.id));
        self.ip(&["netns", "add", &rx]).await?;
        self.ip(&["netns", "add", &tx]).await?;
        self.ip(&["link", "add", &vrx, "type", "veth", "peer", "name", &vtx])
            .await?;
        self.ip(&["link", "set", &vrx, "netns", &rx]).await?;
        self.ip(&["link", "set", &vtx, "netns", &tx]).await?;
        self.ip(&[
            "-n",
            &rx,
            "addr",
            "add",
            &format!("{RX_ADDR}/24"),
            "dev",
            &vrx,
        ])
        .await?;
        self.ip(&[
            "-n",
            &tx,
            "addr",
            "add",
            &format!("{TX_ADDR}/24"),
            "dev",
            &vtx,
        ])
        .await?;
        for (ns, dev) in [(&rx, &vrx), (&tx, &vtx)] {
            self.ip(&["-n", ns, "link", "set", "lo", "up"]).await?;
            self.ip(&["-n", ns, "link", "set", dev, "up"]).await?;
        }
        Ok(())
    }

    /// Deletes both namespaces; the veth pair goes with them. Errors are ignored because the
    /// caller may be cleaning up after a half-built world.
    pub async fn teardown(&self) {
        for ns in [self.rx(), self.tx()] {
            let _ = self.ip(&["netns", "delete", &ns]).await;
        }
    }

    /// Whether either namespace still exists.
    pub async fn leftovers(&self) -> bool {
        let (rx, tx) = (self.rx(), self.tx());
        self.ip(&["netns", "exec", &rx, "true"]).await.is_ok()
            || self.ip(&["netns", "exec", &tx, "true"]).await.is_ok()
    }
}

fn net_at(text: &str) -> Result<Net, WorldError> {
    Net::parse(text).map_err(|e| WorldError(format!("{text}: {e}")))
}

fn neighbour(prefixes: bool, below: bool, step: u32) -> String {
    if prefixes {
        let base = if below { 96 - 8 * step } else { 96 + 8 * step };
        format!("10.200.0.{base}/29")
    } else {
        let host = if below { 100 - step } else { 100 + step };
        format!("10.200.0.{host}/32")
    }
}

fn target(prefixes: bool) -> &'static str {
    if prefixes {
        "10.200.0.96/29"
    } else {
        "10.200.0.100/32"
    }
}

async fn script(runner: &Runner<'_>, text: &str) -> Result<(), WorldError> {
    match runner.script(text).await {
        Ok(run) if run.ok() => Ok(()),
        Ok(run) => Err(WorldError(format!(
            "nft: {}",
            run.stderr.lines().next().unwrap_or_default()
        ))),
        Err(e) => Err(WorldError(e.to_string())),
    }
}

/// Runs one scenario and returns what it measured. The replacement runs use the exact `confirm`
/// template; the control splits it into two separate calls.
pub async fn run_scenario(host: &Host<'_>, s: Scenario) -> Result<Counts, WorldError> {
    let rx = host.rx();
    let table = TableName::parse(TABLE).map_err(|_| fail("table name"))?;
    let max = MaxElements::new(1000).map_err(|_| fail("max elements"))?;
    let runner = Runner {
        netns: Some(rx.as_str()),
        ..host.runner()
    };
    let (replacements, sent, leaked) = measure(host, &runner, &table, max, s, false).await?;
    let (_, control_sent, control_leaked) = measure(host, &runner, &table, max, s, true).await?;
    Ok(Counts {
        name: s.name.to_owned(),
        leaked,
        sent,
        replacements,
        control_leaked,
        control_sent,
        fault: None,
    })
}

async fn prepare(
    runner: &Runner<'_>,
    table: &TableName,
    max: MaxElements,
    s: Scenario,
) -> Result<Net, WorldError> {
    let _ = runner.script(&delete_own_table(table)).await;
    script(
        runner,
        &templates::bootstrap(table, Layout::HostsAndPrefixes, max),
    )
    .await?;
    script(
        runner,
        &format!("add rule inet {TABLE} contain ip saddr {TX_ADDR} counter\n"),
    )
    .await?;
    let hold = Seconds::new(HOLD_SECONDS).map_err(|_| fail("ttl"))?;
    let mut specs: Vec<String> = Vec::new();
    for i in 1..=u32::from(s.below) {
        specs.push(neighbour(s.prefixes, true, i));
    }
    for i in 1..=u32::from(s.above) {
        specs.push(neighbour(s.prefixes, false, i));
    }
    specs.push(target(s.prefixes).to_owned());
    for text in &specs {
        let net = net_at(text)?;
        script(runner, &templates::apply(table, &net)).await?;
        let confirm =
            templates::confirm(table, &net, hold).map_err(|e| WorldError(e.to_string()))?;
        script(runner, &confirm).await?;
    }
    net_at(target(s.prefixes))
}

async fn measure(
    host: &Host<'_>,
    runner: &Runner<'_>,
    table: &TableName,
    max: MaxElements,
    s: Scenario,
    control: bool,
) -> Result<(u64, u64, u64), WorldError> {
    let net = prepare(runner, table, max, s).await?;
    let hold = Seconds::new(HOLD_SECONDS).map_err(|_| fail("ttl"))?;
    let confirm = templates::confirm(table, &net, hold).map_err(|e| WorldError(e.to_string()))?;
    let lines: Vec<&str> = confirm.lines().collect();
    let started = host.clock.now();
    let (mut replacements, mut sent, mut runs) = (0u64, 0u64, 0u32);
    loop {
        let done = AtomicBool::new(false);
        let count = AtomicU64::new(0);
        let senders = async {
            let result = send(host).await;
            done.store(true, Ordering::Release);
            result
        };
        let replacer = async {
            while !done.load(Ordering::Acquire) {
                let ok = if control {
                    let first = script(runner, &format!("{}\n", lines[0])).await.is_ok();
                    let second = script(runner, &format!("{}\n", lines[1])).await.is_ok();
                    first && second
                } else {
                    script(runner, &confirm).await.is_ok()
                };
                if ok {
                    count.fetch_add(1, Ordering::Relaxed);
                }
            }
        };
        let (sent_now, ()) = tokio::join!(senders, replacer);
        sent += sent_now?;
        replacements += count.load(Ordering::Relaxed);
        runs += 1;
        let elapsed = host.clock.now().duration_since(started).as_secs();
        let enough = runs >= MIN_RUNS && (control || replacements >= MIN_REPLACEMENTS);
        let capped = u64::try_from(elapsed).unwrap_or(0) >= SCENARIO_CAP_SECONDS;
        if enough || capped || (control && runs >= MIN_RUNS) {
            break;
        }
    }
    let listing = runner
        .read(templates::list_table_args(table))
        .await
        .map_err(|e| WorldError(e.to_string()))?;
    let view =
        parse(&listing.stdout).map_err(|_| fail("the self-test table listing is not JSON"))?;
    let leaked = view.counters.last().copied().unwrap_or(0);
    Ok((replacements, sent, leaked))
}

/// Starts two senders in the sender namespace at once and sums what they sent.
async fn send(host: &Host<'_>) -> Result<u64, WorldError> {
    let tx = host.tx();
    let ip = locate_explained(host.inspect, Tool::Ip)
        .map_err(|why| fail(&format!("ip is unusable: {why}")))?;
    let one = || {
        let spec = ProcessSpec {
            program: ip.to_owned(),
            args: [
                "netns",
                "exec",
                &tx,
                &host.exe,
                "helper",
                "nft-selftest-sender",
                "--src",
                TX_ADDR,
                "--dst",
                RX_ADDR,
                "--port",
                "9",
                "--seconds",
                &RUN_SECONDS.to_string(),
            ]
            .iter()
            .map(|a| (*a).to_owned())
            .collect(),
            env: BTreeMap::new(),
            cwd: None,
            stdin: None,
            timeout: SignedDuration::from_secs(i64::from(RUN_SECONDS) + 20),
            max_output_bytes: 4096,
            run_as: None,
        };
        host.exec.run(spec)
    };
    let (a, b) = tokio::join!(one(), one());
    let mut total = 0u64;
    for out in [a, b] {
        let out = out.map_err(|e| WorldError(format!("the sender cannot run: {e}")))?;
        let n = out
            .stdout
            .lines()
            .find_map(|l| l.strip_prefix("sent="))
            .and_then(|n| n.trim().parse::<u64>().ok())
            .ok_or_else(|| fail("the sender printed no count"))?;
        total += n;
    }
    Ok(total)
}
