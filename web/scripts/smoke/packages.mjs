/**
 * The mock hub's package, install-job, host-limits and capability answers. Every route the
 * Capability Center calls is listed here, the four helper routes included.
 */

const T0 = "2026-10-31T12:00:00Z";
const DIGEST = "ab12".repeat(16);
const JOB_HASH = "cd".repeat(32);
const HOST_PLAN_HASH = "ef".repeat(32);
const JOB_ID = "0192b000-0000-7000-8000-00000000a001";
const KEY_ID = "0123456789abcdef";

const PACKAGE = {
    name: "nginx-tools",
    version: "1.2.0",
    archive_sha256: DIGEST,
    archive_size: 20480,
    key_id: KEY_ID,
    signer_label: "release key",
    imported_at: T0,
    already_imported: false,
};

function installed(observed) {
    return {
        name: "nginx-tools",
        version: "1.2.0",
        archive_sha256: DIGEST,
        key_id: KEY_ID,
        key_state: "trusted",
        status: "active",
        skills: ["nginx-reload"],
        privilege_boundary: "in_effect",
        installed_at: T0,
        observed_at: observed,
        observed_by: "agent",
    };
}

/** What the mock hub keeps between requests of one run. */
export function makePackageState(hostId) {
    return {
        hostId,
        imported: true,
        jobReads: 0,
        jobCreated: false,
        bodies: [],
        helperPinned: true,
    };
}

function job(state, finished) {
    return {
        id: JOB_ID,
        kind: "install",
        name: PACKAGE.name,
        version: PACKAGE.version,
        archive_sha256: DIGEST,
        key_id: KEY_ID,
        requested_by: "admin",
        created_at: T0,
        status: finished ? "succeeded" : "running",
        job_plan_hash: JOB_HASH,
        targets: [
            {
                id: "t1",
                host_id: state.hostId,
                host_name: "nas",
                attempt: 1,
                status: finished ? "succeeded" : "dispatched",
                failure: null,
                outputs_confirmed: true,
                plan_hash: HOST_PLAN_HASH,
                queued_at: T0,
                started_at: T0,
                finished_at: finished ? T0 : null,
                expires_at: "2099-01-01T00:00:00Z",
                privilege_boundary: "in_effect",
            },
        ],
    };
}

const FINGERPRINT = "0123456789abcdef".repeat(4);

/** The helper view of nas: a pinned key and a verified report, or the key received but unpinned. */
function helperView(pinned) {
    return {
        host: "nas",
        helper: { state: "ok", ops: ["safety.attest"] },
        key_state: pinned ? "pinned" : "not_pinned",
        received_key: {
            fingerprint: FINGERPRINT,
            fingerprint_groups: FINGERPRINT.replace(/(.{4})/g, "$1 ").trim(),
            key_id: FINGERPRINT.slice(0, 16),
        },
        pin: pinned ? { fingerprint: FINGERPRINT, pinned_at: T0, pinned_by: "admin" } : null,
        attestation: pinned
            ? { state: "verified", checked_at: T0 }
            : { state: "unverified", reason: "key_not_pinned", checked_at: T0 },
        verified: pinned
            ? {
                  age_seconds: 30,
                  boot_id: "boot-1",
                  cidrs: ["192.0.2.0/24"],
                  computed_at: T0,
                  config_hash: "c0ffee",
                  fresh: true,
                  generation: 7,
                  hub_addrs: ["192.0.2.10"],
                  link_peers: ["192.0.2.10:7421"],
                  max_active: 4,
                  max_ttl_s: 3600,
                  nonce_issued_at: T0,
                  protected_containers: ["plex"],
                  protected_units: ["sshd.service"],
                  route_state_hash: "abc123",
              }
            : null,
    };
}

/** Answers a package, job, ceiling, capability or helper request; undefined for any other path. */
export function packageRoute(state, method, pathname, body) {
    const ok = (payload, status = 200) => ({ status, payload });
    const key = `${method} ${pathname}`;
    const jobPath = `/api/v1/packages/jobs/${JOB_ID}`;
    const containment = /^\/api\/v1\/hosts\/([^/]+)\/containment(\/refresh)?$/.exec(pathname);
    if (containment && (method === "GET" || (method === "POST" && containment[2]))) {
        // Nothing asked yet and no helper write group offered: the status says so.
        return ok({
            available: false,
            gate: { state: "unknown" },
            drills: { state: "unknown" },
            blocks: [],
        });
    }
    const other = /^\/api\/v1\/hosts\/([^/]+)\/helper$/.exec(pathname);
    if (method === "GET" && other && other[1] !== "nas") {
        // A host with no helper: its card renders the absent state.
        return ok({
            host: other[1],
            helper: { state: "absent", ops: [] },
            key_state: "no_helper",
            received_key: null,
            pin: null,
            attestation: { state: "unverified", reason: "attestation_missing", checked_at: null },
            verified: null,
        });
    }
    switch (key) {
        case "GET /api/v1/hosts/nas/helper":
            return ok(helperView(state.helperPinned !== false));
        case "POST /api/v1/hosts/nas/helper/recheck":
            return { status: 202, payload: undefined };
        case "PUT /api/v1/hosts/nas/helper/key":
            if (body?.password !== "correct-password") {
                return ok({ error: "wrong_password", message: "wrong password" }, 401);
            }
            state.helperPinned = true;
            return ok(helperView(true));
        case "DELETE /api/v1/hosts/nas/helper/key":
            state.helperPinned = false;
            return ok(helperView(false));
        case "GET /api/v1/hosts/nas/capabilities":
            return ok({ host: "nas", entries: [], unknown_contracts: [] });
        case "GET /api/v1/hosts/nas/ceiling":
            return ok({
                host: "nas",
                connected: true,
                reporting: "reported",
                unattended_enabled: true,
                hash: "9f86d081884c7d659a2f",
                entries: [
                    {
                        contract: "container.restart",
                        targets: { kind: "names", names: ["jellyfin", "plex"] },
                        max_per_hour: 3,
                        max_active: null,
                        max_ttl_secs: null,
                        args: {},
                    },
                    {
                        contract: "firewall.block_source",
                        targets: { kind: "any" },
                        max_per_hour: 20,
                        max_active: 50,
                        max_ttl_secs: 3600,
                        args: { min_prefix_len_v4: 24 },
                    },
                ],
                unreadable_reason: null,
            });
        case "GET /api/v1/packages/status":
            return ok({
                enabled: true,
                max_artifact_bytes: 52428800,
                max_store_bytes: 524288000,
                used_store_bytes: 20480,
                artifact_count: 1,
                owner_keys: [{ id: KEY_ID, label: "release key" }],
                revoked: [],
                approval_window_secs: 600,
            });
        case "GET /api/v1/packages":
            return ok(state.imported ? [PACKAGE] : []);
        case "POST /api/v1/packages/import":
            state.bodies.push(typeof body === "string" ? body : JSON.stringify(body));
            state.imported = true;
            return ok(PACKAGE, 201);
        case "GET /api/v1/packages/nginx-tools/1.2.0":
            return ok({
                ...PACKAGE,
                signature_toml: `name = "nginx-tools"\nversion = "1.2.0"\nkey_id = "${KEY_ID}"\n`,
                hosts: [
                    { host_id: state.hostId, host_name: "nas", status: "active", observed_at: T0 },
                ],
                file_status: "ready",
            });
        case "POST /api/v1/packages/jobs/plan":
            return ok({
                kind: body?.kind ?? "install",
                name: PACKAGE.name,
                version: PACKAGE.version,
                archive_sha256: DIGEST,
                key_id: KEY_ID,
                signer_label: "release key",
                job_plan_hash: JOB_HASH,
                targets: [
                    {
                        host_id: state.hostId,
                        host_name: "nas",
                        plan_hash: HOST_PLAN_HASH,
                        availability: "available",
                        availability_text: "The host offers it.",
                        installed: null,
                        warnings: [],
                    },
                ],
            });
        case "POST /api/v1/packages/jobs":
            state.jobCreated = true;
            state.jobReads = 0;
            return ok(job(state, false), 202);
        case "GET /api/v1/packages/jobs":
            return ok({
                items: state.jobCreated ? [job(state, state.jobReads > 1)] : [],
                next_cursor: null,
            });
        case `GET ${jobPath}`:
            state.jobReads += 1;
            return ok(job(state, state.jobReads > 1));
        case "GET /api/v1/hosts/nas/packages":
            return ok({
                host_id: state.hostId,
                observed_at: T0,
                entries: [installed(T0)],
                install_allowed: true,
            });
        case "POST /api/v1/hosts/nas/packages/refresh":
            return ok({
                host_id: state.hostId,
                observed_at: T0,
                entries: [installed(T0)],
                install_allowed: true,
            });
        default:
            return undefined;
    }
}
