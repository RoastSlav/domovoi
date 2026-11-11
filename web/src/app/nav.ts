import type { Overview } from "../api/types";
import type { IconName } from "../design/icons";
import type { Tone } from "../design/tone";
import { PAGES } from "./pages";

/** A number beside a navigation link: which count of the overview, and what it counts. */
export interface NavCount {
  field: {
    [K in keyof Overview]: Overview[K] extends number ? K : never;
  }[keyof Overview];
  /** Read after the number by assistive technology: "2 waiting". */
  label: string;
  tone: Tone;
}

/** One link in the navigation. */
export interface NavItem {
  count?: NavCount;
  icon: IconName;
  label: string;
  /** Starts downloading the page, for links to pages outside the first download. */
  preload?: () => void;
  to: string;
}

/** A titled group of links. */
export interface NavGroup {
  items: NavItem[];
  title: string;
}

/**
 * The navigation, grouped by what the operator is doing. To add a page, add its link to the
 * group it belongs to; a count shows only while it is above zero.
 */
export const NAV_GROUPS: readonly NavGroup[] = [
  {
    title: "Respond",
    items: [
      {
        to: "/incidents",
        label: "Incidents",
        icon: "alert",
        count: { field: "incidents_action_needed", tone: "danger", label: "need attention" },
      },
      {
        to: "/approvals",
        label: "Approvals",
        icon: "approve",
        count: { field: "approvals_waiting", tone: "pending", label: "waiting" },
        preload: PAGES.approvals.preload,
      },
      { to: "/chat", label: "Chat", icon: "chat", preload: PAGES.chat.preload },
    ],
  },
  {
    title: "Fleet",
    items: [
      {
        to: "/hosts",
        label: "Hosts",
        icon: "server",
        count: { field: "hosts_offline", tone: "danger", label: "offline" },
        preload: PAGES.hosts.preload,
      },
      { to: "/skills", label: "Skills", icon: "tool", preload: PAGES.skills.preload },
      { to: "/logs", label: "Logs", icon: "log", preload: PAGES.logs.preload },
    ],
  },
  {
    title: "Knowledge",
    items: [
      { to: "/rules", label: "Known conditions", icon: "rules", preload: PAGES.rules.preload },
      { to: "/notes", label: "Notes", icon: "note", preload: PAGES.notes.preload },
      { to: "/digest", label: "Digest", icon: "digest", preload: PAGES.digest.preload },
    ],
  },
  {
    // The audit log goes here when it exists.
    title: "System",
    items: [{ to: "/settings", label: "Settings", icon: "gear", preload: PAGES.settings.preload }],
  },
];

/** Every navigation link, in order. */
export const NAV_ITEMS: readonly NavItem[] = NAV_GROUPS.flatMap((group) => group.items);
