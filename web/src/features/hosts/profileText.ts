import type { ProfileSection } from "../../api/types";

/** One tool's output in a profile, with why it failed when it did. */
export interface ProfilePart {
  /** The fenced output, as Markdown; empty when the note left it out or the tool failed. */
  body: string;
  /** Why the tool did not answer, in the hub's words; `undefined` when it answered. */
  failure?: string | undefined;
  ok: boolean;
  /** The tool's output without the fence around it; empty when it failed. */
  text: string;
  title: string;
}

/** The line that starts the raw tool output in the profile note. */
const DETAILS = "\n## Details";

/** `body` without the opening and closing fence the note puts around tool output. */
function unfence(body: string): string {
  const lines = body.split("\n");
  if (/^`{3,}/.test(lines[0] ?? "")) {
    lines.shift();
    if (/^`{3,}\s*$/.test(lines[lines.length - 1] ?? "")) {
      lines.pop();
    }
  }
  return lines.join("\n").trim();
}

const FAILED = /^The tool did not answer:\s*/;

/** The text of each `### title` block of `details`, ignoring headings inside fenced output. */
function blocks(details: string): Map<string, string> {
  const found = new Map<string, string>();
  let title: string | undefined;
  let lines: string[] = [];
  let fence: string | undefined;
  const close = () => {
    if (title !== undefined) {
      found.set(title, lines.join("\n").trim());
    }
  };
  for (const line of details.split("\n")) {
    const mark = /^(`{3,})/.exec(line)?.[1];
    if (fence === undefined && mark) {
      fence = mark;
    } else if (fence !== undefined && line.trim() === fence) {
      fence = undefined;
    }
    if (fence === undefined && !mark && line.startsWith("### ")) {
      close();
      title = line.slice(4).trim();
      lines = [];
    } else {
      lines.push(line);
    }
  }
  close();
  return found;
}

/**
 * Splits a profile note into the summary a person reads first and one part per tool, in
 * the order the tools ran. A part whose tool failed carries the reason the hub wrote.
 */
export function splitProfile(
  markdown: string,
  sections: readonly ProfileSection[],
): { parts: ProfilePart[]; summary: string } {
  const at = markdown.indexOf(DETAILS);
  const summary = (at < 0 ? markdown : markdown.slice(0, at)).trim();
  const found = blocks(at < 0 ? "" : markdown.slice(at + DETAILS.length));
  const parts = sections.map((section): ProfilePart => {
    const text = found.get(section.title) ?? "";
    return section.ok
      ? { title: section.title, ok: true, body: text, text: unfence(text) }
      : {
          title: section.title,
          ok: false,
          body: "",
          text: "",
          failure: FAILED.test(text) ? text.replace(FAILED, "").trim() : undefined,
        };
  });
  return { summary, parts };
}
