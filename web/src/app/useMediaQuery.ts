import { useCallback, useSyncExternalStore } from "react";

/** The widest phone layout ends below this; the icon rail and the sidebar start at it. */
export const WIDE_QUERY = "(min-width: 768px)";

/** Between a phone and a desktop: where the sidebar is an icon rail unless expanded. */
export const TABLET_QUERY = "(min-width: 768px) and (max-width: 1023px)";

/** Whether `query` matches now, kept current. False where media queries do not exist. */
export function useMediaQuery(query: string): boolean {
  const subscribe = useCallback(
    (changed: () => void) => {
      if (typeof window.matchMedia !== "function") {
        return () => undefined;
      }
      const list = window.matchMedia(query);
      list.addEventListener("change", changed);
      return () => list.removeEventListener("change", changed);
    },
    [query],
  );
  return useSyncExternalStore(subscribe, () =>
    typeof window.matchMedia === "function" ? window.matchMedia(query).matches : false,
  );
}
