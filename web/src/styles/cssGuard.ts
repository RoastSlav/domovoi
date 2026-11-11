/** The file that may contain raw colour values and palette primitives. */
export const TOKENS_FILE = "/tokens.css";

const NAMED =
  "white|black|red|green|blue|yellow|orange|purple|pink|gray|grey|brown|silver|gold|navy|teal|maroon|olive|lime|aqua|fuchsia|beige|ivory|tan";
const RAW_COLOUR = new RegExp(
  `#[0-9a-fA-F]{3,8}\\b|\\b(?:rgba?|hsla?|hwb|lab|lch|oklab|oklch|color)\\(|(?<![\\w-])(?:${NAMED})(?![\\w-])`,
);

/** One place where a stylesheet breaks the colour rules. */
export interface CssProblem {
  line: number;
  message: string;
  text: string;
}

/**
 * The colour-rule violations in a stylesheet outside `tokens.css`: raw colour values (hex,
 * `rgb()`, `hsl()`, named colours) and `--palette-*` primitives. Components use semantic
 * tokens only.
 */
export function findCssProblems(css: string): CssProblem[] {
  const problems: CssProblem[] = [];
  const lines = css
    .replace(/\/\*[\s\S]*?\*\//g, (comment) => comment.replace(/[^\n]/g, " "))
    .split("\n");
  lines.forEach((text, index) => {
    const line = index + 1;
    if (text.includes("--palette-")) {
      problems.push({
        line,
        text: text.trim(),
        message: "Use a semantic token, not a palette primitive.",
      });
    }
    const colon = text.indexOf(":");
    if (colon === -1 || text.trimEnd().endsWith("{")) {
      return;
    }
    // Only the value can hold a colour; token names inside var() are not colour names.
    const value = text.slice(colon + 1).replace(/var\([^)]*\)/g, "");
    if (RAW_COLOUR.test(value)) {
      problems.push({
        line,
        text: text.trim(),
        message: "Raw colour: use a token from tokens.css.",
      });
    }
  });
  return problems;
}

/** The body of each dark-theme block in `tokens.css`, with indentation removed. */
export function darkThemeBlocks(tokensCss: string): string[] {
  return [...tokensCss.matchAll(/dark-theme:start \*\/([\s\S]*?)\/\* dark-theme:end/g)].map(
    (match) =>
      (match[1] ?? "")
        .split("\n")
        .map((line) => line.trim())
        .filter(Boolean)
        .join("\n"),
  );
}
