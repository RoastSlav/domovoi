// @vitest-environment node
import { readdirSync, readFileSync } from "node:fs";
import { join, relative, sep } from "node:path";
import { describe, expect, it } from "vitest";

const SRC = join(process.cwd(), "src");
const GRAPH = join(SRC, "components", "graph");
const LIBRARY = /["']@dagrejs\/[^"']*["']/;

function walk(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    return entry.isDirectory() ? walk(path) : [path];
  });
}

const isTest = (path: string): boolean => /\.test\.tsx?$/.test(path);
const code = (path: string): boolean => /\.(ts|tsx)$/.test(path);
const rel = (path: string): string => relative(SRC, path).split(sep).join("/");
const read = (path: string): string => readFileSync(path, "utf8");
const graphFiles = (): string[] => walk(GRAPH).filter(code);
const graphDirs = (): string[] =>
  [...graphFiles(), ...walk(join(SRC, "features", "operations", "graph")).filter(code)].filter(
    (path) => !isTest(path),
  );

/** The files of the engine: the one that imports the library, and what it is split into. */
const ENGINE = [
  "GraphEngine.tsx",
  "layout.ts",
  "animate.ts",
  "viewport.ts",
  "useViewport.ts",
  "tweenLayout.ts",
  "engineParts.tsx",
  "nodeText.ts",
].map((name) => join(GRAPH, name));

describe("graph guards", () => {
  it("only_the_graph_directory_imports_the_library", () => {
    const offenders = walk(SRC)
      .filter(code)
      .filter((path) => !isTest(path) && LIBRARY.test(read(path)))
      .map(rel)
      .filter((path) => !path.startsWith("components/graph/"));
    expect(offenders).toEqual([]);
    const importers = graphFiles().filter((path) => LIBRARY.test(read(path)));
    expect(importers.map(rel)).toEqual(["components/graph/layout.ts"]);
  });

  it("the_engine_is_loaded_only_through_a_dynamic_import", () => {
    const references = walk(SRC)
      .filter((path) => code(path) && !isTest(path))
      .flatMap((path) =>
        read(path)
          .split("\n")
          .filter((line) => /["']\.\/GraphEngine["']/.test(line))
          .map((line) => ({ file: rel(path), line: line.trim() })),
      );
    expect(references.length).toBeGreaterThan(0);
    for (const reference of references) {
      expect(reference.file).toBe("components/graph/StepGraph.tsx");
      expect(reference.line).toMatch(/import\(["']\.\/GraphEngine["']\)/);
    }
  });

  it("no_graph_file_uses_html_injection", () => {
    for (const path of graphDirs()) {
      expect(read(path), rel(path)).not.toMatch(/dangerouslySetInnerHTML|innerHTML|outerHTML/);
    }
  });

  it("no_graph_file_writes_a_style_attribute", () => {
    for (const path of graphDirs()) {
      const text = read(path);
      expect(text, rel(path)).not.toMatch(/\.style\b|setAttribute\(\s*["']style|cssText|<style/);
    }
  });

  it("no_literal_millisecond_durations_in_the_engine", () => {
    for (const path of ENGINE) {
      const text = read(path).replace(/\/\*[\s\S]*?\*\/|\/\/.*$/gm, "");
      expect(text, rel(path)).not.toMatch(/\b\d+(\.\d+)?\s*ms\b/);
      expect(text, rel(path)).not.toMatch(/\.run\(\s*\d{2,}/);
      expect(text, rel(path)).not.toMatch(/setTimeout|setInterval/);
    }
    const viewport = read(join(GRAPH, "useViewport.ts"));
    expect(viewport).toContain('motionMs("layout")');
    expect(read(join(GRAPH, "GraphEngine.tsx"))).toMatch(/motionMs\("graph"\)/);
  });

  it("graph_code_cannot_approve", () => {
    const forbidden =
      /operationControls|useApprove|useApproveStep|useDenyStep|useCancelOperation|usePauseOperation|useResumeOperation|useAnswerStep|\.mutate\(/;
    for (const path of graphDirs()) {
      expect(read(path), rel(path)).not.toMatch(forbidden);
    }
  });

  it("the_graph_makes_no_network_request", () => {
    for (const path of graphDirs()) {
      expect(read(path), rel(path)).not.toMatch(
        /\bfetch\(|XMLHttpRequest|WebSocket|sendBeacon|EventSource/,
      );
    }
  });
});
