import { useCallback, useEffect, useState } from "react";

/** Whether the keyboard is a Mac's, where the shortcut is Command and not Control. */
function isMac(): boolean {
  return typeof navigator !== "undefined" && /Mac|iPhone|iPad/i.test(navigator.userAgent);
}

/** The shortcut that opens the command palette, as a key hint: "Ctrl K", or "⌘K" on a Mac. */
export function searchShortcut(): string {
  return isMac() ? "⌘K" : "Ctrl K";
}

/** The shortcut for assistive technology (`aria-keyshortcuts`). */
export function searchShortcutAria(): string {
  return isMac() ? "Meta+K" : "Control+K";
}

/**
 * Whether the command palette is open, with the page-wide Ctrl+K (Cmd+K on a Mac) that
 * toggles it. The shortcut works from anywhere, including inside a text field.
 */
export function useCommandPalette(): {
  open: boolean;
  setOpen: (open: boolean) => void;
} {
  const [open, setOpen] = useState(false);
  const toggle = useCallback(() => setOpen((now) => !now), []);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent): void => {
      const mod = isMac() ? event.metaKey : event.ctrlKey;
      if (mod && !event.altKey && !event.shiftKey && event.key.toLowerCase() === "k") {
        event.preventDefault();
        toggle();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [toggle]);

  return { open, setOpen };
}
