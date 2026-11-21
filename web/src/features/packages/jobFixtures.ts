import type {
  Host,
  InstallJob,
  InstallPlan,
  InstallPlanTarget,
  InstallTarget,
  PackageDetail,
  PackagesStatus,
} from "../../api/types";
import { T0, nas } from "../../test/fixtures";

/** The job-level plan hash of the fixture plan. */
export const JOB_HASH = "a1b2c3d4".repeat(8);
/** The host-level plan hash of the fixture plan's single host. */
export const HOST_HASH = "0f0e0d0c".repeat(8);

/** A host that is not connected but has been seen. */
export const attic: Host = {
  ...nas,
  id: "host-attic",
  name: "attic",
  connected: false,
  last_seen: T0,
};
/** A host with no agent. */
export const bare: Host = {
  ...nas,
  id: "host-bare",
  name: "bare",
  kind: "local",
  connected: false,
  agent_enrolled: false,
};

/** Package installation on, ten-minute approval window. */
export const STATUS: PackagesStatus = {
  enabled: true,
  approval_window_secs: 600,
  artifact_count: 1,
  max_artifact_bytes: 1_000_000,
  max_store_bytes: 10_000_000,
  used_store_bytes: 1_000,
  owner_keys: [{ id: "0123456789abcdef", label: "Owner" }],
  revoked: [],
};

/** The stored package the wizard installs. */
export const DETAIL: PackageDetail = {
  name: "nginx-tools",
  version: "1.2.0",
  archive_sha256: "ab".repeat(32),
  archive_size: 2048,
  imported_at: T0,
  key_id: "0123456789abcdef",
  signer_label: "Owner",
  already_imported: false,
  signature_toml: "signature = 'x'",
  hosts: [],
  file_status: "ready",
};

/** A plan for `nas` that can run. */
export function plan(overrides: Partial<InstallPlan> = {}): InstallPlan {
  return {
    kind: "install",
    name: "nginx-tools",
    version: "1.2.0",
    archive_sha256: "ab".repeat(32),
    key_id: "0123456789abcdef",
    signer_label: "Owner",
    job_plan_hash: JOB_HASH,
    targets: [
      {
        host_id: nas.id,
        host_name: "nas",
        plan_hash: HOST_HASH,
        availability: "available",
        availability_text: "The host offers package installation.",
        installed: null,
        warnings: [],
      },
    ],
    ...overrides,
  };
}

/** One host's attempt, running unless overridden. */
export function target(overrides: Partial<InstallTarget> = {}): InstallTarget {
  return {
    id: "t1",
    host_id: nas.id,
    host_name: "nas",
    attempt: 1,
    status: "dispatched",
    outputs_confirmed: true,
    plan_hash: HOST_HASH,
    queued_at: T0,
    started_at: T0,
    finished_at: null,
    expires_at: "2099-01-01T00:00:00Z",
    failure: null,
    privilege_boundary: null,
    ...overrides,
  };
}

/** An install job over `targets`. */
export function job(
  targets: InstallTarget[] = [target()],
  overrides: Partial<InstallJob> = {},
): InstallJob {
  return {
    id: "job-1",
    kind: "install",
    name: "nginx-tools",
    version: "1.2.0",
    archive_sha256: "ab".repeat(32),
    key_id: "0123456789abcdef",
    requested_by: "admin",
    created_at: T0,
    status: "running",
    job_plan_hash: JOB_HASH,
    targets,
    ...overrides,
  };
}

/** The single host target of the fixture plan, for tests that vary one host. */
export const BASE: InstallPlanTarget = plan().targets[0] as InstallPlanTarget;
