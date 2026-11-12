import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { CommandPalette, type PaletteGroup } from ".";
import { filterGroups } from "./palette";

function groups(onSelect: (id: string) => void): PaletteGroup[] {
  return [
    {
      title: "Pages",
      items: [
        { id: "overview", label: "Overview", onSelect: () => onSelect("overview") },
        {
          id: "incidents",
          label: "Incidents",
          keywords: ["problems"],
          onSelect: () => onSelect("incidents"),
        },
      ],
    },
    {
      title: "Hosts",
      items: [
        { id: "nas-01", label: "nas-01", hint: "Connected", onSelect: () => onSelect("nas-01") },
      ],
    },
    {
      title: "Actions",
      items: [
        {
          id: "theme",
          label: "Toggle theme",
          keywords: ["dark", "light"],
          onSelect: () => onSelect("theme"),
        },
      ],
    },
  ];
}

function Harness({ onSelect, start = true }: { onSelect: (id: string) => void; start?: boolean }) {
  const [open, setOpen] = useState(start);
  return (
    <>
      <button type="button" onClick={() => setOpen(true)}>
        opener
      </button>
      <CommandPalette open={open} onClose={() => setOpen(false)} groups={groups(onSelect)} />
    </>
  );
}

const search = () => screen.findByRole("combobox", { name: "Search" });

test("command_palette_draws_nothing_while_closed", () => {
  render(<Harness onSelect={vi.fn()} start={false} />);
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(screen.queryByRole("combobox")).toBeNull();
});

test("command_palette_opens_as_a_dialog_with_a_search_combobox_and_grouped_options", async () => {
  render(<Harness onSelect={vi.fn()} />);
  const box = await search();
  expect(screen.getByRole("dialog", { name: "Command palette" })).toHaveAttribute(
    "aria-modal",
    "true",
  );
  expect(box).toHaveFocus();
  const list = screen.getByRole("listbox", { name: "Results" });
  expect(box).toHaveAttribute("aria-controls", list.id);
  expect(box).toHaveAttribute("aria-expanded", "true");
  expect(within(screen.getByRole("group", { name: "Pages" })).getAllByRole("option")).toHaveLength(
    2,
  );
  expect(screen.getByRole("group", { name: "Hosts" })).toBeVisible();
  expect(screen.getByRole("group", { name: "Actions" })).toBeVisible();
  expect(screen.getByText("Connected"), "the item's hint is shown").toBeVisible();
  expect(screen.getAllByRole("option")[0], "the first is active").toHaveAttribute(
    "aria-selected",
    "true",
  );
});

test("command_palette_filters_by_label_and_keywords_and_says_when_nothing_matches", async () => {
  const user = userEvent.setup();
  render(<Harness onSelect={vi.fn()} />);
  await user.type(await search(), "dark");
  expect(screen.getAllByRole("option")).toHaveLength(1);
  expect(screen.getByRole("option", { name: "Toggle theme" })).toBeVisible();
  expect(screen.queryByRole("group", { name: "Pages" }), "empty groups are dropped").toBeNull();

  await user.clear(await search());
  await user.type(await search(), "zzz");
  expect(screen.getByRole("status")).toHaveTextContent("Nothing matches");
  expect(screen.queryByRole("listbox")).toBeNull();
  expect(await search()).toHaveAttribute("aria-expanded", "false");
});

test("command_palette_arrow_keys_home_and_end_move_the_active_option_and_wrap", async () => {
  const user = userEvent.setup();
  render(<Harness onSelect={vi.fn()} />);
  const box = await search();
  const active = () => box.getAttribute("aria-activedescendant");
  const options = screen.getAllByRole("option");
  expect(active()).toBe(options[0]?.id);

  await user.keyboard("{ArrowDown}");
  expect(active()).toBe(options[1]?.id);
  await user.keyboard("{End}");
  expect(active()).toBe(options.at(-1)?.id);
  await user.keyboard("{ArrowDown}");
  expect(active(), "wraps to the first").toBe(options[0]?.id);
  await user.keyboard("{ArrowUp}");
  expect(active(), "and back to the last").toBe(options.at(-1)?.id);
  await user.keyboard("{Home}");
  expect(active()).toBe(options[0]?.id);
  expect(options[0]).toHaveAttribute("aria-selected", "true");
  expect(options[1]).toHaveAttribute("aria-selected", "false");
});

test("command_palette_enter_closes_it_and_runs_the_active_item", async () => {
  const user = userEvent.setup();
  const onSelect = vi.fn();
  render(<Harness onSelect={onSelect} start={false} />);
  await user.click(screen.getByRole("button", { name: "opener" }));
  await search();
  await user.keyboard("{ArrowDown}{Enter}");
  expect(onSelect).toHaveBeenCalledExactlyOnceWith("incidents");
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(screen.getByRole("button", { name: "opener" }), "focus goes back").toHaveFocus();
});

test("command_palette_a_click_on_a_result_runs_it", async () => {
  const user = userEvent.setup();
  const onSelect = vi.fn();
  render(<Harness onSelect={onSelect} />);
  await search();
  await user.click(screen.getByRole("option", { name: "nas-01 Connected" }));
  expect(onSelect).toHaveBeenCalledExactlyOnceWith("nas-01");
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("command_palette_escape_closes_it_without_running_anything", async () => {
  const user = userEvent.setup();
  const onSelect = vi.fn();
  render(<Harness onSelect={onSelect} />);
  await search();
  await user.keyboard("{Escape}");
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(onSelect).not.toHaveBeenCalled();
});

test("command_palette_keeps_focus_in_the_search_box_when_tab_is_pressed", async () => {
  const user = userEvent.setup();
  render(<Harness onSelect={vi.fn()} />);
  const box = await search();
  await user.tab();
  expect(box).toHaveFocus();
  await user.tab({ shift: true });
  expect(box).toHaveFocus();
});

test("command_palette_starts_each_opening_with_an_empty_search", async () => {
  const user = userEvent.setup();
  render(<Harness onSelect={vi.fn()} />);
  await user.type(await search(), "over");
  await user.keyboard("{Escape}");
  await user.click(screen.getByRole("button", { name: "opener" }));
  expect(await search()).toHaveValue("");
  expect(screen.getAllByRole("option")).toHaveLength(4);
});

test("filter_groups_needs_every_word_in_the_label_or_keywords_ignoring_case", () => {
  const all = groups(() => undefined);
  const titles = (query: string) =>
    filterGroups(all, query).flatMap((group) => group.items.map((item) => item.id));
  expect(titles("")).toEqual(["overview", "incidents", "nas-01", "theme"]);
  expect(titles("INCI")).toEqual(["incidents"]);
  expect(titles("toggle dark")).toEqual(["theme"]);
  expect(titles("toggle nas")).toEqual([]);
  expect(titles("  problems ")).toEqual(["incidents"]);
});
