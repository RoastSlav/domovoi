import { TOKENS_FILE, darkThemeBlocks, findCssProblems } from "./cssGuard";

const sheets = import.meta.glob<string>("../**/*.css", {
  query: "?raw",
  import: "default",
  eager: true,
});

describe("colour guardrail", () => {
  it("finds every stylesheet", () => {
    expect(Object.keys(sheets).length).toBeGreaterThan(10);
    expect(Object.keys(sheets).some((path) => path.endsWith(TOKENS_FILE))).toBe(true);
  });

  it("no stylesheet outside tokens.css has a raw colour or a palette primitive", () => {
    const found = Object.entries(sheets)
      .filter(([path]) => !path.endsWith(TOKENS_FILE))
      .flatMap(([path, css]) =>
        findCssProblems(css).map((p) => `${path}:${p.line} ${p.message} (${p.text})`),
      );
    expect(found).toEqual([]);
  });

  it("rejects raw colours and palette primitives in a fixture", () => {
    const fixture = [
      ".a { color: #b42318; }",
      ".b {",
      "  background: rgb(0 0 0 / 50%);",
      "  border: 1px solid hsl(20deg 10% 50%);",
      "  color: white;",
      "  outline-color: var(--palette-moss-600);",
      "}",
    ].join("\n");
    expect(findCssProblems(fixture).map((p) => p.line)).toEqual([1, 3, 4, 5, 6]);
  });

  it("accepts tokens, keywords and look-alike property names", () => {
    const fixture = [
      ".a {",
      "  color: var(--tone-danger-fg);",
      "  background: transparent;",
      "  border-color: currentColor;",
      "  white-space: nowrap;",
      "  /* color: #ffffff */",
      "}",
    ].join("\n");
    expect(findCssProblems(fixture)).toEqual([]);
  });

  it("the explicit and the system dark theme are the same block", () => {
    const tokens = Object.entries(sheets).find(([path]) => path.endsWith(TOKENS_FILE))?.[1] ?? "";
    const blocks = darkThemeBlocks(tokens);
    expect(blocks).toHaveLength(2);
    expect(blocks[0]).not.toBe("");
    expect(blocks[1]).toBe(blocks[0]);
  });
});
