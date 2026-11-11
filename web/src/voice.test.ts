const sources = import.meta.glob<string>(
  ["./{features,app,components,design}/**/*.{ts,tsx}", "!./**/*.test.{ts,tsx}"],
  { query: "?raw", import: "default", eager: true },
);

/** Phrases the voice rules forbid in anything a person reads (web/DESIGN.md, "Messaging"). */
const BANNED: [RegExp, string][] = [
  [/\boops\b|\bwhoops\b/i, 'No "Oops": say what happened.'],
  [/something went wrong/i, "Say what happened and what to do next."],
  [/\bError:/, 'No "Error:" prefix: say what could not be done.'],
  [/(^|["'>] *)Failed to\b/, 'Say "Could not …" instead of "Failed to".'],
  [/(^|["'>] *)Unable to\b/, 'Say "Could not …" instead of "Unable to".'],
  [/\bplease\b/i, "Plain and calm: leave out “please”."],
  [/>[^<>{}]*[A-Za-z]!\s*</, "No exclamation marks in text."],
  [/"[A-Z][^"\n]{2,}[a-z]!"/, "No exclamation marks in text."],
];

/** The string literals and JSX text of a source file, without comments. */
function readable(source: string): string[] {
  const stripped = source.replace(/\/\*[\s\S]*?\*\//g, "").replace(/^\s*\/\/.*$/gm, "");
  return stripped.split("\n");
}

describe("the voice rules", () => {
  it("finds the sources", () => {
    expect(Object.keys(sources).length).toBeGreaterThan(100);
  });

  it("no user-facing text is vague, blaming or excited", () => {
    const found = Object.entries(sources).flatMap(([path, source]) =>
      readable(source).flatMap((line, index) =>
        BANNED.filter(([pattern]) => pattern.test(line)).map(
          ([, message]) => `${path}:${index + 1} ${message} (${line.trim().slice(0, 80)})`,
        ),
      ),
    );
    expect(found).toEqual([]);
  });

  it("the rules catch what they describe", () => {
    const hits = (text: string) => BANNED.some(([pattern]) => pattern.test(text));
    expect(hits("<p>Oops, that did not work</p>")).toBe(true);
    expect(hits('const a = "Failed to save the note";')).toBe(true);
    expect(hits("<p>Saved!</p>")).toBe(true);
    expect(hits("<p>Could not save the note.</p>")).toBe(false);
  });
});
