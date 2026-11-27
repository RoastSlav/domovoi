import { lookup, type StatusMeta, type Table } from "./status";

/** Where one host's part of an install is. */
export type InstallTargetStatus =
  | "queued"
  | "transferring"
  | "dispatched"
  | "succeeded"
  | "failed"
  | "not_run"
  | "outcome_unknown"
  | "cancelled";

/** Where a whole install job is, derived by the hub from its hosts. */
export type InstallJobStatus = "running" | "succeeded" | "failed" | "partial";

/** What a host reports about a package it holds. */
export type HostPackageStatus = "active" | "disabled_untrusted_key" | "missing_files";

/** Whether a host trusts the key that signed a package. */
export type KeyState = "trusted" | "unknown" | "revoked";

/** Whether the archive is in the hub's package store. */
export type FileStatus = "ready" | "missing";

/** Whether the privilege boundary is in effect on a host. */
export type PrivilegeBoundary = "in_effect" | "not_in_effect";

/** How a host's unattended limits reached the hub. */
export type CeilingReporting = "reported" | "not_reported" | "unreadable";

/** Where one host's part of an install is: the hub's eight target statuses. */
export const INSTALL_TARGET_STATUSES: Table<InstallTargetStatus> = {
  queued: {
    label: "Queued",
    tone: "neutral",
    icon: "clock",
    hint: "Approved and waiting for the host's agent to be ready. It starts on its own until the approval window closes, then it is dropped.",
  },
  transferring: {
    label: "Sending",
    tone: "progress",
    icon: "loader",
    hint: "The hub is sending the package to the host.",
  },
  dispatched: {
    label: "Running",
    tone: "progress",
    icon: "loader",
    hint: "The host's agent is carrying it out. The hub is waiting for its answer.",
  },
  succeeded: {
    label: "Stored (skills off)",
    tone: "success",
    icon: "check-circle",
    hint: "The host's agent confirmed the package files are stored and verified. Its skills are not active until a person enables them on the host's Packages tab.",
  },
  failed: {
    label: "Failed",
    tone: "danger",
    icon: "x-circle",
    hint: "The host's agent refused it or could not finish. Its work is all or nothing, so nothing was left half done.",
  },
  not_run: {
    label: "Never ran",
    tone: "neutral",
    icon: "ban",
    hint: "Nothing was started on the host: the approval window closed, the host was not ready or the transfer failed. It is safe to try again.",
  },
  outcome_unknown: {
    label: "Result unknown",
    tone: "warning",
    icon: "alert",
    hint: "The hub handed it to the host but never got a trustworthy answer. Check the host to see what is installed before trying again.",
  },
  cancelled: {
    label: "Cancelled",
    tone: "neutral",
    icon: "x",
    hint: "You cancelled it before it started.",
  },
};

/** Where a whole install job is. */
export const INSTALL_JOB_STATUSES: Table<InstallJobStatus> = {
  running: {
    label: "Running",
    tone: "progress",
    icon: "loader",
    hint: "At least one host is still working on it.",
  },
  succeeded: {
    label: "Stored, skills not active",
    tone: "success",
    icon: "check-circle",
    hint: "Every host stored the package. Storing does not enable anything: the package's skills stay off until a person enables them.",
  },
  failed: {
    label: "Failed",
    tone: "danger",
    icon: "x-circle",
    hint: "No host finished it. Each host shows why.",
  },
  partial: {
    label: "Partly done",
    tone: "warning",
    icon: "alert",
    hint: "Some hosts finished and some did not. Each host shows its own result.",
  },
};

/** What a host reports about a package it holds. */
export const HOST_PACKAGE_STATUSES: Table<HostPackageStatus> = {
  active: {
    label: "Stored (skills off)",
    tone: "success",
    icon: "check-circle",
    hint: "The host holds verified files for this package. Its skills are not active until a person enables them on the host's Packages tab.",
  },
  disabled_untrusted_key: {
    label: "Signed by an untrusted key",
    tone: "warning",
    icon: "alert",
    hint: "The host no longer trusts the key that signed this package, so it refuses to run it. Re-sign it with a trusted key or remove it.",
  },
  missing_files: {
    label: "Files missing",
    tone: "danger",
    icon: "x-circle",
    hint: "The host's record names this package but its files are gone. Remove it and install it again.",
  },
};

/** Whether a host trusts the key that signed a package. */
export const KEY_STATES: Table<KeyState> = {
  trusted: {
    label: "Trusted key",
    tone: "success",
    icon: "check-circle",
    hint: "The host trusts the key that signed this package.",
  },
  unknown: {
    label: "Unknown key",
    tone: "warning",
    icon: "help",
    hint: "The host does not have this key in its trusted list.",
  },
  revoked: {
    label: "Revoked key",
    tone: "danger",
    icon: "ban",
    hint: "The host has revoked the key that signed this package.",
  },
};

/** Whether a package's archive is in the hub's store. */
export const FILE_STATUSES: Table<FileStatus> = {
  ready: {
    label: "In the store",
    tone: "success",
    icon: "check-circle",
    hint: "The archive is in the hub's package store.",
  },
  missing: {
    label: "File missing",
    tone: "danger",
    icon: "x-circle",
    hint: "The archive is gone from the hub's package store, so it cannot be installed. Import the same package again to repair it.",
  },
};

/** Whether the privilege boundary is in effect on a host, as its agent reports. */
export const PRIVILEGE_BOUNDARIES: Table<PrivilegeBoundary> = {
  in_effect: {
    label: "Isolated",
    tone: "success",
    icon: "check-circle",
    hint: "The agent reports that the privilege boundary is in effect on this host.",
  },
  not_in_effect: {
    label: "Not isolated",
    tone: "warning",
    icon: "alert",
    hint: "The agent reports that the privilege boundary is not in effect on this host, for example because it runs as root, so package code is not kept apart from it.",
  },
};

/** How a host's unattended limits reached the hub. */
export const CEILING_REPORTING: Table<CeilingReporting> = {
  reported: {
    label: "Reported by the host",
    tone: "success",
    icon: "check-circle",
    hint: "The host sent its unattended limits and they are shown below.",
  },
  not_reported: {
    label: "Not reported",
    tone: "neutral",
    icon: "info",
    hint: "The host has not sent any unattended limits to this hub.",
  },
  unreadable: {
    label: "Unreadable",
    tone: "warning",
    icon: "alert",
    hint: "The host sent something the hub could not read, so it is treated as not allowed.",
  },
};

/** How one host's part of an install is shown. */
export function installTargetStatusMeta(status: string): StatusMeta {
  return lookup(INSTALL_TARGET_STATUSES, status);
}
/** How a whole install job is shown. */
export function installJobStatusMeta(status: string): StatusMeta {
  return lookup(INSTALL_JOB_STATUSES, status);
}
/** How a package on a host is shown. */
export function hostPackageStatusMeta(status: string): StatusMeta {
  return lookup(HOST_PACKAGE_STATUSES, status);
}
/** How a host's trust in a package's key is shown. */
export function keyStateMeta(state: string): StatusMeta {
  return lookup(KEY_STATES, state);
}
/** How a stored archive's presence is shown. */
export function fileStatusMeta(status: string): StatusMeta {
  return lookup(FILE_STATUSES, status);
}
/** How the privilege boundary is shown. */
export function privilegeBoundaryMeta(value: string): StatusMeta {
  return lookup(PRIVILEGE_BOUNDARIES, value);
}
/** How the reporting state of a host's limits is shown. */
export function ceilingReportingMeta(value: string): StatusMeta {
  return lookup(CEILING_REPORTING, value);
}

/** Every package vocabulary, for the walk in `tone.ts`. */
export const PACKAGE_VOCABULARIES: {
  meta: (value: string) => StatusMeta;
  name: string;
  values: string[];
}[] = [
  {
    name: "Install target",
    meta: installTargetStatusMeta,
    values: Object.keys(INSTALL_TARGET_STATUSES),
  },
  { name: "Install job", meta: installJobStatusMeta, values: Object.keys(INSTALL_JOB_STATUSES) },
  { name: "Host package", meta: hostPackageStatusMeta, values: Object.keys(HOST_PACKAGE_STATUSES) },
  { name: "Package key state", meta: keyStateMeta, values: Object.keys(KEY_STATES) },
  { name: "Package file", meta: fileStatusMeta, values: Object.keys(FILE_STATUSES) },
  {
    name: "Privilege boundary",
    meta: privilegeBoundaryMeta,
    values: Object.keys(PRIVILEGE_BOUNDARIES),
  },
  { name: "Ceiling reporting", meta: ceilingReportingMeta, values: Object.keys(CEILING_REPORTING) },
];
