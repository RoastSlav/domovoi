import { slotLayout } from "./layout";

const layout = (available: number, open: string[], wide = true) =>
  slotLayout({ available, wide, open });

test("slot_layout_is_closed_with_nothing_open", () => {
  expect(layout(2000, []).mode).toBe("closed");
});

test("slot_layout_puts_one_surface_beside_the_page_when_640px_remain", () => {
  expect(layout(1040, ["a"])).toEqual({ mode: "side", visible: ["a"], tabs: [] });
  expect(layout(1039, ["a"]).mode).toBe("sheet");
});

test("slot_layout_shows_two_surfaces_only_when_640px_still_remain", () => {
  expect(layout(1440, ["a", "b"])).toEqual({ mode: "side", visible: ["a", "b"], tabs: [] });
  expect(layout(1439, ["a", "b"])).toEqual({ mode: "side", visible: ["b"], tabs: ["a", "b"] });
});

test("slot_layout_is_a_sheet_when_even_one_does_not_fit_and_always_on_a_phone", () => {
  expect(layout(900, ["a", "b"])).toEqual({ mode: "sheet", visible: ["b"], tabs: ["a", "b"] });
  expect(layout(5000, ["a"], false)).toEqual({ mode: "sheet", visible: ["a"], tabs: [] });
  expect(layout(5000, ["a", "b"], false).tabs).toEqual(["a", "b"]);
});
