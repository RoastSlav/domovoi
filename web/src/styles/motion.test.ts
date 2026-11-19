import tokens from "./tokens.css?raw";

const REDUCED = tokens.slice(tokens.indexOf("@media (prefers-reduced-motion: reduce)"));
const ROOT = tokens.slice(0, tokens.indexOf("@media (prefers-reduced-motion: reduce)"));

function durations(css: string): [string, number][] {
  return [...css.matchAll(/(--motion-[a-z-]+):\s*(\d+)ms;/g)].map((m) => [
    m[1] ?? "",
    Number(m[2]),
  ]);
}

describe("motion tokens", () => {
  it("motion_tokens_never_exceed_600ms_and_reduced_motion_zeroes_every_one", () => {
    const all = durations(ROOT);
    expect(all.map(([name]) => name)).toContain("--motion-panel");
    for (const [name, ms] of all) {
      expect(ms, name).toBeLessThanOrEqual(600);
      expect(durations(REDUCED).find(([n]) => n === name)?.[1], `${name} in reduced motion`).toBe(
        0,
      );
    }
  });
});
