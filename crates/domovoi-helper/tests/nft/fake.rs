//! A small stateful `nft` and `conntrack`: enough of the language the helper's templates use to
//! drive the operations without a kernel. Scripts are atomic, as `nft -f` batches are, and the
//! error text follows what the namespace suite recorded.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};

use domovoi_core::ports::{ProcessError, ProcessOutput, ProcessSpec};
use domovoi_testkit::FakeClock;
use serde_json::{Value, json};

#[derive(Clone, Debug)]
pub struct Elem {
    pub v6: bool,
    pub lo: u128,
    pub hi: u128,
    pub expires_at: Option<i64>,
}

#[derive(Clone, Debug, Default)]
pub struct Set {
    pub typ: String,
    pub flags: Vec<String>,
    pub size: Option<u64>,
    pub timeout: Option<u64>,
    pub elems: Vec<Elem>,
}

#[derive(Clone, Debug, Default)]
pub struct Table {
    pub sets: BTreeMap<String, Set>,
    pub chain: Option<(String, i64, String)>,
    pub rules: Vec<String>,
    pub counters: Vec<u64>,
}

#[derive(Default)]
pub struct Nft {
    pub tables: BTreeMap<String, Table>,
    pub flowtable: bool,
    pub conntrack_ok: bool,
    pub scripts: Vec<String>,
}

pub type Shared = Arc<Mutex<Nft>>;

fn ok(stdout: impl Into<String>) -> ProcessOutput {
    ProcessOutput {
        exit_code: Some(0),
        stdout: stdout.into(),
        ..ProcessOutput::default()
    }
}

fn fail(stderr: &str) -> ProcessOutput {
    ProcessOutput {
        exit_code: Some(1),
        stderr: stderr.to_owned(),
        ..ProcessOutput::default()
    }
}

const FILE_EXISTS: &str = "Error: Could not process rule: File exists\n";
const NO_ELEMENT: &str = "Error: Could not process rule: No such file or directory\n";
const BARE_MISSING: &str = "Error: No such file or directory\n";

fn parse_addr(text: &str) -> Option<(bool, u128)> {
    match text.parse::<IpAddr>().ok()? {
        IpAddr::V4(a) => Some((false, u128::from(u32::from(a)))),
        IpAddr::V6(a) => Some((true, u128::from(a))),
    }
}

fn parse_elem(text: &str, now: i64) -> Option<Elem> {
    let mut parts = text.split_whitespace();
    let x = parts.next()?;
    let expires_at = match (parts.next(), parts.next()) {
        (Some("timeout"), Some(t)) => Some(now + t.strip_suffix('s')?.parse::<i64>().ok()?),
        (None, None) => None,
        _ => return None,
    };
    let (v6, lo, hi) = if let Some((a, b)) = x.split_once('-') {
        let (v6, lo) = parse_addr(a)?;
        (v6, lo, parse_addr(b)?.1)
    } else if let Some((a, len)) = x.split_once('/') {
        let (v6, lo) = parse_addr(a)?;
        let bits = if v6 { 128 } else { 32 };
        let host = bits - len.parse::<u32>().ok()?;
        let mask = if host >= 128 {
            u128::MAX
        } else {
            (1u128 << host) - 1
        };
        (v6, lo & !mask, (lo & !mask) | mask)
    } else {
        let (v6, lo) = parse_addr(x)?;
        (v6, lo, lo)
    };
    Some(Elem {
        v6,
        lo,
        hi,
        expires_at,
    })
}

fn braces(line: &str) -> &str {
    let a = line.find('{').map_or(0, |i| i + 1);
    let b = line.rfind('}').unwrap_or(line.len());
    line[a..b].trim()
}

impl Nft {
    pub fn purge(&mut self, now: i64) {
        for table in self.tables.values_mut() {
            for set in table.sets.values_mut() {
                set.elems.retain(|e| e.expires_at.is_none_or(|t| t > now));
            }
        }
    }

    pub fn table(&mut self, name: &str) -> &mut Table {
        self.tables.entry(name.to_owned()).or_default()
    }

    fn line(&mut self, line: &str, now: i64) -> Result<(), String> {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            ["create", "table", "inet", t] => {
                if self.tables.contains_key(*t) {
                    return Err(FILE_EXISTS.to_owned());
                }
                self.tables.insert((*t).to_owned(), Table::default());
            }
            ["delete", "table", "inet", t] => {
                self.tables.remove(*t).ok_or(BARE_MISSING)?;
            }
            ["add", "set", "inet", t, name, ..] => {
                let table = self.tables.get_mut(*t).ok_or(BARE_MISSING)?;
                let mut set = Set::default();
                for field in braces(line)
                    .split(';')
                    .map(str::trim)
                    .filter(|f| !f.is_empty())
                {
                    match field.split_once(' ') {
                        Some(("type", v)) => set.typ = v.to_owned(),
                        Some(("flags", v)) => set.flags = v.split(',').map(str::to_owned).collect(),
                        Some(("timeout", v)) => set.timeout = v.trim_end_matches('s').parse().ok(),
                        Some(("size", v)) => set.size = v.parse().ok(),
                        _ => return Err("Error: syntax error\n".to_owned()),
                    }
                }
                table.sets.insert((*name).to_owned(), set);
            }
            ["add", "chain", "inet", t, ..] => {
                let table = self.tables.get_mut(*t).ok_or(BARE_MISSING)?;
                let inner = braces(line);
                let field = |key: &str| {
                    inner
                        .split(';')
                        .map(str::trim)
                        .find_map(|f| f.strip_prefix(key))
                        .map(|v| v.trim().to_owned())
                };
                let hook = field("type filter hook ")
                    .and_then(|v| v.split(' ').next().map(str::to_owned))
                    .unwrap_or_default();
                let prio = field("type filter hook ")
                    .and_then(|v| {
                        v.split("priority ")
                            .nth(1)
                            .and_then(|p| p.trim().parse().ok())
                    })
                    .unwrap_or(0);
                table.chain = Some((hook, prio, field("policy ").unwrap_or_default()));
            }
            ["add", "rule", "inet", t, "contain", rest @ ..] => {
                let table = self.tables.get_mut(*t).ok_or(BARE_MISSING)?;
                table.rules.push(rest.join(" "));
            }
            ["flush", "set", "inet", t, set] => {
                let table = self.tables.get_mut(*t).ok_or(BARE_MISSING)?;
                table.sets.get_mut(*set).ok_or(BARE_MISSING)?.elems.clear();
            }
            [
                verb @ ("create" | "add" | "delete"),
                "element",
                "inet",
                t,
                set,
                ..,
            ] => {
                let table = self.tables.get_mut(*t).ok_or(BARE_MISSING)?;
                let set = table.sets.get_mut(*set).ok_or(BARE_MISSING)?;
                let interval = set.flags.iter().any(|f| f == "interval");
                for text in braces(line).split(',') {
                    let elem = parse_elem(text.trim(), now).ok_or("Error: syntax error\n")?;
                    let same = |e: &Elem| e.v6 == elem.v6 && e.lo == elem.lo && e.hi == elem.hi;
                    let overlaps = |e: &Elem| e.v6 == elem.v6 && e.lo <= elem.hi && elem.lo <= e.hi;
                    match *verb {
                        "create" => {
                            if set.elems.iter().any(same) {
                                return Err(FILE_EXISTS.to_owned());
                            }
                            if interval && set.elems.iter().any(overlaps) {
                                return Err(
                                    "Error: interval overlaps with an existing one\n".to_owned()
                                );
                            }
                            if set.size.is_some_and(|s| set.elems.len() as u64 >= s) {
                                return Err(
                                    "Error: Could not process rule: Too many open files in system\n"
                                        .to_owned(),
                                );
                            }
                            set.elems.push(elem);
                        }
                        "add" => {
                            if interval && set.elems.iter().any(|e| overlaps(e) && !same(e)) {
                                return Err(
                                    "Error: interval overlaps with an existing one\n".to_owned()
                                );
                            }
                            let present = set.elems.iter().any(same);
                            if !present && set.size.is_some_and(|s| set.elems.len() as u64 >= s) {
                                return Err(
                                    "Error: Could not process rule: Too many open files in system\n"
                                        .to_owned(),
                                );
                            }
                            set.elems.retain(|e| !same(e));
                            set.elems.push(elem);
                        }
                        _ => {
                            let before = set.elems.len();
                            set.elems.retain(|e| !same(e));
                            if set.elems.len() == before {
                                return Err(NO_ELEMENT.to_owned());
                            }
                        }
                    }
                }
            }
            _ => return Err("Error: syntax error\n".to_owned()),
        }
        Ok(())
    }

    fn script(&mut self, text: &str, now: i64) -> ProcessOutput {
        self.scripts.push(text.to_owned());
        self.purge(now);
        let backup: BTreeMap<String, Table> = self.tables.clone();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            if let Err(stderr) = self.line(line, now) {
                self.tables = backup;
                return fail(&stderr);
            }
        }
        ok("")
    }

    fn list(&mut self, name: &str, now: i64) -> ProcessOutput {
        self.purge(now);
        let Some(table) = self.tables.get(name) else {
            return fail(BARE_MISSING);
        };
        let mut items = vec![
            json!({"metainfo": {"version": "1.0.9"}}),
            json!({"table": {"family": "inet", "name": name}}),
        ];
        for (set_name, set) in &table.sets {
            let elems: Vec<Value> = set
                .elems
                .iter()
                .map(|e| {
                    let val = if e.lo == e.hi {
                        json!(addr_text(e.v6, e.lo))
                    } else {
                        json!({"range": [addr_text(e.v6, e.lo), addr_text(e.v6, e.hi)]})
                    };
                    match e.expires_at {
                        Some(t) => json!({"elem": {"val": val, "expires": t - now}}),
                        None => val,
                    }
                })
                .collect();
            let mut set_json =
                json!({"name": set_name, "type": set.typ, "flags": set.flags, "elem": elems});
            if let Some(size) = set.size {
                set_json["size"] = json!(size);
            }
            if let Some(timeout) = set.timeout {
                set_json["timeout"] = json!(timeout);
            }
            items.push(json!({"set": set_json}));
        }
        if let Some((hook, prio, policy)) = &table.chain {
            items.push(json!({"chain": {"name": "contain", "type": "filter", "hook": hook, "prio": prio, "policy": policy}}));
        }
        for (i, rule) in table.rules.iter().enumerate() {
            items.push(json!({"rule": {"chain": "contain", "expr": rule_expr(rule, table.counters.get(i).copied().unwrap_or(0))}}));
        }
        ok(json!({"nftables": items}).to_string())
    }
}

fn addr_text(v6: bool, value: u128) -> String {
    if v6 {
        std::net::Ipv6Addr::from(value).to_string()
    } else {
        std::net::Ipv4Addr::from(value as u32).to_string()
    }
}

fn rule_expr(rule: &str, packets: u64) -> Value {
    let words: Vec<&str> = rule.split_whitespace().collect();
    let mut exprs = Vec::new();
    let mut i = 0;
    while i < words.len() {
        match words[i] {
            "ip" | "ip6" => {
                exprs.push(json!({"match": {"op": "==", "left": {"payload": {"protocol": words[i], "field": words[i + 1]}}, "right": words[i + 2]}}));
                i += 3;
            }
            "counter" => {
                exprs.push(json!({"counter": {"packets": packets, "bytes": 0}}));
                i += 1;
            }
            "accept" => {
                exprs.push(json!({"accept": null}));
                i += 1;
            }
            "drop" => {
                exprs.push(json!({"drop": null}));
                i += 1;
            }
            _ => i += 1,
        }
    }
    Value::Array(exprs)
}

/// The responder for a `ScriptedProcessExecutor`.
pub fn respond(
    nft: Shared,
    clock: Arc<FakeClock>,
) -> impl Fn(&ProcessSpec) -> Result<ProcessOutput, ProcessError> + Send + Sync + 'static {
    move |spec| {
        use domovoi_core::ports::Clock;
        let now = clock.now().as_second();
        let mut nft = nft.lock().map_err(|_| ProcessError::NotFound)?;
        let args: Vec<&str> = spec.args.iter().map(String::as_str).collect();
        if spec.program.ends_with("/conntrack") {
            return Ok(if nft.conntrack_ok {
                ok("")
            } else {
                fail("conntrack failed\n")
            });
        }
        if !spec.program.ends_with("/nft") {
            return Err(ProcessError::NotFound);
        }
        Ok(match args.as_slice() {
            ["--version"] => ok("nftables v1.0.9 (Old Doger)\n"),
            ["-f", "-"] => nft.script(spec.stdin.as_deref().unwrap_or_default(), now),
            ["-j", "list", "table", "inet", name] => nft.list(name, now),
            ["-j", "list", "flowtables"] => {
                if nft.flowtable {
                    ok(json!({"nftables": [{"flowtable": {"name": "ft"}}]}).to_string())
                } else {
                    ok(json!({"nftables": []}).to_string())
                }
            }
            _ => fail("Error: syntax error\n"),
        })
    }
}
