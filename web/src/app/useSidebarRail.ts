import { useState } from "react";
import { TABLET_QUERY, useMediaQuery, WIDE_QUERY } from "./useMediaQuery";

/** The `localStorage` key that remembers whether the sidebar is an icon rail. */
export const SIDEBAR_KEY = "domovoi.sidebar";

type Choice = "collapsed" | "expanded";

function readChoice(): Choice | undefined {
  try {
    const saved = localStorage.getItem(SIDEBAR_KEY);
    return saved === "collapsed" || saved === "expanded" ? saved : undefined;
  } catch {
    return undefined;
  }
}

function writeChoice(choice: Choice): void {
  try {
    localStorage.setItem(SIDEBAR_KEY, choice);
  } catch {
    // Private windows and blocked site data: the choice then lasts for the visit only.
  }
}

/**
 * Whether the sidebar is an icon rail, and how to switch. Until the person chooses, it is a
 * rail on a tablet and full width on a desktop; the choice is remembered. A phone has no
 * rail: its navigation is a sheet.
 */
export function useSidebarRail(): { rail: boolean; toggle: () => void } {
  const [choice, setChoice] = useState(readChoice);
  const wide = useMediaQuery(WIDE_QUERY);
  const tablet = useMediaQuery(TABLET_QUERY);
  const rail = wide && (choice ? choice === "collapsed" : tablet);
  const toggle = () => {
    const next: Choice = rail ? "expanded" : "collapsed";
    setChoice(next);
    writeChoice(next);
  };
  return { rail, toggle };
}
