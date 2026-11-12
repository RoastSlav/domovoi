import { lazy, Suspense, useState } from "react";
import type { PaletteGroup } from "./palette";

const loadView = () => import("./CommandPaletteView");
const CommandPaletteView = lazy(loadView);

/** Starts downloading the palette's code, for a search button to call on hover or focus. */
export function preloadCommandPalette(): void {
  void loadView();
}

/** What the command palette shows and does. */
export interface CommandPaletteProps {
  /** The places and actions, grouped: Pages, Hosts, Actions. */
  groups: readonly PaletteGroup[];
  /** Called on Escape, on a click outside and after an item was chosen. */
  onClose: () => void;
  open: boolean;
  /** The search box's placeholder. */
  placeholder?: string;
}

/**
 * The command palette: a dialog with a search box and grouped results, opened by Ctrl or Cmd
 * with K and by a search button. Arrow keys, Home and End move through the results, Enter
 * runs the active one (after closing the palette), Escape closes. It filters by substring
 * over each item's label and keywords and says "Nothing matches" when nothing does.
 *
 * It is its own download: nothing is fetched until it is first opened (or `preloadCommandPalette`
 * is called), and it keeps no search text between openings. The page owns `open` and the
 * `groups` (the palette knows nothing about routes).
 *
 * Use it for jumping around the app. Do not use it for a form or a question that must be
 * answered: that is a `Dialog`.
 */
export function CommandPalette(props: CommandPaletteProps) {
  const [wanted, setWanted] = useState(props.open);
  if (props.open && !wanted) {
    setWanted(true);
  }
  if (!wanted) {
    return null;
  }
  return (
    <Suspense fallback={null}>
      <CommandPaletteView {...props} />
    </Suspense>
  );
}
