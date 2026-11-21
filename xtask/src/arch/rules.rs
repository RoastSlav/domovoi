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

/// Crates that may only be used as dev-dependencies (R5). `xtask` exports the architecture
/// checker, which the engine and hub gate tests call; that is a test-only use.
pub const DEV_ONLY: &[&str] = &["domovoi-testkit", "xtask"];

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

/// Raw link operations, by the name written at the call site (ADR 0023). A name matches the
/// last segment of a path (`AgentLink::run_tool`) and the method of a call
/// (`self.link.run_tool`), so an operation is counted wherever it is called.
pub const RAW_OPS: &[&str] = &[
    "run_tool",
    "start_skill",
    "skill_result",
    "run_check",
    "run_skill",
    "run_capability",
    "hand_off",
    "await_handoff_result",
    "send_transfer",
    "start_capability",
    "capability_result",
];

/// Boundary paths, `Type::assoc` as written (EXEC-2, ADR 0022, ADR 0023): the one authorizing
/// constructor, the one dispatch, and the dispatch command. A dispatch command that is the path
/// of a struct expression or a struct pattern is counted by the construction and pattern cards
/// instead; this card sees every other mention of it.
pub const BOUNDARY_PAIRS: &[&str] = &[
    "AuthorizedStep::of",
    "execute::execute_step",
    "AuthorizedPackageTarget::of",
    "execute::execute_package",
    "OperationCommand::Dispatch",
    "Caller::web",
    "Caller::chat",
];

/// The raw link's type names (EXEC-4, ADR 0023). Naming one outside the allowed files is how
/// a transport, a service or a new module would reach the unrestricted link.
pub const LINK_NAMES: &[&str] = &["AgentLink", "WsAgentLink"];

/// Methods and fields that hand out or hold the raw link (EXEC-4, ADR 0023), by the name
/// written after a `.`.
pub const LINK_ACCESSORS: &[&str] = &["link", "link_for_tests"];

/// The same accessors as associated paths, `Type::accessor` after `use` renames and type
/// aliases are resolved (EXEC-4, ADR 0023): `AgentServer::link(&server)`, the function item
/// `AgentServer::link`, and `<AgentServer>::link`. The type is part of the match so an
/// unrelated `link` function on another type is not an accessor.
pub const LINK_ACCESSOR_PATHS: &[(&str, &str)] =
    &[("AgentServer", "link"), ("AgentServer", "link_for_tests")];

/// One operation a boundary card counts.
pub enum BoundaryOp {
    /// A raw link operation, by [`RAW_OPS`] name.
    Call(&'static str),
    /// A path mention that is neither a struct expression nor a struct pattern, by
    /// [`BOUNDARY_PAIRS`] key.
    Pair(&'static str),
    /// A struct expression (`Type::Variant { .. }` building a value), by [`BOUNDARY_PAIRS`] key.
    Construct(&'static str),
    /// A struct pattern (`Type::Variant { .. }` matching a value), by [`BOUNDARY_PAIRS`] key.
    Pattern(&'static str),
}

/// One file allowed to hold a card's operation, and how many times it may appear there.
pub struct BoundarySite {
    /// Workspace-relative path with `/` separators, named exactly.
    pub file: &'static str,
    /// Exact number of occurrences in that file.
    pub count: usize,
}

/// One counted boundary operation: the files allowed to hold it, with exact counts, and the
/// message a violation carries. A card is default-deny: every other production file must
/// contain none of the operation, and a listed file must contain exactly its count.
pub struct BoundaryCard {
    /// Rule identifier, `EXEC-1` to `EXEC-3`.
    pub id: &'static str,
    /// The operation this card counts.
    pub op: BoundaryOp,
    /// Every file allowed to hold it. An empty list means the operation has no caller.
    pub sites: &'static [BoundarySite],
    /// What the operation is for and where it belongs.
    pub message: &'static str,
}

/// Every counted boundary operation (EXEC-1 to EXEC-3, ADR 0023, 0024). Counts are checked
/// against the syntax tree, so a second call site, a call split across lines or a call inside a
/// macro body cannot slip past. The hub's link and its runtime capabilities are the only callers
/// of a raw link operation: the first is the link's own `AgentLink` implementation, the second
/// the composition root that turns it into capability objects. Adding a caller is a deliberate
/// edit here, with an ADR.
pub const BOUNDARY_CARDS: &[BoundaryCard] = &[
    BoundaryCard {
        id: "CALLER-1",
        op: BoundaryOp::Pair("Caller::web"),
        sites: &[BoundarySite {
            file: "crates/domovoi-hub/src/api/mod.rs",
            count: 1,
        }],
        message: "a Caller is built only by a transport after it authenticated the person: the session extractor for the web, the allowlist check for chat",
    },
    BoundaryCard {
        id: "CALLER-1",
        op: BoundaryOp::Pair("Caller::chat"),
        sites: &[BoundarySite {
            file: "crates/domovoi-hub/src/auth/interactions/mod.rs",
            count: 1,
        }],
        message: "a Caller is built only by a transport after it authenticated the person: the session extractor for the web, the allowlist check for chat",
    },
    BoundaryCard {
        id: "EXEC-1",
        op: BoundaryOp::Call("run_tool"),
        sites: &[BoundarySite {
            file: "crates/domovoi-hub/src/runtime/caps.rs",
            count: 1,
        }],
        message: "a host is read only through `HostRead::read_host`; the raw `run_tool` call belongs to the runtime capability (ADR 0024)",
    },
    BoundaryCard {
        id: "EXEC-2",
        op: BoundaryOp::Call("start_skill"),
        sites: &[BoundarySite {
            file: "crates/domovoi-hub/src/runtime/caps.rs",
            count: 1,
        }],
        message: "a skill is dispatched only through `HandoffRunner::hand_off` in operations/execute.rs; the raw `start_skill` call belongs to the runtime capability (ADR 0024)",
    },
    BoundaryCard {
        id: "EXEC-2",
        op: BoundaryOp::Call("skill_result"),
        sites: &[BoundarySite {
            file: "crates/domovoi-hub/src/runtime/caps.rs",
            count: 1,
        }],
        message: "a skill result is awaited only through `HandoffRunner::await_handoff_result` in operations/execute.rs; the raw `skill_result` call belongs to the runtime capability (ADR 0024)",
    },
    BoundaryCard {
        id: "EXEC-2",
        op: BoundaryOp::Call("run_skill"),
        sites: &[],
        message: "`run_skill` has no caller: a skill runs only through `execute::execute_step(AuthorizedStep)` in operations/execute.rs (ADR 0023)",
    },
    BoundaryCard {
        id: "EXEC-2",
        op: BoundaryOp::Call("run_capability"),
        sites: &[],
        message: "`run_capability` has no caller; a skill runs only through `execute::execute_step(AuthorizedStep)` in operations/execute.rs (ADR 0023)",
    },
    BoundaryCard {
        id: "EXEC-2",
        op: BoundaryOp::Call("send_transfer"),
        sites: &[BoundarySite {
            file: "crates/domovoi-hub/src/runtime/caps.rs",
            count: 1,
        }],
        message: "a package file reaches an agent only through `PackageRelay::relay_transfer`, called by the install service's one relay in domovoi-engine/src/packages/execute.rs; the raw `send_transfer` call belongs to the runtime capability (ADR 0029)",
    },
    BoundaryCard {
        id: "EXEC-2",
        op: BoundaryOp::Call("start_capability"),
        sites: &[BoundarySite {
            file: "crates/domovoi-hub/src/runtime/caps.rs",
            count: 1,
        }],
        message: "a package capability is started only through `PackageRelay::dispatch_package`, called by the install service's one relay in domovoi-engine/src/packages/execute.rs; the raw `start_capability` call belongs to the runtime capability (ADR 0029)",
    },
    BoundaryCard {
        id: "EXEC-2",
        op: BoundaryOp::Call("capability_result"),
        sites: &[BoundarySite {
            file: "crates/domovoi-hub/src/runtime/caps.rs",
            count: 1,
        }],
        message: "a package capability's answer is awaited only through `PackageRelay::package_result`, called by the install service's one relay in domovoi-engine/src/packages/execute.rs; the raw `capability_result` call belongs to the runtime capability (ADR 0029)",
    },
    BoundaryCard {
        id: "EXEC-2",
        op: BoundaryOp::Call("hand_off"),
        sites: &[BoundarySite {
            file: "crates/domovoi-engine/src/operations/execute.rs",
            count: 1,
        }],
        message: "a skill is handed to an agent only through `HandoffRunner::hand_off`, once, in operations/execute.rs (ADR 0022)",
    },
    BoundaryCard {
        id: "EXEC-2",
        op: BoundaryOp::Call("await_handoff_result"),
        sites: &[BoundarySite {
            file: "crates/domovoi-engine/src/operations/execute.rs",
            count: 1,
        }],
        message: "a skill result is awaited only through `HandoffRunner::await_handoff_result`, once, in operations/execute.rs (ADR 0022)",
    },
    BoundaryCard {
        id: "EXEC-2",
        op: BoundaryOp::Pair("AuthorizedStep::of"),
        sites: &[BoundarySite {
            file: "crates/domovoi-engine/src/operations/effects/dispatch.rs",
            count: 1,
        }],
        message: "`AuthorizedStep::of` is the one authorizing constructor of the operation service and runs once per execution, in effects/dispatch.rs (ADR 0022)",
    },
    BoundaryCard {
        id: "EXEC-2",
        op: BoundaryOp::Pair("execute::execute_step"),
        sites: &[BoundarySite {
            file: "crates/domovoi-engine/src/operations/effects/dispatch.rs",
            count: 1,
        }],
        message: "`execute::execute_step` is the operation service's one dispatch and is called once, from effects/dispatch.rs (ADR 0022)",
    },
    BoundaryCard {
        id: "EXEC-2",
        op: BoundaryOp::Pair("AuthorizedPackageTarget::of"),
        sites: &[BoundarySite {
            file: "crates/domovoi-engine/src/packages/worker.rs",
            count: 1,
        }],
        message: "`AuthorizedPackageTarget::of` is the one authorizing constructor of the install service and runs once per target, in packages/worker.rs (ADR 0029)",
    },
    BoundaryCard {
        id: "EXEC-2",
        op: BoundaryOp::Pair("execute::execute_package"),
        sites: &[BoundarySite {
            file: "crates/domovoi-engine/src/packages/worker.rs",
            count: 1,
        }],
        message: "`execute::execute_package` is the install service's one relay and is called once, from packages/worker.rs (ADR 0029)",
    },
    BoundaryCard {
        id: "EXEC-2",
        op: BoundaryOp::Construct("OperationCommand::Dispatch"),
        sites: &[BoundarySite {
            file: "crates/domovoi-engine/src/operations/execute.rs",
            count: 1,
        }],
        message: "`OperationCommand::Dispatch { .. }` is built once, in operations/execute.rs; a second struct expression is a second dispatch path (ADR 0022)",
    },
    BoundaryCard {
        id: "EXEC-2",
        op: BoundaryOp::Pattern("OperationCommand::Dispatch"),
        sites: &[BoundarySite {
            file: "crates/domovoi-engine/src/operations/writer/mod.rs",
            count: 1,
        }],
        message: "`OperationCommand::Dispatch { .. }` is matched once, in operations/writer/mod.rs; a second struct pattern is a second place that acts on a dispatch (ADR 0022)",
    },
    BoundaryCard {
        id: "EXEC-2",
        op: BoundaryOp::Pair("OperationCommand::Dispatch"),
        sites: &[],
        message: "`OperationCommand::Dispatch` may appear only as the struct expression in operations/execute.rs and the struct pattern in operations/writer/mod.rs; any other mention (a path, a tuple pattern, a macro body) is not one of the two sites (ADR 0022)",
    },
    BoundaryCard {
        id: "EXEC-3",
        op: BoundaryOp::Call("run_check"),
        sites: &[BoundarySite {
            file: "crates/domovoi-hub/src/runtime/caps.rs",
            count: 1,
        }],
        message: "a check is re-run only through `CheckRerun::rerun_check` in operations/effects/verify.rs; the raw `run_check` call belongs to the runtime capability (ADR 0024)",
    },
];

/// The hub's source root, workspace-relative (MOD-1 to MOD-4, ADR 0026).
pub const HUB_SRC: &str = "crates/domovoi-hub/src/";

/// Whether a zone is a way in for a person or a webhook, or the use-case layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneKind {
    /// Authenticates, parses, calls one service and renders the answer.
    Transport,
    /// Holds the commands every transport shares.
    Service,
}

/// A part of the hub the module-edge rules treat as one unit.
pub struct Zone {
    /// The zone's name, for messages.
    pub name: &'static str,
    /// The module below `crate`, `::` separated; it covers every module under it.
    pub module: &'static str,
    /// What the zone is.
    pub kind: ZoneKind,
}

/// The hub's transports and its use-case layer (MOD-1, MOD-2, MOD-3).
pub const ZONES: &[Zone] = &[
    Zone {
        name: "http",
        module: "api",
        kind: ZoneKind::Transport,
    },
    Zone {
        name: "chat",
        module: "auth::interactions",
        kind: ZoneKind::Transport,
    },
    Zone {
        name: "webhook",
        module: "ingest",
        kind: ZoneKind::Transport,
    },
    Zone {
        name: "services",
        module: "services",
        kind: ZoneKind::Service,
    },
];

/// Modules services may not import besides the transports: the composition root (MOD-2).
pub const SERVICE_FORBIDDEN_MODULES: &[&str] = &["runtime", "router"];

/// Command handles transports may not name: commands go through a service (MOD-4).
pub const TRANSPORT_FORBIDDEN_NAMES: &[&str] =
    &["ActionHandle", "OperationHandle", "PackageHandle"];

/// Top-level hub modules that are not a zone. A module neither here nor a zone is MOD-3.
pub const UNZONED_MODULES: &[&str] = &[
    "admin",
    "agent_server",
    "auth",
    "capability",
    "csp",
    "digest",
    "middleware",
    "packages",
    "profile",
    "router",
    "runtime",
    "selftest",
    "settings",
    "spa",
    "version",
];

/// Crates whose production sources the counted operations cover (EXEC-1 to EXEC-3).
pub const EXEC_OPERATION_CRATES: &[&str] = &[
    "crates/domovoi-core/src/",
    "crates/domovoi-engine/src/",
    "crates/domovoi-hub/src/",
    "crates/domovoi-store/src/",
];

/// Crates whose production sources may not name the raw link (EXEC-4). The binary is included
/// because it composes the hub: a first-party source there that reached for the link would
/// bypass every boundary.
pub const EXEC_LINK_CRATES: &[&str] = &[
    "crates/domovoi-core/src/",
    "crates/domovoi-engine/src/",
    "crates/domovoi-hub/src/",
    "crates/domovoi/src/",
];

/// Files allowed to name the raw link or take it through an accessor (EXEC-4, ADR 0023, 0024):
/// the trait and its re-export, the hub's link and the server module that owns it, and the
/// runtime composition root that turns it into capabilities. Exact files only: a new module in
/// the same directory inherits nothing.
pub const LINK_NAMES_ALLOWED_FILES: &[&str] = &[
    "crates/domovoi-core/src/ports/agent_link.rs",
    "crates/domovoi-core/src/ports/mod.rs",
    "crates/domovoi-hub/src/agent_server/link.rs",
    "crates/domovoi-hub/src/agent_server/link_logs.rs",
    "crates/domovoi-hub/src/agent_server/link_package.rs",
    "crates/domovoi-hub/src/agent_server/mod.rs",
    "crates/domovoi-hub/src/runtime/caps.rs",
    "crates/domovoi-hub/src/runtime/mod.rs",
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

/// Rule id for source the checker cannot inspect (SRC-1, ADR 0023): a module or `include!`
/// that is missing, escapes the workspace, or is spelled in a form the checker cannot resolve.
pub const SRC_RULE: &str = "SRC-1";

/// Maximum lines in a non-test Rust source file (R8).
pub const MAX_FILE_LINES: usize = 500;

/// Directory whose files are append-only once merged (R13).
pub const MIGRATIONS_DIR: &str = "crates/domovoi-store/migrations/";

/// Changing this file requires a new ADR (ADR 0001).
pub const RULES_FILE: &str = "xtask/src/arch/rules.rs";

/// Where ADRs live.
pub const ADR_DIR: &str = "docs/adr/";
