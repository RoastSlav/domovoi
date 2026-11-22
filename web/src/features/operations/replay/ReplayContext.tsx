import { createContext, useContext, useState, type ReactNode } from "react";

/** Whether the graph is showing a past moment, and which, for the panels that stay live. */
export interface ReplayStatus {
  replaying: boolean;
  /** The moment shown, as a clock time; empty when not replaying. */
  label: string;
}

const NOT_REPLAYING: ReplayStatus = { replaying: false, label: "" };
const noop = (): void => undefined;

const StatusContext = createContext<ReplayStatus>(NOT_REPLAYING);
const SetContext = createContext<(status: ReplayStatus) => void>(noop);

/** Holds the replay status for one operation page; the Graph view sets it, the inspector reads it. */
export function ReplayStatusProvider({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState(NOT_REPLAYING);
  return (
    <SetContext.Provider value={setStatus}>
      <StatusContext.Provider value={status}>{children}</StatusContext.Provider>
    </SetContext.Provider>
  );
}

/** The replay status; not replaying outside a provider. */
export function useReplayStatus(): ReplayStatus {
  return useContext(StatusContext);
}

/** Sets the replay status; a no-op outside a provider. Pass `NOT_REPLAYING_STATUS` to clear it. */
export function useSetReplayStatus(): (status: ReplayStatus) => void {
  return useContext(SetContext);
}

/** The status that means no replay is showing. */
export const NOT_REPLAYING_STATUS: ReplayStatus = NOT_REPLAYING;
