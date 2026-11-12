import { useState } from "react";

/** How the hosts page lists hosts: a card each, or one table. */
export type HostsView = "cards" | "table";

/** The `localStorage` key that remembers the person's choice between visits. */
export const HOSTS_VIEW_KEY = "domovoi.hosts-view";

function saved(): HostsView {
  try {
    return localStorage.getItem(HOSTS_VIEW_KEY) === "table" ? "table" : "cards";
  } catch {
    // Storage can be blocked (private window, site data off); the default view still works.
    return "cards";
  }
}

/** The chosen hosts view and a setter that remembers the choice when storage allows. */
export function useHostsView(): [HostsView, (view: HostsView) => void] {
  const [view, setView] = useState<HostsView>(saved);
  const choose = (next: HostsView) => {
    setView(next);
    try {
      localStorage.setItem(HOSTS_VIEW_KEY, next);
    } catch {
      // Not remembered; the view still changes for this visit.
    }
  };
  return [view, choose];
}
