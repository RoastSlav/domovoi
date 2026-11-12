import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router";
import {
  Checkbox,
  DataTable,
  Field,
  Menu,
  Section,
  Select,
  StatTile,
  StatusPill,
  SubmitBar,
  Switch,
  Tag,
  type Column,
} from ".";
import { verdictMeta } from "../design/tone";

test("stat_tile_shows_label_value_and_hint_and_is_not_a_link_without_a_destination", () => {
  render(
    <MemoryRouter>
      <StatTile label="Open incidents" value={4} hint="2 need a person" tone="danger" />
    </MemoryRouter>,
  );
  expect(screen.getByText("Open incidents")).toBeVisible();
  expect(screen.getByText("4")).toBeVisible();
  expect(screen.getByText("2 need a person")).toBeVisible();
  expect(screen.queryByRole("link")).toBeNull();
});

test("stat_tile_with_a_destination_is_one_link_naming_what_it_counts", () => {
  render(
    <MemoryRouter>
      <StatTile label="Waiting for approval" value="3" to="/approvals" />
    </MemoryRouter>,
  );
  const link = screen.getByRole("link", { name: /Waiting for approval/ });
  expect(link).toHaveAttribute("href", "/approvals");
  expect(link).toHaveTextContent("3");
});

test("switch_puts_its_badges_beside_the_label_and_not_inside_the_hint", () => {
  render(
    <Switch
      label="Enable the check"
      hint="Runs every minute."
      checked
      onChange={() => undefined}
      badges={<Tag>hub.toml</Tag>}
    />,
  );
  const badge = screen.getByText("hub.toml");
  const label = screen.getByText("Enable the check");
  expect(label.parentElement, "same line as the label").toContainElement(badge);
  expect(screen.getByText("Runs every minute.")).not.toContainElement(badge);
  expect(screen.getByRole("switch")).toBeChecked();
});

test("checkbox_shows_its_badges_beside_the_label_and_still_toggles", async () => {
  const user = userEvent.setup();
  const onChange = vi.fn();
  render(
    <Checkbox
      label="Include logins"
      checked={false}
      onChange={onChange}
      badges={<Tag>default</Tag>}
    />,
  );
  expect(screen.getByText("Include logins").parentElement).toContainElement(
    screen.getByText("default"),
  );
  await user.click(screen.getByRole("checkbox"));
  expect(onChange).toHaveBeenCalledExactlyOnceWith(true);
});

test("submit_bar_renders_nothing_when_nothing_changed", () => {
  const { container } = render(<SubmitBar changes={0} onDiscard={vi.fn()} onSave={vi.fn()} />);
  expect(container).toBeEmptyDOMElement();
});

test("submit_bar_counts_unsaved_changes_and_holds_a_note_slot", async () => {
  const user = userEvent.setup();
  const onSave = vi.fn();
  const onDiscard = vi.fn();
  render(
    <SubmitBar changes={3} onDiscard={onDiscard} onSave={onSave}>
      <Tag>Restarts the hub</Tag>
    </SubmitBar>,
  );
  expect(screen.getByRole("status")).toHaveTextContent("3 unsaved changes");
  expect(screen.getByText("Restarts the hub")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Save" }));
  await user.click(screen.getByRole("button", { name: "Discard" }));
  expect(onSave).toHaveBeenCalledOnce();
  expect(onDiscard).toHaveBeenCalledOnce();
});

test("submit_bar_always_show_keeps_a_clean_bar_with_save_and_discard_explaining_why_not", () => {
  render(<SubmitBar changes={0} alwaysShow onDiscard={vi.fn()} onSave={vi.fn()} />);
  expect(screen.getByRole("status")).toHaveTextContent("No unsaved changes");
  expect(screen.getByRole("button", { name: "Save" })).toHaveAccessibleDescription(
    "Nothing has changed.",
  );
});

test("status_pill_is_a_dot_and_a_label_with_its_meaning_as_the_description", () => {
  const meta = verdictMeta("ACTION_NEEDED");
  const { container } = render(<StatusPill meta={meta} />);
  expect(screen.getByText(meta.label)).toBeVisible();
  expect(container.querySelector("svg"), "no icon: a dot").toBeNull();
  expect(screen.getByText(meta.label)).toHaveAccessibleDescription(meta.hint);
});

test("select_keeps_its_native_combobox_and_value_with_a_chevron_beside_it", async () => {
  const user = userEvent.setup();
  render(
    <Field label="Mute for">
      <Select defaultValue="60">
        <option value="60">1 hour</option>
        <option value="1440">1 day</option>
      </Select>
    </Field>,
  );
  const select = screen.getByRole("combobox", { name: "Mute for" });
  await user.selectOptions(select, "1440");
  expect(select).toHaveValue("1440");
});

test("menu_item_shows_a_shortcut_hint_and_a_check_for_the_chosen_entry", async () => {
  const user = userEvent.setup();
  render(
    <Menu
      label="Go"
      items={[
        { label: "Incidents", shortcut: "G I", checked: true, onSelect: vi.fn() },
        { label: "Hosts", shortcut: "G H", checked: false, onSelect: vi.fn() },
      ]}
    />,
  );
  await user.click(screen.getByRole("button", { name: "Go" }));
  const chosen = screen.getByRole("menuitemradio", { name: /Incidents/ });
  expect(chosen).toHaveTextContent("G I");
  expect(chosen).toHaveAttribute("aria-checked", "true");
  expect(within(chosen).getByText("G I")).toBeVisible();
  expect(screen.getByRole("menuitemradio", { name: /Hosts/ })).toHaveTextContent("G H");
});

interface Row {
  name: string;
}
const COLUMNS: readonly Column<Row>[] = [
  { key: "name", label: "Host", cell: (row) => <a href={`#${row.name}`}>{row.name}</a> },
  { key: "note", label: "Note", cell: () => "all good" },
  {
    key: "act",
    label: "Actions",
    hideLabel: true,
    cell: (row) => (
      <button type="button" aria-label={`Edit ${row.name}`}>
        edit
      </button>
    ),
  },
];

test("data_table_forwards_a_click_anywhere_on_the_row_but_not_on_a_control_inside_it", async () => {
  const user = userEvent.setup();
  const onRowClick = vi.fn();
  render(
    <DataTable
      caption="Hosts"
      columns={COLUMNS}
      rows={[{ name: "nas-01" }]}
      rowKey={(row) => row.name}
      onRowClick={onRowClick}
    />,
  );
  const row = screen.getByRole("row", { name: /nas-01/ });
  expect(row).toHaveAttribute("data-clickable");
  await user.click(screen.getByText("all good"));
  expect(onRowClick).toHaveBeenCalledExactlyOnceWith({ name: "nas-01" });

  await user.click(screen.getByRole("button", { name: "Edit nas-01" }));
  await user.click(screen.getByRole("link", { name: "nas-01" }));
  expect(onRowClick, "controls keep their own clicks").toHaveBeenCalledOnce();
});

test("data_table_row_href_makes_the_first_cell_the_link_covering_the_row", () => {
  render(
    <MemoryRouter>
      <DataTable
        caption="Hosts"
        columns={[{ key: "name", label: "Host", cell: (row: Row) => row.name }]}
        rows={[{ name: "nas-01" }]}
        rowKey={(row) => row.name}
        rowHref={(row) => `/hosts/${row.name}`}
      />
    </MemoryRouter>,
  );
  expect(screen.getByRole("link", { name: "nas-01" })).toHaveAttribute("href", "/hosts/nas-01");
  expect(screen.getByRole("row", { name: /nas-01/ })).toHaveAttribute("data-clickable");
});

test("data_table_marks_numeric_and_action_columns_for_alignment", () => {
  render(
    <DataTable
      caption="Hosts"
      columns={[
        { key: "name", label: "Host", cell: (row: Row) => row.name },
        { key: "n", label: "Open", numeric: true, cell: () => 3 },
        { key: "a", label: "Actions", hideLabel: true, cell: () => "…" },
      ]}
      rows={[{ name: "nas-01" }]}
      rowKey={(row) => row.name}
    />,
  );
  expect(screen.getByRole("columnheader", { name: "Open" })).toHaveAttribute("data-numeric");
  expect(screen.getByRole("columnheader", { name: "Actions" })).toHaveAttribute("data-actions");
  expect(screen.getByRole("cell", { name: "3" })).toHaveAttribute("data-numeric");
});

test("section_puts_its_actions_on_the_title_row_and_the_description_under_both", () => {
  render(
    <Section title="Recent" description="The last five." actions={<a href="#all">View all</a>}>
      <p>content</p>
    </Section>,
  );
  const title = screen.getByRole("heading", { name: "Recent" });
  const row = title.parentElement;
  expect(row).toContainElement(screen.getByRole("link", { name: "View all" }));
  expect(row, "the description is not in the title row").not.toContainElement(
    screen.getByText("The last five."),
  );
});
