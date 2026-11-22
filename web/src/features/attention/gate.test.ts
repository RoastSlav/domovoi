import { describe, expect, it } from "vitest";

const modules = import.meta.glob("../**/*.{ts,tsx}", {
  query: "?raw",
  eager: true,
  import: "default",
}) as Record<string, string>;

/** Strips comments, so a doc comment may name what the code must not use. */
function code(text: string): string {
  return text.replace(/\/\*[\s\S]*?\*\//g, "").replace(/(^|[^:])\/\/.*$/gm, "$1");
}

const FORBIDDEN = ["useApprove", "useApproveStep", "useDeny", "useDenyStep"];

describe("attention and search gates", () => {
  it("attention_and_search_code_never_imports_an_approve_hook", () => {
    const checked = Object.entries(modules).filter(
      ([path]) => /^\.\.\/(attention|search|overview)\//.test(path) && !/\.test\.tsx?$/.test(path),
    );
    expect(checked.length, "the folders were found").toBeGreaterThan(10);
    for (const [path, text] of checked) {
      const body = code(text);
      for (const name of FORBIDDEN) {
        expect(new RegExp(`\b${name}\b`).test(body), `${path} uses ${name}`).toBe(false);
      }
    }
  });
});
