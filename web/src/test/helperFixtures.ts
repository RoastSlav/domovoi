import type { HelperVerified, HelperView } from "../api/types";

/** The fingerprint of the received key, 64 hex characters. */
export const FINGERPRINT = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/** The same, in groups of four, as the host prints it. */
export const GROUPS = FINGERPRINT.replace(/(.{4})/g, "$1 ").trim();

/** A verified, fresh report for a host. */
export function verifiedReport(overrides: Partial<HelperVerified> = {}): HelperVerified {
  return {
    age_seconds: 42,
    boot_id: "boot-1",
    cidrs: ["192.0.2.0/24"],
    computed_at: "2026-10-31T11:59:00Z",
    config_hash: "c0ffee",
    fresh: true,
    generation: 7,
    hub_addrs: ["192.0.2.10"],
    link_peers: ["192.0.2.10:7421"],
    max_active: 4,
    max_ttl_s: 3600,
    nonce_issued_at: "2026-10-31T11:58:59Z",
    protected_containers: ["plex"],
    restart_units: ["nginx.service"],
    protected_units: ["sshd.service"],
    route_state_hash: "abc123",
    ...overrides,
  };
}

/** A helper view with a pinned key and a verified report; override what a test is about. */
export function helperView(overrides: Partial<HelperView> = {}): HelperView {
  return {
    host: "nas",
    helper: { state: "ok", ops: ["safety.attest"] },
    key_state: "pinned",
    received_key: {
      fingerprint: FINGERPRINT,
      fingerprint_groups: GROUPS,
      key_id: "0123456789abcdef",
    },
    pin: { fingerprint: FINGERPRINT, pinned_at: "2026-10-30T10:00:00Z", pinned_by: "admin" },
    attestation: {
      state: "verified",
      checked_at: "2026-10-31T11:59:00Z",
      reason: null,
      helper_reason: null,
      unsigned_hint: null,
    },
    verified: verifiedReport(),
    ...overrides,
  };
}

/** A view whose key is received but not pinned. */
export function unpinnedView(overrides: Partial<HelperView> = {}): HelperView {
  return helperView({
    key_state: "not_pinned",
    pin: null,
    attestation: { state: "unverified", reason: "key_not_pinned" },
    verified: null,
    ...overrides,
  });
}
