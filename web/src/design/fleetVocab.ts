import type { LiveStatus } from "../api/events";
import type {
  Table,
  HostConnection,
  IssueSeverity,
  HostKind,
  CheckState,
  ContainerState,
  SshLogin,
  StepState,
} from "./status";

/** The vocabularies of the fleet: hosts, the live stream, checks, logins, settings issues and guided steps. */

export const HOST_CONNECTIONS: Table<HostConnection> = {
  connected: {
    label: "Connected",
    tone: "success",
    hint: "The agent is connected and reporting.",
    icon: "check-circle",
  },
  offline: {
    label: "Offline",
    tone: "danger",
    hint: "The agent is not connected; checks on this host are not running.",
    icon: "offline",
  },
  never_connected: {
    label: "Never connected",
    tone: "neutral",
    hint: "The agent was enrolled but has not connected yet.",
    icon: "clock",
  },
  hub_only: {
    label: "Hub only, no agent",
    tone: "neutral",
    hint: "The hub records its own problems here. No agent watches this machine.",
    icon: "server",
  },
  incompatible: {
    label: "Incompatible",
    tone: "danger",
    hint: "The agent speaks another protocol version than the hub. Upgrade the agent, then restart it.",
    icon: "ban",
  },
  expected_incompatible: {
    label: "Expected incompatible",
    tone: "warning",
    hint: "The hub was upgraded and this agent has not reconnected yet. It will be incompatible if it still speaks the old protocol.",
    icon: "alert",
  },
};

export const LIVE: Table<LiveStatus> = {
  live: { label: "Live", tone: "success", hint: "Changes appear as they happen.", icon: "dot" },
  connecting: {
    label: "Connecting",
    tone: "progress",
    hint: "Connecting to the hub for live updates.",
    icon: "loader",
  },
  offline: {
    label: "Offline",
    tone: "danger",
    hint: "Live updates are paused; the page may be out of date until the hub answers.",
    icon: "offline",
  },
};

export const ISSUE_SEVERITIES: Table<IssueSeverity> = {
  error: {
    label: "Error",
    tone: "danger",
    hint: "Must be fixed before the settings can be saved.",
    icon: "x-circle",
  },
  warning: {
    label: "Warning",
    tone: "warning",
    hint: "The settings can be saved, but this probably is not what you want.",
    icon: "alert",
  },
};

export const HOST_KINDS: Table<HostKind> = {
  local: { label: "Hub", tone: "neutral", hint: "The machine the hub runs on.", icon: "server" },
  agent: {
    label: "Agent",
    tone: "neutral",
    hint: "A machine watched by an agent installed on it.",
    icon: "server",
  },
  container_agent: {
    label: "Container agent",
    tone: "neutral",
    hint: "An agent that runs in a container and watches its host through mounts.",
    icon: "server",
  },
  external: {
    label: "External",
    tone: "neutral",
    hint: "Something that reports to the hub through a webhook; no agent runs there.",
    icon: "external",
  },
};

export const CHECK_STATES: Table<CheckState> = {
  ok: {
    label: "OK",
    tone: "success",
    hint: "The check runs and has nothing open.",
    icon: "check-circle",
  },
  failing: {
    label: "Failing",
    tone: "danger",
    hint: "The check has an incident that is not resolved yet.",
    icon: "alert-circle",
  },
  disabled: {
    label: "Disabled",
    tone: "neutral",
    hint: "The check is switched off, so it reports nothing on this host.",
    icon: "ban",
  },
};

export const CONTAINER_STATES: Table<ContainerState> = {
  healthy: {
    label: "Healthy",
    tone: "success",
    hint: "The container is running and its health check passes.",
    icon: "check-circle",
  },
  running: {
    label: "Running",
    tone: "success",
    hint: "The container is running. It has no health check.",
    icon: "check-circle",
  },
  unhealthy: {
    label: "Unhealthy",
    tone: "danger",
    hint: "The container is running but its health check fails.",
    icon: "alert-circle",
  },
  restarting: {
    label: "Restarting",
    tone: "danger",
    hint: "The container keeps stopping and the engine keeps starting it again.",
    icon: "refresh",
  },
  exited: {
    label: "Exited",
    tone: "warning",
    hint: "The container stopped and is not running.",
    icon: "x-circle",
  },
  other: {
    label: "Other",
    tone: "neutral",
    hint: "The container is in a state with no status of its own, such as created or paused.",
    icon: "dot",
  },
};

export const SSH_LOGINS: Table<SshLogin> = {
  accepted: {
    label: "Accepted",
    tone: "neutral",
    hint: "The login succeeded. Expected logins never alert; others raise an incident.",
    icon: "check",
  },
  failed: {
    label: "Failed",
    tone: "warning",
    hint: "The password or key was refused.",
    icon: "alert",
  },
  many_failures: {
    label: "Many failures",
    tone: "danger",
    hint: "This address failed to log in many times in a row, which looks like someone guessing passwords.",
    icon: "alert-circle",
  },
};

export const STEP_STATES: Table<StepState> = {
  done: { label: "Done", tone: "success", hint: "This step is finished.", icon: "check-circle" },
  current: {
    label: "Current step",
    tone: "progress",
    hint: "This is the step you are on.",
    icon: "chevron-right",
  },
  skipped: {
    label: "Skipped",
    tone: "neutral",
    hint: "You skipped this step. It can be set up later in Settings.",
    icon: "skip",
  },
  upcoming: { label: "Not started", tone: "neutral", hint: "This step comes later.", icon: "dot" },
};
