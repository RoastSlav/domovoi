import { parsePresentation, readCache, VISIBILITY_KEY, writeCache } from "./visibilityCache";

test("parse_presentation_is_total", () => {
  expect(parsePresentation(null)).toEqual({ profile: "detailed", pages: {} });
  expect(parsePresentation(3)).toEqual({ profile: "detailed", pages: {} });
  expect(parsePresentation({ profile: "x" })).toEqual({ profile: "detailed", pages: {} });
  expect(
    parsePresentation({ profile: "essential", pages: { a: "standard", b: "zzz", c: 5 } }),
  ).toEqual({ profile: "essential", pages: { a: "standard" } });
  expect(parsePresentation({ profile: "standard", pages: ["detailed"] })).toEqual({
    profile: "standard",
    pages: {},
  });
});

test("cache_round_trips_and_ignores_unreadable_storage", () => {
  expect(readCache()).toBeUndefined();
  writeCache({ profile: "standard", pages: { operation: "essential" } });
  expect(readCache()).toEqual({ profile: "standard", pages: { operation: "essential" } });

  localStorage.setItem(VISIBILITY_KEY, "{not json");
  expect(readCache()).toBeUndefined();

  const spy = vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
    throw new Error("blocked");
  });
  expect(readCache()).toBeUndefined();
  spy.mockRestore();
  const set = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new Error("full");
  });
  expect(() => writeCache({ profile: "detailed", pages: {} })).not.toThrow();
  set.mockRestore();
});
