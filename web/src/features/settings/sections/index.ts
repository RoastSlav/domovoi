import { HUB_SECTIONS } from "./hub";
import { DISCORD_SECTION, OUTSIDE_SECTIONS } from "./integrations";
import { MODEL_SECTIONS } from "./models";
import type { SectionSpec } from "./types";
import { UPKEEP_SECTIONS } from "./upkeep";
import { VIEW_SECTIONS } from "./views";

export type { SectionExtra, SectionSpec } from "./types";

/**
 * The hub's sections, in page order. To add one (Account, for example), add one entry
 * here: a section with `fields` is a form, one with `view` draws itself.
 */
export const SECTIONS: SectionSpec[] = [
  ...HUB_SECTIONS,
  DISCORD_SECTION,
  ...MODEL_SECTIONS,
  ...OUTSIDE_SECTIONS,
  ...UPKEEP_SECTIONS,
  ...VIEW_SECTIONS,
];

/** The section named by a URL segment, or `undefined` for an unknown one. */
export function findSection(id: string | undefined): SectionSpec | undefined {
  return SECTIONS.find((section) => section.id === id);
}
