import { createContext, useContext, useMemo, type ReactNode } from "react";
import {
  DEFAULT_PROFILE,
  visibilityOf,
  type Category,
  type ProfileId,
  type Visibility,
} from "../design/visibility";

/** The owner's detail level and the levels they set for single pages. */
export interface PresentationChoice {
  pages: Readonly<Record<string, ProfileId>>;
  profile: ProfileId;
}

/** Detailed with no overrides: what everything shows when nothing else is known. */
export const DEFAULT_PRESENTATION_CHOICE: PresentationChoice = {
  profile: DEFAULT_PROFILE,
  pages: {},
};

/** No provider above means Detailed: a test or a login page never needs one, and nothing is hidden by accident. */
export const VisibilityContext = createContext<PresentationChoice>(DEFAULT_PRESENTATION_CHOICE);

const PageContext = createContext<string | undefined>(undefined);

/**
 * Names the page for per-page detail levels. Renders its children with no element of its own.
 *
 * For the root of a page that offers its own level. Not for sections inside a page.
 */
export function VisibilityPage({ page, children }: { children: ReactNode; page: string }) {
  return <PageContext.Provider value={page}>{children}</PageContext.Provider>;
}

/** The owner's default level and overrides, for the settings UI and the account menu. */
export function usePresentationChoice(): PresentationChoice {
  return useContext(VisibilityContext);
}

/** The profile in force at this place: the page's override when it has one, else the default. */
export function useProfile(): { custom: boolean; page: string | undefined; profile: ProfileId } {
  const choice = useContext(VisibilityContext);
  const page = useContext(PageContext);
  const override = page === undefined ? undefined : choice.pages[page];
  return { profile: override ?? choice.profile, page, custom: override !== undefined };
}

/**
 * The primitive every other piece builds on: how an element of `category` appears here.
 * Never throws; without a provider it answers Detailed.
 */
export function useVisibility(category: Category): Visibility {
  const { profile } = useProfile();
  return useMemo(() => visibilityOf(profile, category), [profile, category]);
}

/**
 * For an element that cannot take a wrapper (a list row, a table cell): spread the result onto
 * it. `id` names the kind of element (`operation.step`), never data.
 */
export function visibilityAttrs(
  category: Category,
  id: string,
): { "data-visibility": Category; "data-visibility-id": string } {
  return { "data-visibility": category, "data-visibility-id": id };
}
