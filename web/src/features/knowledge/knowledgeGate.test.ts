import { describe, expect, it } from "vitest";

const modules = import.meta.glob("../../**/*.{ts,tsx}", {
  query: "?raw",
  eager: true,
  import: "default",
}) as Record<string, string>;

/** `../x/y.ts` as seen from this folder, as a path from `src/`. */
function fromSrc(key: string): string {
  const parts = ["features", "knowledge"];
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

/** Strips comments, so a doc comment may talk about the detail level or a forbidden word. */
function code(text: string): string {
  return text.replace(/\/\*[\s\S]*?\*\//g, "").replace(/(^|[^:])\/\/.*$/gm, "$1");
}

const inFolders = (...folders: string[]) =>
  Object.entries(sources).filter(([path]) => folders.some((folder) => path.startsWith(folder)));

const K = "features/knowledge/";
const E = "features/experience/";

/** Which files may call each write hook, besides the file that defines it: the controls that call it on a click. */
const WRITE_HOOKS: Record<string, { allowed: string[]; defined: string }> = {
  useAssertFact: { allowed: [`${K}AssertFactDialog.tsx`], defined: "api/hooks/knowledgeWrites.ts" },
  useAcceptFact: {
    allowed: [`${K}FactButtons.tsx`, `${K}ProposalsTab.tsx`, `${K}PathActions.tsx`],
    defined: "api/hooks/knowledgeWrites.ts",
  },
  useVerifyFact: { allowed: [`${K}FactButtons.tsx`], defined: "api/hooks/knowledgeWrites.ts" },
  useRetractFact: {
    allowed: [`${K}FactButtons.tsx`, `${K}ProposalsTab.tsx`, `${K}PathActions.tsx`],
    defined: "api/hooks/knowledgeWrites.ts",
  },
  useConfirmPath: { allowed: [`${K}PathActions.tsx`], defined: "api/hooks/knowledgeWrites.ts" },
  useEditSummaryField: {
    allowed: [`${E}SummaryFieldRow.tsx`, `${E}EditSummaryDialog.tsx`],
    defined: "api/hooks/experience.ts",
  },
};

/** The text of every `useEffect(...)` and `useLayoutEffect(...)` call in `text`, parentheses balanced. */
function effectBodies(text: string): string[] {
  const bodies: string[] = [];
  for (const match of text.matchAll(/\buse(?:Layout)?Effect\(/g)) {
    let depth = 1;
    let at = (match.index ?? 0) + match[0].length;
    const start = at;
    while (at < text.length && depth > 0) {
      const char = text[at];
      depth += char === "(" ? 1 : char === ")" ? -1 : 0;
      at += 1;
    }
    bodies.push(text.slice(start, at));
  }
  return bodies;
}

describe("knowledge_write_hooks_are_only_used_by_explicit_controls", () => {
  for (const [hook, { allowed, defined }] of Object.entries(WRITE_HOOKS)) {
    it(`${hook} is called only by ${allowed.map((file) => file.split("/").pop()).join(", ")}`, () => {
      const users = Object.entries(sources)
        .filter(([, text]) => new RegExp(`\\b${hook}\\(`).test(code(text)))
        .map(([path]) => path)
        .sort();
      expect(users).toEqual([defined, ...allowed].sort());
    });
  }

  it("never calls a write from inside an effect", () => {
    const controls = new Set(Object.values(WRITE_HOOKS).flatMap(({ allowed }) => allowed));
    const files = [...controls, `${K}useStepUp.tsx`];
    for (const file of files) {
      for (const body of effectBodies(code(sources[file] ?? ""))) {
        expect(body, `${file} writes inside an effect`).not.toMatch(
          /\.mutate(?:Async)?\(|\bstart\(|\brun\(|\bconfirm\(|\baccept\(|\bretract\(|\bverify\(/,
        );
      }
    }
  });

  it("finds every file it names", () => {
    for (const { allowed, defined } of Object.values(WRITE_HOOKS)) {
      for (const file of [defined, ...allowed]) {
        expect(sources, file).toHaveProperty([file]);
      }
    }
  });
});

describe("step_up_and_actions_contain_no_visibility_primitive", () => {
  const WORDS = [
    "useVisibility",
    "useProfile",
    "Visible",
    "presentationFor",
    "DetailsScope",
    "visibilityAttrs",
  ];
  const guarded = [
    `${K}useStepUp.tsx`,
    `${K}PathActions.tsx`,
    `${K}FactButtons.tsx`,
    "api/hooks/knowledge.ts",
    "api/hooks/knowledgeWrites.ts",
  ];

  it("keeps the password flow and the owner's buttons free of the detail level", () => {
    for (const file of guarded) {
      const text = code(sources[file] ?? "");
      expect(text.length, `${file} is missing`).toBeGreaterThan(0);
      for (const word of WORDS) {
        expect(text, `${file} uses ${word}`).not.toMatch(new RegExp(`\\b${word}\\b`));
      }
    }
  });

  it("keeps every module under src/api free of the detail level", () => {
    for (const [file, text] of Object.entries(sources).filter(([path]) =>
      path.startsWith("api/"),
    )) {
      expect(code(text), file).not.toMatch(/design\/visibility|visibilityContext/);
    }
  });
});

interface Usage {
  category: string | undefined;
  hasLabel: boolean;
  id: string | undefined;
}

/** Every `<Visible ...>` opening tag in the knowledge and experience features. */
function visibleUsages(): Usage[] {
  const found: Usage[] = [];
  for (const [, text] of inFolders(K, E)) {
    for (const tag of code(text).matchAll(/<Visible\b([\s\S]*?)>/g)) {
      const attrs = tag[1] ?? "";
      found.push({
        category: /category="(P[0-3])"/.exec(attrs)?.[1],
        id: /\bid="([^"]+)"/.exec(attrs)?.[1],
        hasLabel: /\blabel=/.test(attrs),
      });
    }
  }
  return found;
}

describe("every_p0_id_of_the_knowledge_table_appears_in_the_sources", () => {
  const P0 = [
    "knowledge.waiting-note",
    "knowledge.facts",
    "knowledge.proposals",
    "knowledge.relations",
    "knowledge.entities",
    "fact.state",
    "fact.status",
    "fact.actions",
    "fact.notes",
    "entity.state",
    "entity.actions",
    "paths.state",
    "paths.blocked",
    "paths.coverage",
    "paths.list",
    "experience.label",
    "experience.results",
    "experience.summary",
  ];
  const LABELLED = [
    ["P2", "fact.origin"],
    ["P3", "fact.times"],
    ["P2", "fact.history"],
    ["P2", "experience.summary-history"],
    ["P2", "experience.similar"],
  ] as const;

  it("marks each of the eighteen P0 blocks as P0", () => {
    expect(P0).toHaveLength(18);
    const usages = visibleUsages();
    for (const id of P0) {
      expect(
        usages.some((usage) => usage.id === id && usage.category === "P0"),
        `${id} is not a P0 Visible`,
      ).toBe(true);
    }
  });

  it("gives the folded blocks their category and a label", () => {
    const usages = visibleUsages();
    for (const [category, id] of LABELLED) {
      const usage = usages.find((candidate) => candidate.id === id);
      expect(usage?.category, id).toBe(category);
      expect(usage?.hasLabel, `${id} has no label`).toBe(true);
    }
  });
});

describe("no_source_labels_a_path_as_covered", () => {
  it("never writes covered or protected, as a whole word, outside comments and tests", () => {
    const files = [
      ...inFolders(K, E),
      ...Object.entries(sources).filter(([path]) => /^design\/knowledge/.test(path)),
    ];
    expect(files.length).toBeGreaterThan(20);
    for (const [path, text] of files) {
      expect(code(text), path).not.toMatch(/\b(covered|protected)\b/i);
    }
  });
});
