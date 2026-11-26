/** Plain words for the audit page: why a chain broke, what a kind means, what a check covered. */

const BREAK_REASONS: Readonly<Record<string, string>> = {
  missing_chain_fields: "An entry has no hash chain fields.",
  hash_mismatch: "An entry was edited.",
  link_mismatch: "An entry was removed or inserted.",
  sequence_gap: "Entries are missing.",
  checkpoint_mismatch: "The retained start does not match the checkpoint.",
};

/** The sentence for a break reason code; an unknown code uses the hub's own text, else the code. */
export function breakReasonText(code: string, fallback: string | null): string {
  return Object.hasOwn(BREAK_REASONS, code) ? (BREAK_REASONS[code] as string) : (fallback ?? code);
}

const KIND_HINTS: Readonly<Record<string, string>> = {
  "audit.chain_broken": "The hub found the chain broken when it started.",
  "audit.checkpoint": "Older entries were removed here. The check starts from this entry.",
};

/** What an audit kind means, when it needs saying. */
export function auditKindHint(kind: string): string | undefined {
  return Object.hasOwn(KIND_HINTS, kind) ? KIND_HINTS[kind] : undefined;
}

/** What a check covered. */
export function scopeSentence(scope: string, recentLimit: number): string {
  if (scope === "recent") {
    return `Checked the newest ${recentLimit.toLocaleString("en-US")} entries.`;
  }
  if (scope === "full") {
    return "Checked every entry.";
  }
  return "Checked.";
}
