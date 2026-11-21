import { DEFAULT_PRESENTATION_CHOICE, type PresentationChoice } from "../components";
import { DEFAULT_PROFILE, isProfileId, type ProfileId } from "../design/visibility";

/** The browser key of the last detail level this browser saw. */
export const VISIBILITY_KEY = "domovoi.visibility";

/** Reads a stored or received presentation. Total: anything unreadable is Detailed. */
export function parsePresentation(raw: unknown): PresentationChoice {
  if (typeof raw !== "object" || raw === null) {
    return DEFAULT_PRESENTATION_CHOICE;
  }
  const { profile, pages } = raw as { pages?: unknown; profile?: unknown };
  const kept: Record<string, ProfileId> = {};
  if (typeof pages === "object" && pages !== null && !Array.isArray(pages)) {
    for (const [key, value] of Object.entries(pages)) {
      if (isProfileId(value)) {
        kept[key] = value;
      }
    }
  }
  return { profile: isProfileId(profile) ? profile : DEFAULT_PROFILE, pages: kept };
}

/** The last choice this browser saw, or undefined when there is none or storage is unreadable. */
export function readCache(): PresentationChoice | undefined {
  try {
    const text = window.localStorage.getItem(VISIBILITY_KEY);
    return text === null ? undefined : parsePresentation(JSON.parse(text));
  } catch {
    return undefined;
  }
}

/** Remembers the choice for the next first paint. A storage failure is ignored. */
export function writeCache(choice: PresentationChoice): void {
  try {
    window.localStorage.setItem(VISIBILITY_KEY, JSON.stringify(choice));
  } catch {
    // The cache only avoids a flash on load; the hub is the authority.
  }
}
