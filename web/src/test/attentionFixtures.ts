import type { Host, OperationSummary } from "../api/types";
import { nas, T0 } from "./fixtures";

/** An operation summary in `status`, overridable; waits for what its status says. */
export function summary(
  id: string,
  status: string,
  overrides: Partial<OperationSummary> = {},
): OperationSummary {
  return {
    id,
    kind: "investigate",
    goal: `Goal ${id}`,
    status,
    pause_reason: null,
    waiting_for: [],
    cycle: 0,
    incidents: [],
    hosts: [nas.id],
    created_at: T0,
    started_at: T0,
    ended_at: null,
    updated_at: T0,
    ...overrides,
  } as OperationSummary;
}

/** An enrolled agent that is not connected: offline. */
export function offlineHost(name: string, overrides: Partial<Host> = {}): Host {
  return { ...nas, id: `host-${name}`, name, connected: false, open_incidents: 0, ...overrides };
}

/** An agent speaking another protocol version than the hub. */
export function incompatibleHost(name: string, overrides: Partial<Host> = {}): Host {
  return offlineHost(name, { protocol_state: "incompatible", ...overrides });
}

/** An agent that was never enrolled and never connected. */
export function neverConnectedHost(name: string): Host {
  return offlineHost(name, { agent_enrolled: false });
}

/** The hub's own machine without an agent. */
export function hubHost(name: string): Host {
  return offlineHost(name, { kind: "local", agent_enrolled: false });
}

/** The empty first page of a list the Control Room and the attention page ask for. */
export const EMPTY_PAGE = { items: [], next_cursor: null, as_of_seq: 0 };
