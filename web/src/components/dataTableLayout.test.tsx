import { render, screen, within } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import css from "./data.module.css?raw";
import { DataTable, Grid, type Column } from ".";

interface Row {
  id: string;
  long: string;
  name: string;
}

const columns: Column<Row>[] = [
  { key: "name", label: "Name", width: "8rem", cell: (row) => row.name },
  {
    key: "long",
    label: "Long",
    truncate: "clamp2",
    tip: (row) => row.long,
    cell: (row) => row.long,
  },
  { key: "one", label: "One line", width: "10rem", truncate: "ellipsis", cell: (row) => row.long },
];

const rows: Row[] = [{ id: "a", name: "alpha", long: "a very long value ".repeat(8) }];

function table(layout?: "fixed") {
  return render(
    <MemoryRouter>
      <DataTable
        caption="Things"
        columns={columns}
        rows={rows}
        rowKey={(row) => row.id}
        {...(layout ? { layout } : {})}
      />
    </MemoryRouter>,
  );
}

test("a_fixed_layout_table_takes_its_widths_from_its_columns", () => {
  table("fixed");

  const element = screen.getByRole("table", { name: "Things" });
  expect(element).toHaveAttribute("data-layout", "fixed");
  expect([...element.querySelectorAll("col")].map((col) => col.style.width)).toEqual([
    "8rem",
    "",
    "10rem",
  ]);
});

test("an_automatic_layout_table_has_no_column_widths", () => {
  table();

  const element = screen.getByRole("table", { name: "Things" });
  expect(element).toHaveAttribute("data-layout", "auto");
  expect(element.querySelector("colgroup")).toBeNull();
});

test("truncated_cells_are_cut_by_css_and_keep_their_full_text", () => {
  table("fixed");

  const row = screen.getAllByRole("row")[1] as HTMLElement;
  const cells = within(row).getAllByRole("cell");
  expect(cells[0]?.hasAttribute("data-truncate")).toBe(false);
  expect(cells[1]).toHaveAttribute("data-truncate", "clamp2");
  expect(cells[2]).toHaveAttribute("data-truncate", "ellipsis");
  expect(cells[1], "the whole value stays in the DOM").toHaveTextContent(/^a very long value/);
});

test("a_fill_grid_stretches_its_children_over_the_row", () => {
  const { container } = render(
    <Grid min="xs" fill>
      <span>one</span>
    </Grid>,
  );
  const grid = container.firstElementChild as HTMLElement;
  expect(grid).toHaveAttribute("data-fill", "true");
  expect(grid).toHaveAttribute("data-min", "xs");
});

test("in_card_mode_a_cut_value_is_constrained_so_it_cannot_stretch_the_card", () => {
  // jsdom has no layout: check the stylesheet's rule for the stacked (container query) mode.
  const stacked = css.slice(css.indexOf("@container"));
  expect(stacked).toMatch(/td\[data-truncate\]\s*\{[^}]*min-width: 0;[^}]*max-width: 100%/);
  expect(stacked).toMatch(
    /td > :is\(\.ellipsis, \.clamp2\)\s*\{[^}]*min-width: 0;[^}]*max-width: 100%/,
  );
  table("fixed");
  expect(screen.getAllByRole("cell")[1]).toHaveAttribute("data-truncate", "clamp2");
});
