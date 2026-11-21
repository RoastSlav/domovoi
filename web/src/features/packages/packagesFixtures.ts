import { T0 } from "../../test/fixtures";

/** A status answer with installation on and one owner key. */
export function status(overrides: Record<string, unknown> = {}) {
  return {
    enabled: true,
    max_artifact_bytes: 104857600,
    max_store_bytes: 1073741824,
    used_store_bytes: 5242880,
    artifact_count: 1,
    owner_keys: [{ id: "0123456789abcdef", label: "Release key" }],
    revoked: ["fedcba9876543210"],
    approval_window_secs: 600,
    ...overrides,
  };
}

/** The stored package the tests import and list. */
export function pkg(overrides: Record<string, unknown> = {}) {
  return {
    name: "nginx-tools",
    version: "1.2.0",
    archive_sha256: "a".repeat(64),
    archive_size: 2048,
    key_id: "0123456789abcdef",
    signer_label: "Release key",
    imported_at: T0,
    already_imported: false,
    ...overrides,
  };
}

/** A package a host holds. */
export function hostPackage(overrides: Record<string, unknown> = {}) {
  return {
    name: "nginx-tools",
    version: "1.2.0",
    key_id: "0123456789abcdef",
    key_state: "trusted",
    archive_sha256: "a".repeat(64),
    installed_at: T0,
    status: "active",
    skills: ["nginx-reload"],
    privilege_boundary: "in_effect",
    observed_at: T0,
    observed_by: "agent",
    ...overrides,
  };
}

/** An install job with one target on nas. */
export function job(overrides: Record<string, unknown> = {}) {
  return {
    id: "job-1",
    kind: "install",
    name: "nginx-tools",
    version: "1.2.0",
    archive_sha256: "a".repeat(64),
    key_id: "0123456789abcdef",
    requested_by: "admin",
    created_at: T0,
    status: "running",
    job_plan_hash: "b".repeat(64),
    targets: [
      {
        id: "t1",
        host_id: "host-nas",
        host_name: "nas",
        attempt: 1,
        status: "queued",
        plan_hash: "c".repeat(64),
        outputs_confirmed: false,
        queued_at: T0,
        expires_at: T0,
      },
    ],
    ...overrides,
  };
}
