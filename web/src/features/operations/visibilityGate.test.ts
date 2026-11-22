import { describe, expect, it } from "vitest";

const modules = import.meta.glob("../../**/*.{ts,tsx}", {
  query: "?raw",
  eager: true,
  import: "default",
}) as Record<string, string>;

/** `../x/y.ts` as seen from this folder, as a path from `src/`. */
function fromSrc(key: string): string {
  const parts = ["features", "operations"];
  for (const part of key.split("/")) {
    if (part === "..") {
      parts.pop();
    } else if (part !== ".") {
      parts.push(part);
    }
  }
  return parts.join("/");
}

/** Non-test sources, keyed by their path from `src/`. */
const sources: Record<string, string> = {};
for (const [path, text] of Object.entries(modules)) {
  const clean = fromSrc(path);
  if (/\.test\.tsx?$/.test(clean) || clean.startsWith("test/")) {
    continue;
  }
  sources[clean] = text;
}

const named = (file: string) =>
  Object.entries(sources).filter(([path]) => path.split("/").pop() === file);

/** Words that mean "this code reads the detail level". */
const VISIBILITY_WORDS = [
  "useVisibility",
  "useProfile",
  "Visible",
  "presentationFor",
  "DetailsScope",
  "visibilityAttrs",
];

/** Strips comments, so a doc comment may talk about the detail level. */
function code(text: string): string {
  return text.replace(/\/\*[\s\S]*?\*\//g, "").replace(/(^|[^:])\/\/.*$/gm, "$1");
}

interface Usage {
  attrs: string;
  file: string;
}

/** Every `<Visible ...>` opening tag in `features` and `app`, with its attribute text. */
function visibleUsages(): Usage[] {
  const found: Usage[] = [];
  for (const [file, text] of Object.entries(sources)) {
    if (!file.startsWith("features/") && !file.startsWith("app/")) {
      continue;
    }
    const body = code(text);
    const open = /<Visible(?=[\s>/])/g;
    let match: RegExpExecArray | null;
    while ((match = open.exec(body)) !== null) {
      let depth = 0;
      let quote = "";
      let at = match.index + match[0].length;
      for (; at < body.length; at += 1) {
        const char = body[at] as string;
        if (quote) {
          if (char === quote && body[at - 1] !== "\\") {
            quote = "";
          }
        } else if (char === '"' || char === "'" || char === "`") {
          quote = char;
        } else if (char === "{") {
          depth += 1;
        } else if (char === "}") {
          depth -= 1;
        } else if (char === ">" && depth === 0) {
          break;
        }
      }
      found.push({ file, attrs: body.slice(match.index + match[0].length, at) });
    }
  }
  return found;
}

describe("visibility gates", () => {
  it("visibility_never_gates_approval_or_banner_code", () => {
    const files = [
      "StepApproveControl.tsx",
      "ApproveControl.tsx",
      "OperationBanner.tsx",
      "banner.ts",
    ];
    let checked = 0;
    for (const file of files) {
      for (const [path, text] of named(file)) {
        checked += 1;
        const body = code(text);
        for (const word of VISIBILITY_WORDS) {
          expect(new RegExp(`\\b${word}\\b`).test(body), `${path} uses ${word}`).toBe(false);
        }
        expect(body, `${path} imports the visibility design module`).not.toMatch(
          /design\/visibility/,
        );
      }
    }
    // StepApproveControl.tsx, OperationBanner.tsx and banner.ts exist today.
    expect(checked).toBeGreaterThanOrEqual(3);
  });

  it("no_api_code_imports_visibility", () => {
    const api = Object.entries(sources).filter(([path]) => path.startsWith("api/"));
    expect(api.length).toBeGreaterThan(0);
    const visibilityImport =
      /from\s+["'][^"']*(design\/visibility|\/components(\/index)?|components\/(Visible|DetailsScope|Disclosure|visibilityContext))["']/;
    for (const [path, text] of api) {
      expect(code(text), `${path} imports the visibility module or kit`).not.toMatch(
        visibilityImport,
      );
    }
  });

  it("visible_usage_follows_the_rules", () => {
    const usages = visibleUsages();
    expect(usages.length).toBeGreaterThan(10);
    const shape = /^[a-z][a-z0-9-]*(\.[a-z][a-z0-9-]*)+$/;
    for (const { file, attrs } of usages) {
      const where = `${file}: <Visible${attrs.replace(/\s+/g, " ")}`;
      const id = /\bid="([^"]*)"/.exec(attrs)?.[1];
      expect(id, `${where} needs a string-literal id`).toBeDefined();
      expect(shape.test(id as string), `${where} id must look like page.block`).toBe(true);
      const category = /\bcategory="(P[0-3])"/.exec(attrs)?.[1];
      expect(category, `${where} needs a literal category`).toBeDefined();
      if (category === "P2" || category === "P3") {
        expect(/\blabel=/.test(attrs), `${where} needs a label`).toBe(true);
      } else {
        expect(/\breveal\b/.test(attrs), `${where} cannot reveal a ${category}`).toBe(false);
      }
    }
  });

  it("visible_ids_are_unique_per_category", () => {
    const seen = new Map<string, string>();
    for (const { file, attrs } of visibleUsages()) {
      const id = /\bid="([^"]*)"/.exec(attrs)?.[1] ?? "";
      const category = /\bcategory="(P[0-3])"/.exec(attrs)?.[1] ?? "";
      const before = seen.get(id);
      if (before !== undefined) {
        expect(category, `${id} (${file}) is used with two categories`).toBe(before);
      }
      seen.set(id, category);
    }
  });

  it("every_p0_id_of_the_table_appears_in_the_sources", () => {
    const ids = [
      "operations.table",
      "operations.state",
      "operation.title",
      "operation.controls",
      "operation.stale-note",
      "operation.schema-warning",
      "operation.banner",
      "operation.progress",
      "operation.waiting",
      "operation.scope",
      "inspector.outcome",
      "inspector.approval",
      "approval.card",
      "incident.investigation-status",
      "control.tiles",
      "control.attention",
      "control.running",
      "control.approvals",
      "control.hosts",
      "attention.list",
      "search.scope",
      "search.status",
      "search.history-label",
    ];
    const usages = visibleUsages();
    const stepRows = Object.values(sources).some((text) => text.includes('"operation.step"'));
    expect(stepRows, "operation.step rows are marked").toBe(true);
    for (const id of ids) {
      const hits = usages.filter(({ attrs }) =>
        new RegExp(`\\bid="${id.replace(".", "\\.")}"`).test(attrs),
      );
      expect(hits.length, `${id} is a Visible id in a source file`).toBeGreaterThan(0);
      expect(
        hits.every(({ attrs }) => /\bcategory="P0"/.test(attrs)),
        `${id} is P0 everywhere it is used`,
      ).toBe(true);
    }
  });
});
