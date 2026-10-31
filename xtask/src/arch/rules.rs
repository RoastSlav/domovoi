//! The architecture rules, as data.
//!
//! This file is the machine-readable form of ARCHITECTURE.md §4 and §15.
//! Changing it requires a new ADR in docs/adr (enforced by `arch-check --base`).

/// One workspace crate and what it may depend on.
pub struct CrateRule {
    /// Package name.
    pub name: &'static str,
    /// Internal crates allowed as normal dependencies.
    pub internal: &'static [&'static str],
    /// If set, the only external crates allowed as normal dependencies.
    pub external_allowlist: Option<&'static [&'static str]>,
}

/// Every workspace member must appear here.
pub const CRATES: &[CrateRule] = &[
    CrateRule {
        name: "domovoi-core",
        internal: &[],
        external_allowlist: Some(&[
            "serde",
            "serde_json",
            "thiserror",
            "jiff",
            "uuid",
            "async-trait",
            "sha2",
            "hex",
            "secrecy",
            "regex",
        ]),
    },
    CrateRule {
        name: "domovoi-config",
        internal: &["domovoi-core"],
        external_allowlist: Some(&[
            "serde",
            "toml",
            "thiserror",
            "secrecy",
            "jiff",
            "schemars",
            "url",
            "uuid",
            "serde_path_to_error",
        ]),
    },
    CrateRule {
        name: "domovoi-proto",
        internal: &["domovoi-core"],
        external_allowlist: Some(&[
            "serde",
            "serde_json",
            "thiserror",
            "jiff",
            "uuid",
            "base64",
            "async-trait",
        ]),
    },
    CrateRule {
        name: "domovoi-engine",
        internal: &["domovoi-core"],
        external_allowlist: Some(&[
            "serde",
            "serde_json",
            "thiserror",
            "jiff",
            "uuid",
            "async-trait",
            "tokio",
            "tokio-util",
            "futures",
            "tracing",
            "regex",
            "sha2",
            "hex",
            "secrecy",
            "metrics",
        ]),
    },
    CrateRule {
        name: "domovoi-store",
        internal: &["domovoi-core"],
        external_allowlist: None,
    },
    CrateRule {
        name: "domovoi-llm",
        internal: &["domovoi-core"],
        external_allowlist: None,
    },
    CrateRule {
        name: "domovoi-chat",
        internal: &["domovoi-core"],
        external_allowlist: None,
    },
    CrateRule {
        name: "domovoi-checks",
        internal: &["domovoi-core"],
        external_allowlist: None,
    },
    CrateRule {
        name: "domovoi-runner",
        internal: &["domovoi-core"],
        external_allowlist: None,
    },
    CrateRule {
        name: "domovoi-agent",
        internal: &[
            "domovoi-core",
            "domovoi-config",
            "domovoi-proto",
            "domovoi-checks",
            "domovoi-runner",
        ],
        external_allowlist: None,
    },
    CrateRule {
        name: "domovoi-hub",
        internal: &[
            "domovoi-core",
            "domovoi-config",
            "domovoi-proto",
            "domovoi-engine",
            "domovoi-store",
            "domovoi-llm",
            "domovoi-chat",
            "domovoi-checks",
        ],
        external_allowlist: None,
    },
    CrateRule {
        name: "domovoi-testkit",
        internal: &["domovoi-core", "domovoi-proto"],
        external_allowlist: None,
    },
    CrateRule {
        name: "domovoi",
        internal: &[
            "domovoi-core",
            "domovoi-config",
            "domovoi-hub",
            "domovoi-agent",
            "domovoi-runner",
        ],
        external_allowlist: None,
    },
    CrateRule {
        name: "xtask",
        internal: &[],
        external_allowlist: None,
    },
];

/// Crates that may only be used as dev-dependencies (R5).
pub const DEV_ONLY: &[&str] = &["domovoi-testkit"];

/// Crates allowed to depend on `anyhow` (R9).
pub const ANYHOW_ALLOWED: &[&str] = &["domovoi", "xtask"];

/// Crates nobody may depend on directly (R11: rustls only).
pub const BANNED: &[&str] = &["openssl", "openssl-sys", "native-tls"];

/// Path prefixes (relative to the workspace root) where processes may be spawned (R3).
pub const PROCESS_ALLOWED_PREFIXES: &[&str] = &["crates/domovoi-runner/"];

/// Path prefixes where `HumanApproval::new` may be called (INV-1, ADR 0007).
pub const HUMAN_APPROVAL_ALLOWED_PREFIXES: &[&str] =
    &["crates/domovoi-hub/src/auth/", "crates/domovoi-testkit/"];

/// Path prefixes where `HumanApproval::restore` may be called: loading a stored approval
/// (INV-1, ADR 0014).
pub const HUMAN_APPROVAL_RESTORE_ALLOWED_PREFIXES: &[&str] =
    &["crates/domovoi-store/src/", "crates/domovoi-testkit/"];

/// Crates whose sources must stay free of I/O (R2).
pub const PURE_CRATE_DIRS: &[&str] = &[
    "crates/domovoi-core/src/",
    "crates/domovoi-proto/src/",
    "crates/domovoi-engine/src/",
];

/// Module paths that pure crates must not use (R2).
pub const PURE_FORBIDDEN_PATHS: &[&str] = &[
    "std::fs",
    "std::net",
    "std::process",
    "tokio::net",
    "tokio::fs",
    "tokio::process",
];

/// Maximum lines in a non-test Rust source file (R8).
pub const MAX_FILE_LINES: usize = 500;

/// Directory whose files are append-only once merged (R13).
pub const MIGRATIONS_DIR: &str = "crates/domovoi-store/migrations/";

/// Changing this file requires a new ADR (ADR 0001).
pub const RULES_FILE: &str = "xtask/src/arch/rules.rs";

/// Where ADRs live.
pub const ADR_DIR: &str = "docs/adr/";
