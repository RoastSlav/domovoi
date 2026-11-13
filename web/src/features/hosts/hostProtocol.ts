import type { Host } from "../../api/types";

/** One line naming the protocols when the hub and agent disagree; null when they do not. */
export function protocolDetail(host: Host): string | null {
  if (host.connected) {
    return null;
  }
  const reported = host.reported_protocol ?? "unknown";
  if (host.protocol_state === "incompatible") {
    return `agent speaks ${reported}, hub requires ${host.hub_protocol}; upgrade the agent`;
  }
  if (host.protocol_state === "expected_incompatible") {
    return `last reported ${reported}, hub requires ${host.hub_protocol}`;
  }
  return null;
}
