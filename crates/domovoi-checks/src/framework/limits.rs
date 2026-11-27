//! What a hub-sent check override may contain: the interval range per check and the
//! threshold keys with their allowed values. The hub checks it before it saves and sends;
//! the agent checks it again on receipt, so a mistyped key never looks armed.

use serde_json::{Map, Value};

/// The longest interval an override may set, in seconds (one day).
pub const MAX_INTERVAL_SECONDS: u32 = 86_400;

/// What one threshold holds.
#[derive(Clone, Copy)]
enum Kind {
    Num(f64, f64),
    Int(u64, u64),
    Strings,
    /// A map of mount point to `warning_free_pct` and `critical_free_pct`.
    Mounts,
}

type Spec = &'static [(&'static str, Kind)];

const PCT: Kind = Kind::Num(0.0, 100.0);

/// The checks an override may name, with the smallest interval and the threshold keys.
const CHECKS: &[(&str, u32, Spec)] = &[
    (
        "disk.free",
        60,
        &[
            ("warning_free_pct", PCT),
            ("critical_free_pct", PCT),
            ("mounts", Kind::Mounts),
            ("ignore", Kind::Strings),
        ],
    ),
    (
        "memory.available",
        10,
        &[
            ("warning_available_gib", Kind::Num(0.0, 4096.0)),
            ("critical_available_gib", Kind::Num(0.0, 4096.0)),
            ("warning_swap_used_pct", PCT),
        ],
    ),
    (
        "power.supply",
        10,
        &[("low_battery_pct", PCT), ("charge_limit_pct", PCT)],
    ),
    (
        "temperature.cpu",
        10,
        &[
            ("warning_c", Kind::Num(0.0, 150.0)),
            ("critical_c", Kind::Num(0.0, 150.0)),
        ],
    ),
    (
        "unit.watch",
        10,
        &[
            ("units", Kind::Strings),
            ("critical", Kind::Strings),
            ("restarts", Kind::Int(1, 1000)),
        ],
    ),
    ("unit.failed", 10, &[]),
    (
        "container.restart_loop",
        10,
        &[("restarts", Kind::Int(1, 1000))],
    ),
    ("container.expected_down", 10, &[]),
    ("container.unhealthy", 10, &[]),
    ("container.new", 10, &[]),
    ("kernel.errors", 10, &[]),
    ("smart.health", 300, &[]),
    ("system.reboot", 60, &[]),
    ("system.reboot_required", 60, &[]),
    (
        "security.fail2ban",
        60,
        &[
            ("jails", Kind::Strings),
            ("bans_per_hour", Kind::Int(1, 100_000)),
        ],
    ),
    (
        "security.new_port",
        10,
        &[("expiry_days", Kind::Int(0, 3650))],
    ),
    ("security.listener_gone", 10, &[]),
    ("security.ssh_login", 10, &[]),
    ("security.sudo_failure", 10, &[]),
];

/// The smallest interval, in seconds, an override may set for `check`; `None` when no
/// built-in check has that key.
#[must_use]
pub fn min_interval_seconds(check: &str) -> Option<u32> {
    CHECKS.iter().find(|c| c.0 == check).map(|c| c.1)
}

/// Whether `check` is a hub-side probe, whose settings the hub itself owns.
fn is_probe(check: &str) -> bool {
    check.starts_with("probe.")
}

/// Why an override is refused. The text names the key and the allowed range.
///
/// # Errors
/// An unknown check, an interval outside its range, an unknown threshold key, or a
/// threshold value of the wrong type or outside its range.
pub fn validate_override(
    check: &str,
    interval_seconds: Option<u32>,
    thresholds: &std::collections::BTreeMap<String, Value>,
) -> Result<(), String> {
    if is_probe(check) {
        return Ok(());
    }
    let Some((_, min, spec)) = CHECKS.iter().find(|c| c.0 == check) else {
        return Err(format!("{check}: no check has this key"));
    };
    if let Some(seconds) = interval_seconds
        && !(*min..=MAX_INTERVAL_SECONDS).contains(&seconds)
    {
        return Err(format!(
            "{check}.interval_seconds: {seconds} is outside {min}..={MAX_INTERVAL_SECONDS} seconds for this check"
        ));
    }
    for (name, value) in thresholds {
        let Some((_, kind)) = spec.iter().find(|s| s.0 == name) else {
            let allowed = if spec.is_empty() {
                "this check has no thresholds".to_owned()
            } else {
                format!(
                    "allowed: {}",
                    spec.iter().map(|s| s.0).collect::<Vec<_>>().join(", ")
                )
            };
            return Err(format!("{check}.thresholds.{name}: unknown key; {allowed}"));
        };
        check_value(&format!("{check}.thresholds.{name}"), *kind, value)?;
    }
    Ok(())
}

fn check_value(path: &str, kind: Kind, value: &Value) -> Result<(), String> {
    match kind {
        Kind::Num(lo, hi) => match value.as_f64() {
            Some(n) if (lo..=hi).contains(&n) => Ok(()),
            Some(n) => Err(format!("{path}: {n} is outside {lo}..={hi}")),
            None => Err(format!("{path}: must be a number in {lo}..={hi}")),
        },
        Kind::Int(lo, hi) => match value.as_u64() {
            Some(n) if (lo..=hi).contains(&n) => Ok(()),
            Some(n) => Err(format!("{path}: {n} is outside {lo}..={hi}")),
            None => Err(format!("{path}: must be a whole number in {lo}..={hi}")),
        },
        Kind::Strings => match value.as_array() {
            Some(items) if items.iter().all(Value::is_string) => Ok(()),
            _ => Err(format!("{path}: must be a list of strings")),
        },
        Kind::Mounts => {
            let Some(map) = value.as_object() else {
                return Err(format!("{path}: must be a table of mount points"));
            };
            for (mount, entry) in map {
                let Some(entry) = entry.as_object() else {
                    return Err(format!("{path}.{mount}: must be a table"));
                };
                mount_entry(&format!("{path}.{mount}"), entry)?;
            }
            Ok(())
        }
    }
}

fn mount_entry(path: &str, entry: &Map<String, Value>) -> Result<(), String> {
    for (name, value) in entry {
        if name != "warning_free_pct" && name != "critical_free_pct" {
            return Err(format!(
                "{path}.{name}: unknown key; allowed: warning_free_pct, critical_free_pct"
            ));
        }
        check_value(&format!("{path}.{name}"), PCT, value)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::{Value, json};

    use super::{min_interval_seconds, validate_override};

    fn t(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect()
    }

    #[test]
    fn a_one_second_interval_is_refused_with_the_range() {
        let err = validate_override("disk.free", Some(1), &t(&[])).unwrap_err();
        assert!(err.contains("disk.free.interval_seconds"), "{err}");
        assert!(err.contains("60..=86400"), "{err}");
        assert!(validate_override("disk.free", Some(300), &t(&[])).is_ok());
        assert!(validate_override("disk.free", Some(90_000), &t(&[])).is_err());
    }

    #[test]
    fn an_unknown_threshold_key_names_the_key_and_the_allowed_ones() {
        let err = validate_override("disk.free", None, &t(&[("bogus_key", json!(1))])).unwrap_err();
        assert!(err.contains("bogus_key"), "{err}");
        assert!(err.contains("warning_free_pct"), "{err}");
        let err = validate_override("kernel.errors", None, &t(&[("x", json!(1))])).unwrap_err();
        assert!(err.contains("no thresholds"), "{err}");
    }

    #[test]
    fn values_are_range_and_type_checked() {
        let bad = t(&[("warning_free_pct", json!(150))]);
        assert!(
            validate_override("disk.free", None, &bad)
                .unwrap_err()
                .contains("0..=100")
        );
        let bad = t(&[("restarts", json!("three"))]);
        assert!(validate_override("unit.watch", None, &bad).is_err());
        let good = t(&[
            ("warning_free_pct", json!(20)),
            ("mounts", json!({"/": {"critical_free_pct": 5}})),
        ]);
        assert!(validate_override("disk.free", None, &good).is_ok());
    }

    #[test]
    fn unknown_checks_are_refused_and_probes_pass() {
        assert!(validate_override("nope.nothing", None, &t(&[])).is_err());
        assert!(validate_override("probe.http", Some(5), &t(&[])).is_ok());
        assert_eq!(min_interval_seconds("disk.free"), Some(60));
    }
}
