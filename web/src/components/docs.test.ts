import { readFileSync, readdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

/** The book's sources, relative to this file in `web/src/components`. */
const BOOK = resolve(dirname(fileURLToPath(import.meta.url)), "../../../docs/book/src");

/** The UI sources to scan, `web/src`. */
const UI_SRC = resolve(dirname(fileURLToPath(import.meta.url)), "..");

/** Every `docs="..."` value in the UI sources, in file order. */
function docsPaths(): string[] {
  const found: string[] = [];
  for (const entry of readdirSync(UI_SRC, { recursive: true })) {
    const name = String(entry);
    if (!name.endsWith(".tsx")) {
      continue;
    }
    const text = readFileSync(resolve(UI_SRC, name), "utf8");
    for (const match of text.matchAll(/docs="([^"]+)"/g)) {
      found.push(match[1] ?? "");
    }
  }
  return found;
}

/** mdBook's `normalize_id` over a heading's text. */
function headingId(title: string): string {
  let out = "";
  for (const c of title) {
    if (/[\p{L}\p{N}]/u.test(c)) {
      out += c.toLowerCase();
    } else if (/\s/u.test(c)) {
      out += "-";
    }
  }
  return out;
}

/** The ids mdBook gives the headings in a page, fenced code skipped. */
function headingIds(page: string): Set<string> {
  const ids = new Set<string>();
  const seen = new Map<string, number>();
  let fenced = false;
  for (const line of page.split("\n")) {
    if (line.trimStart().startsWith("```")) {
      fenced = !fenced;
      continue;
    }
    if (fenced || !line.startsWith("#")) {
      continue;
    }
    const id = headingId(line.replace(/^#+/, "").trim());
    const count = seen.get(id) ?? 0;
    ids.add(count === 0 ? id : `${id}-${count}`);
    seen.set(id, count + 1);
  }
  return ids;
}

test("page_intro_doc_links_exist_in_the_book", () => {
  const summary = readFileSync(resolve(BOOK, "SUMMARY.md"), "utf8");
  const paths = docsPaths();
  expect(paths.length, "the pages have docs links to check").toBeGreaterThan(0);
  for (const path of paths) {
    const hash = path.indexOf("#");
    const page = hash === -1 ? path : path.slice(0, hash);
    const anchor = hash === -1 ? null : path.slice(hash + 1);
    const md = page.replace(/\.html$/, ".md");
    expect(summary, `${md} is not in SUMMARY.md`).toContain(`](${md})`);
    const ids = headingIds(readFileSync(resolve(BOOK, md), "utf8"));
    if (anchor !== null) {
      expect([...ids], `${path} names no heading in ${md}`).toContain(anchor);
    }
  }
});
