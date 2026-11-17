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

/// Path prefixes where `PolicyPermit::issue` may be called. The permit is a data factory,
/// not an issuer, and M1 has no operational caller (ADR 0021).
pub const POLICY_PERMIT_ALLOWED_PREFIXES: &[&str] = &["crates/domovoi-hub/src/policy/"];

/// Path prefixes where `Authorization::human` may be called: only the hub's auth layer
/// turns an authenticated human into an authorization (INV-1, ADR 0021).
pub const AUTHORIZATION_HUMAN_ALLOWED_PREFIXES: &[&str] =
    &["crates/domovoi-hub/src/auth/", "crates/domovoi-testkit/"];

/// Path prefixes where `Authorization::policy` may be called: only a future policy layer and
/// tests; M1 refuses the case at the writer (INV-1, ADR 0021).
pub const AUTHORIZATION_POLICY_ALLOWED_PREFIXES: &[&str] =
    &["crates/domovoi-hub/src/policy/", "crates/domovoi-testkit/"];

/// Path prefixes where the `plan:v2` domain line may be written: one implementation only
/// (ADR 0021).
pub const PLAN_HASH_V2_ALLOWED_PREFIXES: &[&str] = &["crates/domovoi-core/src/action/"];

/// The one module that may define `Authorization`; a public enum elsewhere would reopen a
/// direct construction path (ADR 0021).
pub const AUTHORIZATION_MODULE: &str = "crates/domovoi-core/src/action/authorization.rs";

/// Where an `AgentLink` may be declared, implemented or faked, and the extra exception those
/// files have for `EXEC-1` to `EXEC-3` (ADR 0022): the one trait file, the hub's real link and
/// policy wrapper, and the fakes. An entry ending in `/` is a directory; any other entry names
/// one file exactly, so a file beside it inherits nothing. Adding an implementation means a
/// deliberate edit here.
pub const LINK_DEFINITION_PREFIXES: &[&str] = &[
    "crates/domovoi-core/src/ports/agent_link.rs",
    "crates/domovoi-hub/src/agent_server/link.rs",
    "crates/domovoi-hub/src/agent_server/skill_policy.rs",
    "crates/domovoi-testkit/",
];

/// Where a host may be read: the one read boundary directory (EXEC-1, ADR 0022), plus the
/// declaration files through [`LINK_DEFINITION_PREFIXES`].
pub const RUN_TOOL_ALLOWED_PREFIXES: &[&str] = &["crates/domovoi-engine/src/evidence/"];

/// Where a skill may be handed to an agent: the one execution boundary file, named exactly
/// (EXEC-2, ADR 0022). `run_capability` is scanned before the capability registry adds it, so
/// the rule starts enforcing the day it lands.
pub const RUN_SKILL_ALLOWED_PREFIXES: &[&str] = &["crates/domovoi-engine/src/actions/execute.rs"];

/// Where a check may be re-run on an agent: the one verification file, named exactly (EXEC-3,
/// ADR 0022).
pub const RUN_CHECK_ALLOWED_PREFIXES: &[&str] =
    &["crates/domovoi-engine/src/actions/verify_wake.rs"];

/// Where a link may be named or taken (EXEC-4, ADR 0022): the code that declares, implements
/// or composes it, plus the engine's two boundaries. The core `ports/` exceptions are exact
/// files - the trait declaration and the module that re-exports it - so another port module
/// added beside them is denied. Everything else is default-deny: a new transport directory
/// is refused without an edit here, and a new allowed area is a deliberate, reviewable one. An
/// entry ending in `/` is a directory; any other entry names one file exactly.
pub const AGENT_LINK_ALLOWED_PREFIXES: &[&str] = &[
    "crates/domovoi-core/src/ports/agent_link.rs",
    "crates/domovoi-core/src/ports/mod.rs",
    "crates/domovoi-hub/src/agent_server/",
    "crates/domovoi-hub/src/runtime/",
    "crates/domovoi-engine/src/evidence/",
    "crates/domovoi-engine/src/actions/",
    "crates/domovoi-testkit/",
];

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
