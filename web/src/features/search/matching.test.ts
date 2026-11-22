import { matchesAll, words } from "./matching";

test("matching_needs_every_word_ignores_case_and_treats_characters_literally", () => {
  expect(words("  Disk   DISK full ")).toEqual(["disk", "full"]);
  expect(words("   ")).toEqual([]);
  expect(matchesAll("Disk /srv is FULL", words("disk full"))).toBe(true);
  expect(matchesAll("Disk /srv is nearly empty", words("disk full"))).toBe(false);
  expect(matchesAll("axb", words("a.b"))).toBe(false);
  expect(matchesAll("a.b", words("a.b"))).toBe(true);
  expect(matchesAll("(x)", words("(x)"))).toBe(true);
  expect(matchesAll("anything", [])).toBe(false);
});
