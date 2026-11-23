//! The closed sets of refusal and check tokens, each with one stable spelling.
//!
//! Tokens cross the helper socket, the self-test JSON and the hub, so a spelling change is a
//! compatibility break. `ALL` lists every member for the arch rule that demands a test per token.

use serde::{Deserialize, Serialize};

macro_rules! tokens {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $variant:ident => $text:literal,)+ }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        pub enum $name {
            $($(#[$vmeta])* #[serde(rename = $text)] $variant,)+
        }

        impl $name {
            /// Every token, in declaration order.
            pub const ALL: &'static [Self] = &[$(Self::$variant,)+];

            /// The stable spelling used on the wire and in reports.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text,)+ }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

tokens! {
    /// Why the helper refused a request, refused to start, or a local command refused to run.
    RefusalToken {
        /// A wrong mode, owner, group, type or a symlink on helper state; never repaired at start.
        HelperStatePermissions => "helper_state_permissions",
        /// `helper init` has not run.
        Uninitialized => "uninitialized",
        /// The `domovoi-agent` account cannot be resolved.
        AgentAccountMissing => "agent_account_missing",
        /// `helper.toml` does not parse.
        HelperConfigInvalid => "helper_config_invalid",
        /// The connecting uid is not the agent's.
        PeerNotAgent => "peer_not_agent",
        /// The frame length exceeds the cap.
        FrameTooLarge => "frame_too_large",
        /// The body is not a request.
        FrameMalformed => "frame_malformed",
        /// The protocol number is not 1.
        UnsupportedVersion => "unsupported_version",
        /// The `op` string is not in the closed set.
        UnknownOperation => "unknown_operation",
        /// `LISTEN_FDS` is set and not `0`.
        AdoptUnexpectedListenFds => "adopt_unexpected_listen_fds",
        /// Descriptor 0 is not a socket.
        AdoptNotASocket => "adopt_not_a_socket",
        /// The platform record is missing; write groups stay refused.
        PlatformRecordMissing => "platform_record_missing",
        /// The platform record is for another kernel; write groups stay refused.
        PlatformRecordStale => "platform_record_stale",
        /// `helper repair` was run without a terminal; nothing changed.
        ConfirmationRequiresTerminal => "confirmation_requires_terminal",
    }
}

tokens! {
    /// Why a self-test check failed or warned.
    CheckToken {
        /// ST-01: the agent is not the `domovoi-agent` account.
        PrivilegeBoundaryNotInEffect => "privilege_boundary_not_in_effect",
        /// ST-02: the agent's groups are not its own plus `systemd-journal`.
        AgentGroupsUnexpected => "agent_groups_unexpected",
        /// ST-03: the skill account is missing or too privileged.
        SkillAccountInvalid => "skill_account_invalid",
        /// ST-03: the agent account cannot be resolved.
        AgentAccountMissing => "agent_account_missing",
        /// ST-04: a Domovoi account is in the docker group.
        DockerGroupMember => "docker_group_member",
        /// ST-05: the runner holds groups besides its own.
        RunnerGroupsExtra => "runner_groups_extra",
        /// ST-06: the runner holds capabilities or may gain privileges.
        RunnerPrivileges => "runner_privileges",
        /// ST-07: a unit differs from the shipped one.
        UnitModified => "unit_modified",
        /// ST-07: `systemctl show` failed or named no fragment.
        UnitNotInspectable => "unit_not_inspectable",
        /// ST-09: a shipped unit names a numeric uid or gid.
        UnitHardcodedId => "unit_hardcoded_id",
        /// ST-09: a unit executes a path under `/run`, `/tmp` or `/dev/shm`.
        UnitExecUnderRun => "unit_exec_under_run",
        /// ST-10: a path of the table is off its mode, owner, group or type.
        PathModeWrong => "path_mode_wrong",
        /// ST-11: helper state is off its mode, owner, group or type.
        HelperStatePermissions => "helper_state_permissions",
        /// ST-12: a socket node is not `root:domovoi-agent 0660`.
        SocketModeWrong => "socket_mode_wrong",
        /// ST-14: the helper socket did not answer `health`.
        SocketUnreachable => "socket_unreachable",
        /// ST-15: the agent could reach helper state or a root process's environment.
        AgentReachesHelperState => "agent_reaches_helper_state",
        /// ST-16: the agent could connect to the Docker socket.
        AgentReachesDocker => "agent_reaches_docker",
        /// ST-17: a container install exposes an escape surface.
        ContainerEscapeSurface => "container_escape_surface",
        /// ST-18: `/etc/domovoi` is an empty directory created by a mask.
        HostDirectoryCreatedByMask => "host_directory_created_by_mask",
        /// ST-13: the runner's view of the host is not isolated.
        RunnerNotIsolated => "runner_not_isolated",
        /// ST-20: state writes are slow; a warning only.
        StateWriteSlow => "state_write_slow",
    }
}
