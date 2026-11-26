import css from "./state.module.css?raw";

test("a_link_inside_an_empty_state_is_underlined_so_it_is_not_told_apart_by_colour_alone", () => {
  expect(css).toMatch(/\.emptyBody a\s*\{[^}]*text-decoration:\s*underline/);
});
