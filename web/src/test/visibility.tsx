import type { ReactNode } from "react";
import { VisibilityContext } from "../components";
import { PROFILE_IDS, type ProfileId } from "../design/visibility";
import { renderAt } from "./render";

/** Supplies the detail level directly, as the app's provider would after the hub answered. */
export function ProfileProvider({
  profile,
  pages = {},
  children,
}: {
  children: ReactNode;
  pages?: Record<string, ProfileId>;
  profile: ProfileId;
}) {
  return (
    <VisibilityContext.Provider value={{ profile, pages }}>{children}</VisibilityContext.Provider>
  );
}

/** `renderAt` with the page drawn at `profile` (and per-page overrides). */
export function renderWithProfile(
  profile: ProfileId,
  url: string,
  path: string,
  element: ReactNode,
  pages?: Record<string, ProfileId>,
) {
  return renderAt(
    url,
    path,
    <ProfileProvider profile={profile} pages={pages ?? {}}>
      {element}
    </ProfileProvider>,
  );
}

/** Every element marked P0 under `root`. */
export function p0Elements(root: ParentNode = document): HTMLElement[] {
  return [...root.querySelectorAll<HTMLElement>('[data-visibility="P0"]')];
}

function idsOf(elements: HTMLElement[]): Map<string, number> {
  const counts = new Map<string, number>();
  for (const element of elements) {
    const id = element.dataset.visibilityId ?? "";
    counts.set(id, (counts.get(id) ?? 0) + 1);
  }
  return counts;
}

/**
 * Renders a screen once per profile and checks that no profile removes, collapses or hides a
 * P0 element: every expected id is present, no P0 element sits inside a collapsed, hidden or
 * aria-hidden ancestor, and each id occurs as often as in Detailed. Fails when `expectedIds`
 * is empty, so a test that proves nothing cannot pass.
 */
export async function expectP0Everywhere({
  render,
  expectedIds,
}: {
  expectedIds: readonly string[];
  render: (profile: ProfileId) => Promise<{ unmount: () => void }>;
}): Promise<void> {
  if (expectedIds.length === 0) {
    throw new Error("expectP0Everywhere needs at least one expected P0 id");
  }
  let baseline: Map<string, number> | undefined;
  for (const profile of PROFILE_IDS) {
    const view = await render(profile);
    const found = p0Elements();
    const counts = idsOf(found);
    for (const id of expectedIds) {
      expect(counts.has(id), `${id} is missing in ${profile}`).toBe(true);
    }
    for (const element of found) {
      expect(element.isConnected).toBe(true);
      expect(
        element.closest('[data-presentation="collapsed"], [hidden], [aria-hidden="true"]'),
        `${element.dataset.visibilityId} sits in a folded container in ${profile}`,
      ).toBeNull();
    }
    if (baseline === undefined) {
      baseline = counts;
    } else {
      expect([...counts.entries()].sort(), `P0 elements differ in ${profile}`).toEqual(
        [...baseline.entries()].sort(),
      );
    }
    view.unmount();
  }
}
