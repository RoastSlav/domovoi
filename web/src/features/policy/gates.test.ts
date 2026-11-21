import { describe, expect, it } from "vitest";
import { POLICY_VOCABULARIES } from "../../design/policyMeta";

const modules = import.meta.glob("../../**/*.{ts,tsx}", {
  query: "?raw",
  eager: true,
  import: "default",
}) as Record<string, string>;

/** `../../x/y.ts` as seen from this folder, as a path from `src/`. */
function fromSrc(key: string): string {
  const parts = ["features", "policy"];
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

/** Strips comments, so a doc comment may talk about what the code must not do. */
function code(text: string): string {
  return text.replace(/\/\*[\s\S]*?\*\//g, "").replace(/(^|[^:])\/\/.*$/gm, "$1");
}

const IDENT = /[A-Za-z0-9_]/;

/** Whether `word` occurs in `text` as a whole identifier. */
function mentions(text: string, word: string): boolean {
  let at = text.indexOf(word);
  while (at !== -1) {
    const before = text.charAt(at - 1);
    const after = text.charAt(at + word.length);
    if (!IDENT.test(before) && !IDENT.test(after)) return true;
    at = text.indexOf(word, at + 1);
  }
  return false;
}

const policyFeature = Object.entries(sources).filter(([path]) =>
  path.startsWith("features/policy/"),
);
const named = (file: string) => policyFeature.filter(([path]) => path.split("/").pop() === file);

/** A whole `<Visible category="P0" id="...">` opening, tolerant of line breaks. */
function declaresP0(text: string, id: string): boolean {
  const open = /<Visible\s+category="P0"\s+id="([^"]+)"/g;
  for (const match of text.matchAll(open)) {
    if (match[1] === id) return true;
  }
  return false;
}

describe("policy gates", () => {
  it("the_gate_reads_the_policy_sources", () => {
    expect(policyFeature.length).toBeGreaterThan(30);
    expect(Object.keys(sources)).toContain("api/hooks/policy.ts");
    // The word matcher really matches, so the checks below cannot pass by matching nothing.
    expect(mentions("a Visible b", "Visible")).toBe(true);
    expect(mentions("InvisibleThing", "Visible")).toBe(false);
  });

  it("visibility_never_gates_policy_publication_code", () => {
    const words = [
      "useVisibility",
      "useProfile",
      "Visible",
      "presentationFor",
      "DetailsScope",
      "visibilityAttrs",
    ];
    for (const file of [
      "PublishControls.tsx",
      "LockedChangeDialog.tsx",
      "useSaveDraftFlow.tsx",
      "StartDraft.tsx",
    ]) {
      const found = named(file);
      expect(found, `${file} exists`).toHaveLength(1);
      for (const [path, text] of found) {
        const body = code(text);
        expect(body.length, `${path} is not empty`).toBeGreaterThan(200);
        for (const word of words) {
          expect(mentions(body, word), `${path} mentions ${word}`).toBe(false);
        }
      }
    }
  });

  it("policy_pages_offer_no_approval_control", () => {
    const banned = [
      "useApprove",
      "useApproveStep",
      "useDeny",
      "useDenyStep",
      "ApprovalCard",
      "ApproveControl",
      "StepApproveControl",
    ];
    for (const [path, text] of policyFeature) {
      const body = code(text);
      for (const word of banned) {
        expect(mentions(body, word), `${path} uses ${word}`).toBe(false);
      }
    }
  });

  it("the_password_is_never_stored_or_logged", () => {
    // The gate bites only if the password really is handled in the feature.
    const handling = policyFeature
      .filter(([, text]) => mentions(code(text), "password"))
      .map(([path]) => path);
    expect(handling).toContain("features/policy/PublishControls.tsx");
    for (const [path, text] of policyFeature) {
      const body = code(text);
      for (const word of ["localStorage", "sessionStorage", "console."]) {
        expect(body.includes(word), `${path} mentions ${word}`).toBe(false);
      }
      // A query key or an address built from a password would put it in the cache or the URL.
      expect(/queryKey[^;]*password/i.test(body), `${path} puts password in a query key`).toBe(
        false,
      );
      expect(/keys\.[a-zA-Z]+\([^)]*password/i.test(body), `${path} keys on a password`).toBe(
        false,
      );
      expect(/\$\{[^}]*password[^}]*\}/i.test(body), `${path} interpolates a password`).toBe(false);
      expect(/[?&]password=/i.test(body), `${path} has a password in an address`).toBe(false);
    }
  });

  it("policy_words_come_from_the_vocabulary", () => {
    const wanted = new Set([
      "Policy outcome",
      "Rule mark",
      "Decision reason",
      "Hard limit",
      "Policy finding",
      "Lock change",
    ]);
    const labels = new Map<string, string>();
    for (const vocab of POLICY_VOCABULARIES) {
      if (!wanted.has(vocab.name)) continue;
      for (const value of vocab.values) {
        labels.set(vocab.meta(value).label, `${vocab.name} ${value}`);
      }
    }
    expect(labels.size).toBeGreaterThan(20);
    const retyped: string[] = [];
    for (const [path, text] of policyFeature) {
      const body = code(text);
      const found: string[] = [];
      for (const match of body.matchAll(
        /"((?:[^"\\\n]|\\.)*)"|'((?:[^'\\\n]|\\.)*)'|`([^`$]*)`/g,
      )) {
        found.push(match[1] ?? match[2] ?? match[3] ?? "");
      }
      for (const match of body.matchAll(/>([^<>{}\n]+)</g)) {
        found.push((match[1] ?? "").trim());
      }
      // JSX text on a line of its own.
      for (const match of body.matchAll(/^\s*([A-Z][^<>{}=;()\n]*?)\s*$/gm)) {
        found.push(match[1] ?? "");
      }
      for (const literal of found) {
        const origin = labels.get(literal);
        if (origin) retyped.push(`${path}: "${literal}" is ${origin}`);
      }
    }
    expect(retyped).toEqual([]);
  });

  it("the_policy_hooks_are_the_only_policy_network_path", () => {
    // The generated API types name every route; they are not a way to reach one.
    const generated = new Set(["api/schema.d.ts"]);
    const users = Object.keys(sources).filter(
      (path) => !generated.has(path) && sources[path]?.includes("/api/v1/policy"),
    );
    expect(users).toContain("api/hooks/policy.ts");
    for (const path of users) {
      expect(/^api\/hooks\/policy[A-Za-z]*\.ts$/.test(path), `${path} reaches /api/v1/policy`).toBe(
        true,
      );
    }
  });

  it("every_policy_p0_id_of_the_table_appears_in_the_sources", () => {
    const ids = [
      "policy.state",
      "policy.mode",
      "policy.shadow",
      "policy.active",
      "policy.rules",
      "policy.draft-stale",
      "policy.decision-rows",
      "version.status",
      "version.acknowledgements",
      "draft.banner",
      "draft.rules",
      "draft.validation",
      "publish.validation",
      "publish.findings",
      "publish.locks",
      "publish.widening",
      "publish.controls",
      "decision.state",
      "decision.outcome",
      "decision.locked",
      "decision.exception",
      "decision.why",
      "decision.limits",
      "decision.gaps",
      "simulator.state",
      "simulator.basis",
      "simulator.widening",
    ];
    for (const id of ids) {
      const hit = policyFeature.some(([, text]) => declaresP0(code(text), id));
      expect(hit, `${id} is declared as P0 in a policy source`).toBe(true);
    }
  });
});
