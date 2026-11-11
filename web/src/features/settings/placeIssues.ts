import type { Issue } from "../../api/types";
import { fieldsForIssue } from "./form";

/** What the form can show of a list of hub issues. */
export interface PlacedIssues {
  /** Messages for each field that has a place in the form. */
  byField: Map<string, string[]>;
  /** The summary at the top: each issue with the field it links to, or no field. */
  summary: { path: string; message: string }[];
  /** Sections that hold at least one issue. */
  sections: Set<string>;
}

/**
 * Gives each hub issue the field it is about. An issue for a table covers its fields and is
 * shown on the first; one whose path matches no visible field has no place and is listed in
 * the summary with its path, so nothing the hub said is lost.
 *
 * `fieldsBySection` lists the visible field paths of each section, in page order.
 */
export function placeIssues(
  issues: readonly Issue[],
  fieldsBySection: ReadonlyMap<string, readonly string[]>,
): PlacedIssues {
  const known = [...fieldsBySection.values()].flat();
  const placed: PlacedIssues = { byField: new Map(), summary: [], sections: new Set() };
  const seen = new Set<string>();
  for (const issue of issues) {
    const key = `${issue.path}\n${issue.message}`;
    if (seen.has(key)) {
      continue;
    }
    seen.add(key);
    const first = fieldsForIssue(issue, known)[0];
    if (first === undefined) {
      const message = issue.path ? `${issue.path}: ${issue.message}` : issue.message;
      placed.summary.push({ path: "", message });
      continue;
    }
    placed.byField.set(first, [...(placed.byField.get(first) ?? []), issue.message]);
    placed.summary.push({ path: first, message: issue.message });
    for (const [section, paths] of fieldsBySection) {
      if (paths.includes(first)) {
        placed.sections.add(section);
      }
    }
  }
  return placed;
}
