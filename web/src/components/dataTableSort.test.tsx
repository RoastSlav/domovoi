import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { DataTable } from ".";

const COLUMNS = [
  { key: "name", label: "Name", sortable: true, cell: (row: { name: string }) => row.name },
  { key: "note", label: "Note", cell: (row: { name: string }) => `note ${row.name}` },
];

test("a_sortable_header_reports_its_key_and_says_which_way_the_table_is_sorted", async () => {
  const user = userEvent.setup();
  const onSort = vi.fn();
  render(
    <DataTable
      caption="Things"
      columns={COLUMNS}
      rows={[{ name: "a" }]}
      rowKey={(row) => row.name}
      sort={{ key: "name", direction: "descending" }}
      onSort={onSort}
    />,
  );

  expect(screen.getByRole("columnheader", { name: "Name" })).toHaveAttribute(
    "aria-sort",
    "descending",
  );
  expect(
    screen.getByRole("columnheader", { name: "Note" }),
    "a column that cannot be sorted says nothing about sorting",
  ).not.toHaveAttribute("aria-sort");
  expect(screen.queryByRole("button", { name: "Note" })).toBeNull();

  await user.click(screen.getByRole("button", { name: "Name" }));
  expect(onSort).toHaveBeenCalledWith("name");
});

test("a_sortable_column_that_is_not_the_sort_reads_as_unsorted", () => {
  render(
    <DataTable
      caption="Things"
      columns={COLUMNS}
      rows={[{ name: "a" }]}
      rowKey={(row) => row.name}
      sort={{ key: "other", direction: "ascending" }}
      onSort={() => undefined}
    />,
  );
  expect(screen.getByRole("columnheader", { name: "Name" })).toHaveAttribute("aria-sort", "none");
});
