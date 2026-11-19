import type { CapabilityState } from "./status";
import type { Table } from "./status";

/** Whether one capability is offered on one host: the hub's eight answers. */
export const CAPABILITY_STATES: Table<CapabilityState> = {
  available: {
    label: "Available",
    tone: "success",
    hint: "The host implements it, the hub enables it and nothing it needs is missing. This is not an approval.",
    icon: "check-circle",
  },
  not_installed: {
    label: "Not installed",
    tone: "neutral",
    hint: "The host's complete implementation list does not contain it.",
    icon: "ban",
  },
  disabled_by_hub: {
    label: "Disabled here",
    tone: "neutral",
    hint: "You switched it off for this host. The agent still has it.",
    icon: "ban",
  },
  not_allowed_on_host: {
    label: "Refused by the host",
    tone: "warning",
    hint: "The host's own unattended ceiling does not cover it.",
    icon: "shield",
  },
  unavailable: {
    label: "Unavailable",
    tone: "warning",
    hint: "The host has it, but something it needs is missing or broken right now.",
    icon: "alert",
  },
  host_unknown: {
    label: "Host unknown",
    tone: "neutral",
    hint: "This host has never connected to this hub run.",
    icon: "clock",
  },
  host_absent: {
    label: "Host offline",
    tone: "danger",
    hint: "This host's agent is not connected.",
    icon: "offline",
  },
  host_incompatible: {
    label: "Incompatible",
    tone: "danger",
    hint: "This host's agent speaks another protocol version; upgrade it, then restart it.",
    icon: "ban",
  },
};
