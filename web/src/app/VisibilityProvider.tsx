import { useEffect, useMemo, useState, type ReactNode } from "react";
import { usePreferences } from "../api/hooks/preferences";
import {
  DEFAULT_PRESENTATION_CHOICE,
  VisibilityContext,
  type PresentationChoice,
} from "../components";
import { parsePresentation, readCache, writeCache } from "./visibilityCache";

/**
 * Supplies the owner's detail level to everything below. The hub's answer wins; the browser's
 * last known choice fills the gap before it arrives; with neither, or on any failure, the
 * level is Detailed, so a failure shows more, never less. It shows no spinner or error.
 */
export function VisibilityProvider({ children }: { children: ReactNode }) {
  const query = usePreferences();
  const [cached] = useState(readCache);
  const server = useMemo(
    () => (query.data ? parsePresentation(query.data.presentation) : undefined),
    [query.data],
  );
  useEffect(() => {
    if (server) {
      writeCache(server);
    }
  }, [server]);
  const value: PresentationChoice = server ?? cached ?? DEFAULT_PRESENTATION_CHOICE;
  return <VisibilityContext.Provider value={value}>{children}</VisibilityContext.Provider>;
}
