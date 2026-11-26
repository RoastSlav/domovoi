import { breakReasonText, scopeSentence, auditKindHint } from "./auditText";
import { chainStatusMeta } from "./tone";

test("break_reasons_have_plain_words_and_unknown_ones_fall_back", () => {
  expect(breakReasonText("hash_mismatch", null)).toBe("An entry was edited.");
  expect(breakReasonText("sequence_gap", "x")).toBe("Entries are missing.");
  expect(breakReasonText("from_the_future", "the hub's words")).toBe("the hub's words");
  expect(breakReasonText("from_the_future", null)).toBe("from_the_future");
});

test("scope_sentences_say_what_was_checked", () => {
  expect(scopeSentence("recent", 10000)).toContain("newest 10,000 entries");
  expect(scopeSentence("full", 10000)).toContain("every entry");
  expect(scopeSentence("other", 1)).toBe("Checked.");
  expect(auditKindHint("audit.checkpoint")).toContain("Older entries were removed");
  expect(auditKindHint("x")).toBeUndefined();
});

test("unknown_audit_statuses_are_neutral_with_the_raw_value", () => {
  for (const meta of [chainStatusMeta("from_the_future")]) {
    expect(meta.tone).toBe("neutral");
    expect(meta.label).toBe("from_the_future");
  }
});

test("only_intact_is_green_and_failures_are_danger", () => {
  expect(chainStatusMeta("intact").tone).toBe("success");
  expect(chainStatusMeta("broken").tone).toBe("danger");
});

test("audit_words_never_overclaim", () => {
  const words = [
    ...["empty", "intact", "broken"].map((v) => chainStatusMeta(v).hint),
    breakReasonText("hash_mismatch", null),
    scopeSentence("recent", 10),
  ].join(" ");
  expect(words).not.toMatch(/tamper-proof|secure|guaranteed|!/i);
});
