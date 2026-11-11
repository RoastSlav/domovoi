import { AboutSection } from "../views/AboutSection";
import { StatusSection } from "../views/StatusSection";
import type { SectionSpec } from "./types";

const none = () => [];

/** The sections that show the hub's state instead of editing it. */
export const VIEW_SECTIONS: SectionSpec[] = [
  {
    id: "status",
    title: "Status",
    intro: "Model spending this month, the heartbeat and open advisories.",
    fields: none,
    view: StatusSection,
  },
  {
    id: "about",
    title: "About",
    intro: "Which version the hub and its agents run, and whether a newer release is out.",
    fields: none,
    view: AboutSection,
  },
];
