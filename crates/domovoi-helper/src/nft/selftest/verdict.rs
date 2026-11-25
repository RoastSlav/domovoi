//! What the self-test's counts mean: pass, fail or inconclusive.
//!
//! `Pass` needs no leaked packet in any scenario, a control that leaked more than 1% (so the
//! test could see a window), and the replacement minimum reached. A leaked packet is `Fail`.
//! Everything else, including a minimum not reached, is `Inconclusive`, which blocks.

use domovoi_core::containment::Verdict;

/// The fewest replacements a scenario must complete.
pub const MIN_REPLACEMENTS: u64 = 20_000;
/// The fewest runs of 4 seconds a scenario must make.
pub const MIN_RUNS: u32 = 2;
/// How long one run of senders lasts, in seconds.
pub const RUN_SECONDS: u32 = 4;
/// The most seconds a scenario may take before it is inconclusive.
pub const SCENARIO_CAP_SECONDS: u64 = 120;
/// The share of packets the control must leak for the test to count as able to see a window.
pub const CONTROL_MIN_LEAK_RATIO: f64 = 0.01;

/// What one scenario measured.
#[derive(Debug, Clone, PartialEq)]
pub struct Counts {
    /// The scenario's name.
    pub name: String,
    /// Packets that passed the drop while the exact `confirm` template replaced the element.
    pub leaked: u64,
    /// Packets the senders sent in the replacement runs.
    pub sent: u64,
    /// Replacements completed.
    pub replacements: u64,
    /// Packets that passed the drop in the split `delete`/`add` control.
    pub control_leaked: u64,
    /// Packets the senders sent in the control.
    pub control_sent: u64,
    /// Why the scenario could not finish, when it could not.
    pub fault: Option<String>,
}

impl Counts {
    /// The share of control packets that leaked.
    #[must_use]
    pub fn control_ratio(&self) -> f64 {
        if self.control_sent == 0 {
            0.0
        } else {
            self.control_leaked as f64 / self.control_sent as f64
        }
    }
}

/// The verdict of one set type from its scenarios.
#[must_use]
pub fn verdict(scenarios: &[Counts]) -> Verdict {
    if scenarios.is_empty() {
        return Verdict::Inconclusive {
            reason: "no_scenario_ran".to_owned(),
        };
    }
    let leaked: u64 = scenarios.iter().map(|s| s.leaked).sum();
    if leaked > 0 {
        return Verdict::Fail { leaked };
    }
    if let Some(fault) = scenarios.iter().find_map(|s| s.fault.clone()) {
        return Verdict::Inconclusive { reason: fault };
    }
    if scenarios
        .iter()
        .any(|s| s.control_ratio() <= CONTROL_MIN_LEAK_RATIO)
    {
        return Verdict::Inconclusive {
            reason: "control_did_not_leak".to_owned(),
        };
    }
    if scenarios.iter().any(|s| s.replacements < MIN_REPLACEMENTS) {
        return Verdict::Inconclusive {
            reason: "replacements_not_reached".to_owned(),
        };
    }
    Verdict::Pass
}

/// The lowest replacement count across `scenarios`, as stored in the result.
#[must_use]
pub fn lowest_replacements(scenarios: &[Counts]) -> u64 {
    scenarios.iter().map(|s| s.replacements).min().unwrap_or(0)
}

/// The lowest control leak ratio across `scenarios`.
#[must_use]
pub fn lowest_control_ratio(scenarios: &[Counts]) -> f64 {
    scenarios
        .iter()
        .map(Counts::control_ratio)
        .fold(f64::INFINITY, f64::min)
        .min(1.0)
}
