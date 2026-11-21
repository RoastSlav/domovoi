import {
  CATEGORIES,
  CATEGORY_META,
  PRESENTATION,
  PRESENTATION_WORDS,
  PROFILE_IDS,
  PROFILE_META,
  isProfileId,
  presentationFor,
  visibilityOf,
} from "./visibility";

test("p0_and_p1_are_shown_in_every_profile", () => {
  for (const profile of PROFILE_IDS) {
    expect(presentationFor(profile, "P0")).toBe("shown");
    expect(presentationFor(profile, "P1")).toBe("shown");
  }
});

test("table_matches_the_product_decision_exactly", () => {
  expect(PRESENTATION).toEqual({
    detailed: { P0: "shown", P1: "shown", P2: "shown", P3: "shown" },
    standard: { P0: "shown", P1: "shown", P2: "collapsed", P3: "collapsed" },
    essential: { P0: "shown", P1: "shown", P2: "collapsed", P3: "hidden" },
  });
});

test("p0_is_shown_even_if_the_table_were_edited", () => {
  const original = PRESENTATION.essential.P0;
  try {
    (PRESENTATION.essential as Record<string, string>).P0 = "hidden";
    expect(presentationFor("essential", "P0")).toBe("shown");
  } finally {
    (PRESENTATION.essential as Record<string, string>).P0 = original;
  }
});

test("visibility_of_has_exactly_one_flag_true", () => {
  for (const profile of PROFILE_IDS) {
    for (const category of CATEGORIES) {
      const v = visibilityOf(profile, category);
      expect([v.shown, v.collapsed, v.hidden].filter(Boolean)).toHaveLength(1);
      expect(v.presentation).toBe(presentationFor(profile, category));
    }
  }
});

test("profile_meta_category_meta_and_words_cover_every_value", () => {
  const texts = [
    ...PROFILE_IDS.flatMap((id) => [PROFILE_META[id].label, PROFILE_META[id].hint]),
    ...CATEGORIES.flatMap((id) => [CATEGORY_META[id].label, CATEGORY_META[id].hint]),
    ...Object.values(PRESENTATION_WORDS),
  ];
  expect(texts.length).toBeGreaterThan(0);
  for (const text of texts) {
    expect(text.length).toBeGreaterThan(0);
    expect(text).not.toContain("!");
    expect(text.charAt(0)).toBe(text.charAt(0).toUpperCase());
  }
});

test("is_profile_id_rejects_everything_but_the_three_names", () => {
  for (const good of ["detailed", "standard", "essential"]) {
    expect(isProfileId(good)).toBe(true);
  }
  for (const bad of ["", "Detailed", null, 42, undefined, {}]) {
    expect(isProfileId(bad)).toBe(false);
  }
});
