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
      !("set" in value || "clear" in value || "keep" in value);
    return isTable ? changedPaths(value as Changes, path) : [path];
  });
}

/** Whether a change widens trust, so the hub asks for the password again. */
export function isSensitive(changes: Changes): boolean {
  return changedPaths(changes).some((path) => {
    const secret = getIn(changes, path);
    const isSecret =
      typeof secret === "object" && secret !== null && ("set" in secret || "clear" in secret);
    return (
      isSecret ||
      path === "discord.allowed_user_ids" ||
      path === "web.trusted_proxies" ||
      path === "webhooks" ||
      path.startsWith("webhooks.")
    );
  });
}
