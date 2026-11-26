import type { Changes } from "../../../api/hooks/settings";
import { getIn } from "./paths";

/** Dotted paths of every value in `changes`. */
export function changedPaths(changes: Changes, prefix = ""): string[] {
  return Object.entries(changes).flatMap(([key, value]) => {
    const path = prefix ? `${prefix}.${key}` : key;
    const isTable =
      typeof value === "object" &&
      value !== null &&
      !Array.isArray(value) &&
      !("set" in value || "clear" in value || (value as { keep?: unknown }).keep === true);
    return isTable ? changedPaths(value as Changes, path) : [path];
  });
}

/** Whether a changed path is under [retention]. */
export function isRetentionPath(path: string): boolean {
  return path === "retention" || path.startsWith("retention.");
}

/** Whether a changed path is one of the backup counts. */
function isBackupCount(path: string): boolean {
  return path === "backup.keep" || path === "backup.keep_pre_migrate";
}

/** Whether one changed path is a secret, a trust setting, a period or a backup count. */
function pathNeedsPassword(changes: Changes, path: string): boolean {
  return isSecretChange(changes, path) || isTrustPath(path) || isDeletionPath(path);
}

function isSecretChange(changes: Changes, path: string): boolean {
  const value = getIn(changes, path);
  return typeof value === "object" && value !== null && ("set" in value || "clear" in value);
}

/** Settings whose whole table steps up: trust, programs the hub runs, metrics and packages. */
const TRUST_TABLES = ["webhooks", "metrics", "packages"];

/** The policy limits that widen what runs unattended or delay the circuit breaker. */
const POLICY_LIMITS = [
  "policy.permit_ttl",
  "policy.circuit_failures",
  "policy.circuit_window",
  "policy.max_permits_per_hour",
];

/**
 * Mirrors `STEP_UP_SETTINGS` in the hub (`settings/step_up.rs`), which is the authority; a
 * save the hub still refuses with 403 opens the password prompt too (see `EditorForm`).
 */
function isTrustPath(path: string): boolean {
  return (
    path === "discord.allowed_user_ids" ||
    path === "web.trusted_proxies" ||
    TRUST_TABLES.some((table) => path === table || path.startsWith(`${table}.`)) ||
    /^llm\.[^.]+\.cli(\.|$)/.test(path) ||
    POLICY_LIMITS.includes(path)
  );
}

/** Retention periods and backup counts: changing them can delete data sooner. */
function isDeletionPath(path: string): boolean {
  return isRetentionPath(path) || isBackupCount(path);
}

/** Whether a change needs the password again: trust, secrets, retention and backup counts. */
export function isSensitive(changes: Changes): boolean {
  return changedPaths(changes).some((path) => pathNeedsPassword(changes, path));
}

/** Why the password is asked: deleting sooner, or widening trust (also for a mix of both). */
export function sensitiveReason(changes: Changes): string {
  const trust = changedPaths(changes).some(
    (path) => !isDeletionPath(path) && (isSecretChange(changes, path) || isTrustPath(path)),
  );
  return trust
    ? "This change affects who can act or what is trusted. Enter your password to save it."
    : "A shorter period deletes data sooner. Enter your password to save it.";
}
