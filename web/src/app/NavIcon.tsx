/** The sidebar's pages: path, label and the icon drawn beside the label. */
export const NAV = [
  ["/incidents", "Incidents", "alert"],
  ["/chat", "Chat", "chat"],
  ["/rules", "Known conditions", "rules"],
  ["/approvals", "Approvals", "check"],
  ["/hosts", "Hosts", "server"],
  ["/skills", "Skills", "tool"],
  ["/notes", "Notes", "note"],
  ["/digest", "Digest", "list"],
  ["/logs", "Logs", "log"],
  ["/settings", "Settings", "gear"],
] as const;

/** Which icon a nav entry uses. */
export type IconName = (typeof NAV)[number][2];

/** Stroke paths on a 16x16 grid, one entry per icon. */
const PATHS: Record<IconName, string> = {
  alert: "M8 2 1.5 13.5h13L8 2Zm0 4v4m0 2v.5",
  chat: "M2.5 3.5h11v7h-5.5l-3.5 3v-3h-2v-7Z",
  rules: "M3 3h10v10H3V3Zm2.5 3h5m-5 2h5m-5 2h3",
  check: "M2.5 8.5 6 12l7.5-8",
  server: "M2 3h12v4H2V3Zm0 6h12v4H2V9Zm2-4h.5m-.5 6h.5",
  tool: "M10.5 2.5a3 3 0 0 0-2.7 4L2.5 11.8l1.7 1.7L9.5 8.2a3 3 0 0 0 4-2.7l-2 2-1.5-.5-.5-1.5 2-2a3 3 0 0 0-1-.5Z",
  note: "M3.5 2h6l3 3v9h-9V2Zm6 0v3h3M5.5 8h5m-5 2.5h5",
  list: "M2.5 4h11m-11 4h11m-11 4h7",
  log: "M2.5 3.5h11v9h-11Zm2 2l2 2l-2 2m3.5 2h3",
  gear: "M8 5.5a2.5 2.5 0 1 0 0 5 2.5 2.5 0 0 0 0-5ZM8 1.5v2m0 9v2m6.5-6.5h-2m-9 0h-2m10.6-4.6-1.4 1.4m-6.4 6.4-1.4 1.4m0-9.2 1.4 1.4m6.4 6.4 1.4 1.4",
};

/** A decorative 16px icon; the label next to it carries the meaning. */
export function NavIcon({ name }: { name: IconName }) {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      <path d={PATHS[name]} />
    </svg>
  );
}
