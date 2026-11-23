//! Child process used by the interprocess tests. Each child opens its own descriptor on the lock
//! file. Synchronisation is by stdin/stdout lines, never by sleeping.

use std::io::{BufRead, Write};
use std::path::PathBuf;

use r10::fixture::{FixtureSource, demo_snapshot};
use r10::helper::{Env, Inputs, attest};
use r10::state::{HelperDir, Stage};

fn say(s: &str) {
    let mut o = std::io::stdout().lock();
    let _ = writeln!(o, "{s}");
    let _ = o.flush();
}

fn wait_line() -> String {
    let mut l = String::new();
    let _ = std::io::stdin().lock().read_line(&mut l);
    l.trim().to_owned()
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mode = a.get(1).map_or("", String::as_str);
    let dir = HelperDir(PathBuf::from(a.get(2).cloned().unwrap_or_default()));
    let cfg = std::fs::read_to_string(a.get(3).cloned().unwrap_or_default()).ok();
    let env = Env { now: 1_700_000_000, boot_id: "11111111-1111-4111-8111-111111111111".into(), hub_addrs: vec![], link_peers: vec![] };
    let src = FixtureSource(demo_snapshot());
    let inp = Inputs { config_text: cfg.as_deref(), routes: &src, env: &env };
    match mode {
        // Attest once, print the generation.
        "attest" => {
            say("ready");
            wait_line();
            let r = attest(&dir, [7; 32], &inp, &mut |_| {});
            let g = r.ok().and_then(|a| {
                r10::report::parse_report(&a.report, r10::report::EmptyReasonPolicy::EmptyIffComplete).ok()
            });
            say(&format!("done {}", g.map_or(0, |r| r.generation)));
        }
        // Attest and block after the tmp file is written until killed.
        "attest_block_after_tmp" => {
            let mut hook = |s: Stage| {
                if s == Stage::TmpWritten {
                    say("tmp_written");
                    loop {
                        std::thread::park();
                    }
                }
            };
            let _ = attest(&dir, [7; 32], &inp, &mut hook);
            say("finished");
        }
        // Hold the lock inside the apply path between compare and apply.
        "hold_between_compare_and_apply" => {
            use r10::helper::{ContainRequest, HelperRuntime, apply_contain};
            let mut rt = HelperRuntime { per_hour: 10, ..Default::default() };
            let req = ContainRequest { target: "203.0.113.0/24".into(), ttl_s: 60, expected: None, agent_max_ttl: None, agent_max_active: None, agent_reported_active: None };
            let mut hook = |s: &'static str| {
                if s == "compared" {
                    say("compared");
                    wait_line();
                }
            };
            let r = apply_contain(&dir, &req, &inp, &mut rt, &mut |_| {}, &mut hook);
            say(&format!("result {r:?}"));
        }
        _ => say("unknown mode"),
    }
}
