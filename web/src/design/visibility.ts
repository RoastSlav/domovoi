/**
 * What each detail level shows. Every element of a screen has one category; the profile and
 * the category decide, through the table below and nothing else, whether it is shown,
 * collapsed or hidden. This file is pure: no React, no storage.
 *
 * A profile is a display preference. It never changes what is allowed or what is sent.
 */

/** P0 safety, approval, audit and current state; P1 primary; P2 technical detail; P3 metadata. */
export type Category = "P0" | "P1" | "P2" | "P3";

/** All categories, most important first. */
export const CATEGORIES: readonly Category[] = ["P0", "P1", "P2", "P3"];

/** The three detail levels. */
export type ProfileId = "detailed" | "standard" | "essential";

/** All profiles, most to least detailed. */
export const PROFILE_IDS: readonly ProfileId[] = ["detailed", "standard", "essential"];

/** The level a new owner gets, and the one used whenever nothing else is known. */
export const DEFAULT_PROFILE: ProfileId = "detailed";

/** How an element appears: drawn, behind a toggle, or behind a "+N details" button. */
export type Presentation = "shown" | "collapsed" | "hidden";

/** The product decision, as data. A page that needs another result changes an element's category. */
export const PRESENTATION: Readonly<Record<ProfileId, Readonly<Record<Category, Presentation>>>> = {
  detailed: { P0: "shown", P1: "shown", P2: "shown", P3: "shown" },
  standard: { P0: "shown", P1: "shown", P2: "collapsed", P3: "collapsed" },
  essential: { P0: "shown", P1: "shown", P2: "collapsed", P3: "hidden" },
};

/** The one lookup. P0 is "shown" whatever the table or the profile says. */
export function presentationFor(profile: ProfileId, category: Category): Presentation {
  if (category === "P0") {
    return "shown";
  }
  return PRESENTATION[profile]?.[category] ?? "shown";
}

/** Whether a value is one of the three profile names. */
export function isProfileId(value: unknown): value is ProfileId {
  return typeof value === "string" && (PROFILE_IDS as readonly string[]).includes(value);
}

/** The answer `useVisibility` gives: exactly one of `shown`, `collapsed` and `hidden` is true. */
export interface Visibility {
  category: Category;
  collapsed: boolean;
  hidden: boolean;
  presentation: Presentation;
  profile: ProfileId;
  shown: boolean;
}

/** The presentation of `category` in `profile`, with the three flags. */
export function visibilityOf(profile: ProfileId, category: Category): Visibility {
  const presentation = presentationFor(profile, category);
  return {
    category,
    profile,
    presentation,
    shown: presentation === "shown",
    collapsed: presentation === "collapsed",
    hidden: presentation === "hidden",
  };
}

/** The words for each profile. */
export const PROFILE_META: Readonly<Record<ProfileId, { hint: string; label: string }>> = {
  detailed: {
    label: "Detailed",
    hint: "Shows everything, technical detail and metadata included.",
  },
  standard: {
    label: "Standard",
    hint: "Shows what matters. Technical detail and metadata are one click away.",
  },
  essential: {
    label: "Essential",
    hint: 'Shows what matters and what needs you. Technical detail is one click away; metadata is behind a "+N details" button.',
  },
};

/** The words for each category. */
export const CATEGORY_META: Readonly<Record<Category, { hint: string; label: string }>> = {
  P0: {
    label: "Safety and state",
    hint: "Approvals, what is happening now, what authority applies and what was recorded. Always shown.",
  },
  P1: { label: "Primary", hint: "The page's main content and frequent actions." },
  P2: {
    label: "Technical detail",
    hint: "Diagnostics, raw inputs and outputs, relations and history.",
  },
  P3: { label: "Metadata", hint: "Identifiers, times and other rarely needed facts." },
};

/** The words for each presentation, as the settings table shows them. */
export const PRESENTATION_WORDS: Readonly<Record<Presentation, string>> = {
  shown: "Shown",
  collapsed: "Collapsed, one click away",
  hidden: 'Behind a "+N details" button',
};

/** The pages that offer their own detail level. A later page appends itself here. */
export const VISIBILITY_PAGES: readonly { key: string; label: string }[] = [
  { key: "operations", label: "Operations list" },
  { key: "operation", label: "Operation detail" },
  { key: "policy", label: "Policy overview and versions" },
  { key: "policy-draft", label: "Policy editor and publication" },
  { key: "policy-decision", label: "Policy decision" },
  { key: "policy-simulator", label: "Policy simulator" },
];
