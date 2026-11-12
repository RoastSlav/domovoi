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
];
