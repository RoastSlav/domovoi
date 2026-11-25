//! Which settings changes and which future actions need the admin's password again.
//!
//! A change that widens who can act, or that can make the hub run a program, must be
//! confirmed with the password even when the session is valid. The list lives here, as
//! data, so a new setting that carries the same weight cannot be added without saying
//! whether it needs the password: [`crate::settings::patch::is_sensitive`] reads only
//! this list, and a test walks the configuration schema so a new `llm.*.cli` field
//! cannot slip through.
//!
//! [`STEP_UP_ENFORCED_ACTIONS`] names the actions that already ask for the password again,
//! and which service checks it. [`STEP_UP_ACTIONS`] names the actions that need the same
//! confirmation and are not settings changes. Some wait for their feature; `installation.start`
//! is enforced by the package job endpoints, which confirm the password before they create a
//! job. The entries are here so the intent is written down in one place.
//!
//! Cancelling, pausing, resuming and answering an operation are deliberately not on that
//! list: an owner session is enough, as for approving an offered step. `autonomy.resume` is
//! the future global pause switch, not the resume of one operation.

/// How a rule picks settings paths. Paths are dotted leaf paths such as
/// `llm.primary.cli.args`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathRule {
    /// Exactly this path.
    Exact(&'static str),
    /// This path or anything under it. `*` stands for exactly one segment.
    /// `llm.*.cli` covers `llm.primary.cli.command`, `llm.secondary.cli.args`, ...
    Under(&'static str),
}

/// One reason a settings change needs the password again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepUpRule {
    /// The paths it covers.
    pub path: PathRule,
    /// Why a change there matters, in one phrase.
    pub why: &'static str,
}

/// Settings whose change needs the password. Every entry is enforced.
///
/// Any `Change::Secret` is sensitive as well, whatever its path: storing or clearing a
/// secret is a change to `is_sensitive`, not to this list.
pub const STEP_UP_SETTINGS: &[StepUpRule] = &[
    StepUpRule {
        path: PathRule::Exact("discord.allowed_user_ids"),
        why: "widens who can approve actions from chat",
    },
    StepUpRule {
        path: PathRule::Exact("web.trusted_proxies"),
        why: "changes which client addresses the hub trusts",
    },
    StepUpRule {
        path: PathRule::Under("webhooks"),
        why: "webhook secrets and sources decide who can raise incidents",
    },
    StepUpRule {
        path: PathRule::Under("llm.*.cli"),
        why: "the program (and its arguments) the hub runs as the model",
    },
    StepUpRule {
        path: PathRule::Under("metrics"),
        why: "who can read the hub's metrics",
    },
    StepUpRule {
        path: PathRule::Under("retention"),
        why: "changes how long data is kept; applies once configurable retention exists",
    },
    StepUpRule {
        path: PathRule::Exact("backup.keep"),
        why: "lowering the backup count can remove recovery points",
    },
    StepUpRule {
        path: PathRule::Exact("backup.keep_pre_migrate"),
        why: "lowering the pre-upgrade backup count can remove recovery points",
    },
    StepUpRule {
        path: PathRule::Exact("policy.permit_ttl"),
        why: "a longer permit lifetime widens how long an unattended action stays authorized",
    },
    StepUpRule {
        path: PathRule::Exact("policy.circuit_failures"),
        why: "raising the failure count delays the circuit breaker that suspends a rule",
    },
    StepUpRule {
        path: PathRule::Exact("policy.circuit_window"),
        why: "shrinking the window lets the circuit breaker forget failures sooner",
    },
    StepUpRule {
        path: PathRule::Exact("policy.max_permits_per_hour"),
        why: "raising the permit rate widens how much runs unattended",
    },
    StepUpRule {
        path: PathRule::Under("packages"),
        why: "package installation can put code on hosts",
    },
];

/// An action that will need the password again once its feature exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepUpAction {
    /// A stable name for the action, such as `policy.publish`.
    pub id: &'static str,
    /// What has to exist before the action and its confirmation can be built.
    pub applies_once: &'static str,
}

/// An action that already asks for the password again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnforcedStepUp {
    /// A stable name for the action, such as `policy.publish`.
    pub id: &'static str,
    /// The service that checks the password before it acts.
    pub enforced_by: &'static str,
}

/// Actions that ask for the password again today: publishing a policy version, changing a
/// locked prohibition in a policy draft, and pinning or unpinning a helper key (which also
/// needs the full fingerprint typed out).
pub const STEP_UP_ENFORCED_ACTIONS: &[EnforcedStepUp] = &[
    EnforcedStepUp {
        id: "policy.publish",
        enforced_by: "PolicyService",
    },
    EnforcedStepUp {
        id: "policy.lock_changed",
        enforced_by: "PolicyService",
    },
    EnforcedStepUp {
        id: "helper.key_pin",
        enforced_by: "HelperTrust",
    },
    EnforcedStepUp {
        id: "helper.key_unpin",
        enforced_by: "HelperTrust",
    },
];

/// Actions that need the password again and are not settings changes. Each says where the
/// confirmation happens, or what must exist first. `installation.start` is enforced by the
/// package job endpoints (creating a job, retrying a target); the settings service does not
/// check it because starting an installation is not a settings change.
pub const STEP_UP_ACTIONS: &[StepUpAction] = &[
    StepUpAction {
        id: "capability.enable",
        applies_once: "capability packages exist",
    },
    StepUpAction {
        id: "installation.start",
        applies_once: "enforced by the package job endpoints (create and retry), before any approval is made",
    },
    StepUpAction {
        id: "autonomy.resume",
        applies_once: "autonomy (the pause switch) exists",
    },
];

/// Whether `path` is covered by `rule`, compared segment by segment.
#[must_use]
pub fn covers(rule: &PathRule, path: &str) -> bool {
    let (prefix, exact) = match rule {
        PathRule::Exact(prefix) => (*prefix, true),
        PathRule::Under(prefix) => (*prefix, false),
    };
    let prefix: Vec<&str> = prefix.split('.').collect();
    let path: Vec<&str> = path.split('.').collect();
    if exact && path.len() != prefix.len() {
        return false;
    }
    if path.len() < prefix.len() {
        return false;
    }
    prefix
        .iter()
        .zip(&path)
        .all(|(rule_segment, path_segment)| *rule_segment == "*" || rule_segment == path_segment)
}

/// Whether changing `path` needs the password (any [`STEP_UP_SETTINGS`] rule covers it).
#[must_use]
pub fn path_needs_step_up(path: &str) -> bool {
    STEP_UP_SETTINGS.iter().any(|rule| covers(&rule.path, path))
}

#[cfg(test)]
mod tests {
    use super::{
        PathRule, STEP_UP_ACTIONS, STEP_UP_ENFORCED_ACTIONS, STEP_UP_SETTINGS, covers,
        path_needs_step_up,
    };

    #[test]
    fn operation_controls_are_not_step_up_actions() {
        assert!(
            STEP_UP_ACTIONS
                .iter()
                .all(|action| !action.id.starts_with("operation."))
        );
    }

    #[test]
    fn step_up_rules_cover_their_paths() {
        let cases = [
            ("llm.primary.cli.args", true),
            ("llm.primary.cli.command", true),
            ("llm.primary.cli.preset", true),
            ("llm.secondary.cli.command", true),
            ("llm.primary.model", false),
            // A secret is sensitive as a `Change::Secret`, not through a path rule.
            ("llm.primary.api_key", false),
            ("webhooks.grafana.secret", true),
            ("webhooks", true),
            ("webhooks_other", false),
            ("metrics.token", true),
            ("metrics", true),
            ("retention.signals_days", true),
            ("retention", true),
            ("triage.per_hour_runs", false),
            // The wildcard needs exactly one segment.
            ("llm.cli", false),
            ("discord.allowed_user_ids", true),
            ("discord.channel_id", false),
        ];
        for (path, expected) in cases {
            assert_eq!(
                path_needs_step_up(path),
                expected,
                "path_needs_step_up({path})"
            );
        }
    }

    #[test]
    fn removing_a_cli_table_is_a_covered_leaf() {
        use super::super::patch::{Change, apply, is_sensitive};
        use serde_json::{Map, Value, json};

        let map = |value: Value| match value {
            Value::Object(map) => map,
            _ => Map::new(),
        };
        // The document stores a CLI table; removing it reports one leaf per removed key,
        // and emptying an object leaves the table as a leaf of its own.
        let document = map(json!({
            "llm": { "secondary": { "kind": "cli", "cli": { "preset": "claude", "args": [] } } }
        }));
        let patched = apply(
            &document,
            &map(json!({ "llm": { "secondary": { "cli": null } } })),
        )
        .expect("the patch applies");
        assert!(!patched.changes.is_empty());
        for change in &patched.changes {
            assert!(
                is_sensitive(change),
                "{} is under llm.secondary.cli",
                change.path()
            );
        }
        assert!(patched.changes.iter().any(|change| matches!(
            change,
            Change::Plain { path, .. } if path == "llm.secondary.cli.preset"
        )));
    }

    #[test]
    fn packages_enabled_change_requires_step_up() {
        for path in [
            "packages.enabled",
            "packages.approval_window",
            "packages.max_store_bytes",
        ] {
            assert!(path_needs_step_up(path), "{path}");
        }
        assert!(!path_needs_step_up("packages_other"));
    }

    #[test]
    fn exact_rules_match_only_the_whole_path() {
        assert!(covers(
            &PathRule::Exact("web.trusted_proxies"),
            "web.trusted_proxies"
        ));
        assert!(!covers(
            &PathRule::Exact("web.trusted_proxies"),
            "web.trusted_proxies.x"
        ));
        assert!(!covers(
            &PathRule::Exact("webhooks"),
            "webhooks.grafana.secret"
        ));
        assert!(covers(
            &PathRule::Under("webhooks"),
            "webhooks.grafana.secret"
        ));
        assert!(covers(&PathRule::Under("webhooks"), "webhooks"));
    }

    #[test]
    fn enforced_step_up_actions_are_not_declared_as_future() {
        for action in STEP_UP_ENFORCED_ACTIONS {
            assert!(
                STEP_UP_ACTIONS.iter().all(|future| future.id != action.id),
                "{} is enforced, so it is not a future action",
                action.id
            );
            assert!(
                !action.enforced_by.is_empty(),
                "{} names its service",
                action.id
            );
        }
        let ids: Vec<&str> = STEP_UP_ENFORCED_ACTIONS.iter().map(|a| a.id).collect();
        assert!(ids.contains(&"policy.publish") && ids.contains(&"policy.lock_changed"));
    }

    #[test]
    fn step_up_registry_lists_helper_pin_and_unpin_as_enforced() {
        for id in ["helper.key_pin", "helper.key_unpin"] {
            let action = STEP_UP_ENFORCED_ACTIONS
                .iter()
                .find(|action| action.id == id)
                .unwrap_or_else(|| panic!("{id} is registered as enforced"));
            assert_eq!(action.enforced_by, "HelperTrust");
            assert!(
                !STEP_UP_ACTIONS.iter().any(|future| future.id == id),
                "{id} is not still waiting on a feature"
            );
        }
    }

    #[test]
    fn step_up_list_declares_future_actions() {
        let mut ids: Vec<&str> = STEP_UP_ACTIONS.iter().map(|action| action.id).collect();
        ids.sort_unstable();
        let unique = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), unique, "action ids are unique");
        for action in STEP_UP_ACTIONS {
            assert!(
                !action.applies_once.trim().is_empty(),
                "{} says what it waits for",
                action.id
            );
            assert!(
                !STEP_UP_SETTINGS
                    .iter()
                    .any(|rule| covers(&rule.path, action.id)),
                "{} is an action, not a settings path",
                action.id
            );
        }
        for rule in STEP_UP_SETTINGS {
            let text = match rule.path {
                PathRule::Exact(path) | PathRule::Under(path) => path,
            };
            assert!(!text.is_empty(), "a rule names a path");
            assert!(!rule.why.trim().is_empty(), "{text} says why");
        }
    }

    /// Every property path in the schema of `node`, following `$ref` into `$defs` and
    /// every branch of `anyOf`, `oneOf` and `allOf` (how `Option<T>` is emitted).
    fn property_paths(
        node: &serde_json::Value,
        root: &serde_json::Value,
        path: &str,
        out: &mut std::collections::BTreeSet<String>,
    ) {
        for object in objects(node, root) {
            if let Some(properties) = object.get("properties").and_then(|p| p.as_object()) {
                for (key, child) in properties {
                    let path = if path.is_empty() {
                        key.clone()
                    } else {
                        format!("{path}.{key}")
                    };
                    out.insert(path.clone());
                    property_paths(child, root, &path, out);
                }
            }
            if let Some(items) = object.get("items") {
                property_paths(items, root, path, out);
            }
        }
    }

    /// `node` and the object schemas it refers to or chooses between.
    fn objects<'a>(
        node: &'a serde_json::Value,
        root: &'a serde_json::Value,
    ) -> Vec<&'a serde_json::Value> {
        let serde_json::Value::Object(map) = node else {
            return Vec::new();
        };
        if let Some(name) = map
            .get("$ref")
            .and_then(|r| r.as_str())
            .and_then(|r| r.strip_prefix("#/$defs/"))
        {
            return root
                .get("$defs")
                .and_then(|defs| defs.get(name))
                .map(|target| objects(target, root))
                .unwrap_or_default();
        }
        let mut out = vec![node];
        for key in ["anyOf", "oneOf", "allOf"] {
            if let Some(serde_json::Value::Array(branches)) = map.get(key) {
                for branch in branches {
                    out.extend(objects(branch, root));
                }
            }
        }
        out
    }

    #[test]
    fn every_llm_cli_setting_in_the_schema_requires_step_up() {
        let schema = serde_json::to_value(domovoi_config::HubConfig::json_schema())
            .expect("the schema serialises");
        let mut paths = std::collections::BTreeSet::new();
        property_paths(&schema, &schema, "", &mut paths);
        let cli: Vec<&String> = paths
            .iter()
            .filter(|path| path.split('.').any(|segment| segment == "cli"))
            .collect();
        assert!(
            !cli.is_empty(),
            "the walk found the CLI settings; a schema change must not silence this test"
        );
        for path in &cli {
            assert!(
                path_needs_step_up(path),
                "{path} is a CLI setting but does not need the password"
            );
        }
        // A field added to `CliConfig` lands under the same prefix and is covered too.
        assert!(path_needs_step_up("llm.primary.cli.new_field"));
        assert!(path_needs_step_up("llm.secondary.cli.new_field"));
    }
}
