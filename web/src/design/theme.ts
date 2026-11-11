import { useCallback, useSyncExternalStore } from "react";

/** The theme a person can pick; "system" follows the device. */
export type Theme = "system" | "light" | "dark";

/** Where the choice is kept. The inline script in index.html reads the same key before first paint. */
export const THEME_KEY = "domovoi.theme";

const listeners = new Set<() => void>();

function isTheme(value: unknown): value is Theme {
  return value === "system" || value === "light" || value === "dark";
}

/** The saved choice, or "system" when nothing is saved or storage is unavailable. */
export function readTheme(): Theme {
  try {
    const saved = localStorage.getItem(THEME_KEY);
    return isTheme(saved) ? saved : "system";
  } catch {
    return "system";
  }
}

/** Points the browser's toolbar colour at the page background of the theme now showing. */
function syncThemeColor(): void {
  const bg = getComputedStyle(document.documentElement).getPropertyValue("--color-bg").trim();
  if (!bg) {
    return;
  }
  for (const meta of document.querySelectorAll('meta[name="theme-color"]')) {
    meta.setAttribute("content", bg);
  }
}

/** Shows `theme` now: sets or removes `data-theme` on the root element. */
export function applyTheme(theme: Theme): void {
  const root = document.documentElement;
  if (theme === "system") {
    delete root.dataset.theme;
  } else {
    root.dataset.theme = theme;
  }
  syncThemeColor();
}

/** Shows `theme` and remembers it; a browser that refuses storage still switches for this visit. */
export function setTheme(theme: Theme): void {
  try {
    localStorage.setItem(THEME_KEY, theme);
  } catch {
    // Private mode or blocked storage: the choice lasts until the page reloads.
  }
  applyTheme(theme);
  for (const listener of listeners) {
    listener();
  }
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function current(): Theme {
  const shown = document.documentElement.dataset.theme;
  return shown === "light" || shown === "dark" ? shown : "system";
}

/** The theme now showing and a way to change it. */
export function useTheme(): { setTheme: (theme: Theme) => void; theme: Theme } {
  const theme = useSyncExternalStore(subscribe, current);
  return { theme, setTheme: useCallback((next: Theme) => setTheme(next), []) };
}
