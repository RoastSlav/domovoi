import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { DataTable, type Column } from "./DataTable";

interface Row {
  id: string;
  name: string;
}

const rows: Row[] = Array.from({ length: 200 }, (_, index) => ({
  id: `row-${index}`,
  name: `Incident ${index}`,
}));

describe("DataTable rows", () => {
  it("a live update that changes one row redraws one row", () => {
    const drawn: string[] = [];
    const columns: Column<Row>[] = [
      {
        key: "name",
        label: "Name",
        cell: (row) => {
          drawn.push(row.id);
          return row.name;
        },
      },
    ];
    const table = (data: Row[]) => (
      <MemoryRouter>
        <DataTable caption="Incidents" columns={columns} rows={data} rowKey={(row) => row.id} />
      </MemoryRouter>
    );
    const { rerender } = render(table(rows));
    expect(drawn).toHaveLength(200);

    drawn.length = 0;
    const changed = rows.map((row) => (row.id === "row-7" ? { ...row, name: "Renamed" } : row));
    rerender(table(changed));

    expect(screen.getByText("Renamed")).toBeInTheDocument();
    expect(drawn).toEqual(["row-7"]);
  });

  it("redraws every row when the columns change", () => {
    const drawn: string[] = [];
    const make = (): Column<Row>[] => [
      {
        key: "name",
        label: "Name",
        cell: (row) => {
          drawn.push(row.id);
          return row.name;
        },
      },
    ];
    const table = (columns: Column<Row>[]) => (
      <MemoryRouter>
        <DataTable
          caption="Incidents"
          columns={columns}
          rows={rows.slice(0, 5)}
          rowKey={(row) => row.id}
        />
      </MemoryRouter>
    );
    const { rerender } = render(table(make()));
    drawn.length = 0;
    rerender(table(make()));
    expect(drawn).toHaveLength(5);
  });
});
