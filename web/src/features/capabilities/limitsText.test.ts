import { boundsText, formatHold, targetsText } from "./limitsText";

test("formatHold reads hours, minutes and seconds, and dashes a missing value", () => {
  expect(formatHold(3600)).toBe("1 h");
  expect(formatHold(1800)).toBe("30 min");
  expect(formatHold(90)).toBe("90 s");
  expect(formatHold(null)).toBe("–");
});

test("targetsText and boundsText say what an entry covers", () => {
  expect(targetsText({ kind: "any" })).toBe("Any target");
  expect(targetsText({ kind: "names", names: ["plex", "jellyfin"] })).toBe("plex, jellyfin");
  expect(boundsText({})).toBe("–");
  expect(boundsText({ min_prefix_len_v4: 24 })).toBe("min_prefix_len_v4: 24");
});
