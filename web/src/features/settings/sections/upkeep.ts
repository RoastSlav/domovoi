import { AboutSection } from "../views/AboutSection";
import { fixed, type SectionSpec } from "./types";

/** The sections about the hub's own upkeep: leaked secrets, backups and release checks. */
export const UPKEEP_SECTIONS: SectionSpec[] = [
  {
    id: "exposed-secrets",
    title: "Exposed secrets",
    intro:
      "Tell the hub about secrets that leaked, so it keeps an advisory open until you rotate them.",
    fields: fixed([
      {
        path: "secrets.exposed",
        label: "Secrets known to be exposed",
        kind: "list",
        hint: "Names of secrets you know leaked. Each raises an advisory until you remove it here.",
      },
    ]),
  },
  {
    id: "backups",
    title: "Backups",
    intro: "Where the hub keeps copies of its own data, and how many.",
    // A status line about the latest backup goes here: SectionBody leaves a slot above the fields.
    fields: fixed([
      { path: "backup.dir", label: "Backup directory", kind: "text" },
      { path: "backup.keep", label: "Backups to keep", kind: "number" },
    ]),
  },
  {
    id: "updates",
    title: "About and updates",
    intro:
      "Which version the hub and its agents run, and whether a newer release is out. The hub looks on GitHub twice a day; nothing is sent but the request itself.",
    view: AboutSection,
    fields: fixed([{ path: "updates.check", label: "Check for new releases", kind: "bool" }]),
  },
];
