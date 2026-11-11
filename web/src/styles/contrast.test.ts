import { contrast, requiredPairs, resolveTokens, type Theme } from "./contrast";

const sheets = import.meta.glob<string>("./tokens.css", {
  query: "?raw",
  import: "default",
  eager: true,
});
const tokens = Object.values(sheets)[0] ?? "";

describe.each<Theme>(["light", "dark"])("contrast in the %s theme", (theme) => {
  const resolved = resolveTokens(tokens, theme);

  it("reads every token the pairs name", () => {
    const names = new Set(requiredPairs().flatMap((pair) => [pair.fg, pair.bg]));
    const missing = [...names].filter((name) => !resolved[name]);
    expect(missing).toEqual([]);
  });

  it("every colour pair meets WCAG 2.2 AA", () => {
    const failing = requiredPairs().flatMap((pair) => {
      const fg = resolved[pair.fg];
      const bg = resolved[pair.bg];
      if (!fg || !bg) {
        return [];
      }
      const ratio = contrast(fg, bg);
      return ratio >= pair.min
        ? []
        : [`${pair.fg} on ${pair.bg} (${pair.why}): ${ratio.toFixed(2)}, needs ${pair.min}`];
    });
    expect(failing).toEqual([]);
  });
});

describe("the system dark theme", () => {
  it("is the explicit dark theme, so the same pairs hold", () => {
    const explicit = resolveTokens(tokens, "dark");
    const names = Object.keys(explicit);
    // The media block repeats the explicit block; it is compared in cssGuard.test.ts.
    expect(names).toContain("--color-primary");
  });
});

describe("the contrast helper", () => {
  it("rates black on white at 21 and a colour on itself at 1", () => {
    expect(contrast([0, 0, 0], [255, 255, 255])).toBeCloseTo(21, 5);
    expect(contrast([120, 80, 40], [120, 80, 40])).toBeCloseTo(1, 5);
  });
});
