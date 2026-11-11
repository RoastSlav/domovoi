import type { IconName } from "../design/icons";

/** The sidebar's pages: path, label and the icon drawn beside the label. */
export const NAV: readonly (readonly [path: string, label: string, icon: IconName])[] = [
  ["/incidents", "Incidents", "alert"],
  ["/chat", "Chat", "chat"],
  ["/rules", "Known conditions", "rules"],
  ["/approvals", "Approvals", "approve"],
  ["/hosts", "Hosts", "server"],
  ["/skills", "Skills", "tool"],
  ["/notes", "Notes", "note"],
  ["/digest", "Digest", "digest"],
  ["/logs", "Logs", "log"],
  ["/settings", "Settings", "gear"],
];
