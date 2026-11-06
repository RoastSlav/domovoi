import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from "react";
import { useSearchParams } from "react-router";

/** The `localStorage` key that remembers the selected host between visits. */
export const SELECTED_HOST_KEY = "selected-host";

/** The remembered host, or nothing when storage is empty or unavailable. */
function readStored(): string | undefined {
  try {
    return localStorage.getItem(SELECTED_HOST_KEY) || undefined;
  } catch {
    return undefined;
  }
}

/** Remembers `name`, or forgets the selection when it is absent. Storage may be blocked. */
function writeStored(name: string | undefined) {
  try {
    if (name) {
      localStorage.setItem(SELECTED_HOST_KEY, name);
    } else {
      localStorage.removeItem(SELECTED_HOST_KEY);
    }
  } catch {
    // Private windows and blocked site data: the selection then lasts for the visit only.
  }
}

/** The selected host's name (`undefined` is "all hosts") and how to change it. */
type HostSelection = readonly [string | undefined, (name: string | undefined) => void];

const HostContext = createContext<HostSelection | null>(null);

/**
 * Holds the app-wide host selection. The URL's `?host=` wins, so shared links keep working;
 * without it the last choice is read back from `localStorage`.
 */
export function HostProvider({ children }: { children: ReactNode }) {
  const [params, setParams] = useSearchParams();
  const [stored, setStored] = useState(readStored);
  const fromUrl = params.get("host") || undefined;
  const selected = fromUrl ?? stored;

  const select = useCallback(
    (name: string | undefined) => {
      setStored(name);
      writeStored(name);
      setParams((previous) => {
        const next = new URLSearchParams(previous);
        if (name) {
          next.set("host", name);
        } else {
          next.delete("host");
        }
        return next;
      });
    },
    [setParams],
  );

  const value = useMemo<HostSelection>(() => [selected, select], [selected, select]);
  return <HostContext.Provider value={value}>{children}</HostContext.Provider>;
}

/** The selected host and a setter; pages filter by it instead of keeping their own host choice. */
export function useSelectedHost(): HostSelection {
  const value = useContext(HostContext);
  if (!value) {
    throw new Error("useSelectedHost needs a HostProvider above it");
  }
  return value;
}
