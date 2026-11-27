import { act, renderHook } from "@testing-library/react";
import { useRememberedRefusal } from "./rememberedRefusal";

beforeEach(() => window.localStorage.clear());

test("a_refusal_survives_a_reload_until_it_is_cleared", () => {
  const first = renderHook(({ error }) => useRememberedRefusal("fact-1", error), {
    initialProps: {
      error: { title: "Could not confirm", detail: "Add the network to management_cidrs." },
    },
  });
  expect(first.result.current.shown?.detail).toBe("Add the network to management_cidrs.");
  first.unmount();

  // A new page load: no live error, the kept one is still there.
  const second = renderHook(() => useRememberedRefusal("fact-1", undefined));
  expect(second.result.current.shown?.title).toBe("Could not confirm");
  // Another fact does not see it.
  const other = renderHook(() => useRememberedRefusal("fact-2", undefined));
  expect(other.result.current.shown).toBeUndefined();

  act(() => second.result.current.clear());
  expect(second.result.current.shown).toBeUndefined();
  const third = renderHook(() => useRememberedRefusal("fact-1", undefined));
  expect(third.result.current.shown).toBeUndefined();
});
