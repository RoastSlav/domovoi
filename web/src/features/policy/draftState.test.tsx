import { act, renderHook } from "@testing-library/react";
import type { PolicyDraft, PolicyRule } from "../../api/types";
import { DRAFT_4 } from "../../test/policyFixtures";
import { useDraftState } from "./useDraftState";

const EXTERNAL: PolicyDraft = { ...DRAFT_4, revision: 5, updated_by: "other" };

function edited(): PolicyRule {
  const rule = DRAFT_4.rules[0];
  if (!rule) throw new Error("fixture has no rules");
  return { ...rule, note: "edited" };
}

function mount(first: PolicyDraft = DRAFT_4) {
  return renderHook(({ fetched }) => useDraftState(fetched), {
    initialProps: { fetched: first },
  });
}

test("an_external_update_while_clean_is_adopted", () => {
  const hook = mount();
  hook.rerender({ fetched: EXTERNAL });
  expect(hook.result.current.saved.revision).toBe(5);
  expect(hook.result.current.staleRevision).toBe(false);
  expect(hook.result.current.dirty).toBe(false);
});

test("an_external_update_while_dirty_keeps_the_edits_and_flags_the_revision", () => {
  const hook = mount();
  const rule = edited();
  act(() => hook.result.current.apply(rule, rule.id));
  hook.rerender({ fetched: EXTERNAL });
  expect(hook.result.current.dirty).toBe(true);
  expect(hook.result.current.staleRevision).toBe(true);
  expect(hook.result.current.saved.revision).toBe(4);
  expect(hook.result.current.working.find((r) => r.id === rule.id)?.note).toBe("edited");
});

test("a_save_followed_by_a_refetch_does_not_flag_a_stale_revision", () => {
  const hook = mount();
  const rule = edited();
  act(() => hook.result.current.apply(rule, rule.id));
  const stored: PolicyDraft = { ...DRAFT_4, revision: 5, rules: hook.result.current.working };
  act(() => hook.result.current.adopt(stored));
  hook.rerender({ fetched: stored });
  expect(hook.result.current.staleRevision).toBe(false);
  expect(hook.result.current.dirty).toBe(false);
});

test("reloading_after_an_external_update_replaces_the_working_copy", () => {
  const hook = mount();
  const rule = edited();
  act(() => hook.result.current.apply(rule, rule.id));
  hook.rerender({ fetched: EXTERNAL });
  act(() => hook.result.current.reload());
  expect(hook.result.current.saved.revision).toBe(5);
  expect(hook.result.current.working).toBe(EXTERNAL.rules);
  expect(hook.result.current.dirty).toBe(false);
  expect(hook.result.current.staleRevision).toBe(false);
});

test("a_failed_save_then_another_edit_keeps_the_copy_dirty", () => {
  const hook = mount();
  const rule = edited();
  act(() => hook.result.current.apply(rule, rule.id));
  // A failed save adopts nothing; the next edit builds on the same working copy.
  act(() => hook.result.current.apply({ ...rule, note: "again" }, rule.id));
  expect(hook.result.current.dirty).toBe(true);
  expect(hook.result.current.saved.revision).toBe(4);
  expect(hook.result.current.working.find((r) => r.id === rule.id)?.note).toBe("again");
});
