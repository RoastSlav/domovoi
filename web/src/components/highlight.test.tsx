import { render } from "@testing-library/react";
import { Highlight } from "./Highlight";

function marked(container: HTMLElement): (string | null)[] {
  return [...container.querySelectorAll("mark")].map((mark) => mark.textContent);
}

test("highlight_marks_every_word_of_the_query_ignoring_case", () => {
  const { container } = render(
    <p>
      <Highlight text="Scrub the ZFS pool, then check zfs status" query="zfs pool" />
    </p>,
  );
  expect(marked(container)).toEqual(["ZFS", "pool", "zfs"]);
  expect(container.querySelector("p")).toHaveTextContent(
    "Scrub the ZFS pool, then check zfs status",
  );
});

test("highlight_treats_the_query_as_plain_text_not_a_pattern", () => {
  const { container } = render(
    <p>
      <Highlight text="cost (a+b) and axb" query="(a+b)" />
    </p>,
  );
  expect(marked(container)).toEqual(["(a+b)"]);
});

test("highlight_leaves_text_alone_without_a_query", () => {
  const { container } = render(
    <p>
      <Highlight text="nothing to mark" query="   " />
    </p>,
  );
  expect(container.querySelector("mark")).toBeNull();
  expect(container).toHaveTextContent("nothing to mark");
});
