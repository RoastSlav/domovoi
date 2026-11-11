import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import {
  ChatThread,
  DataTable,
  KeyValueList,
  Menu,
  Section,
  Text,
  Timeline,
  TIMELINE_PREVIEW,
  type ChatThreadProps,
} from ".";
import { timelineMeta } from "../design/timeline";

test("a_collapsed_section_renders_its_content_only_once_opened", async () => {
  const user = userEvent.setup();
  render(
    <Section title="Recently decided" description="The last day." collapsed>
      <p>Inside</p>
    </Section>,
  );

  const toggle = screen.getByRole("button", { name: "Recently decided" });
  expect(screen.getByRole("heading", { level: 2, name: "Recently decided" })).toBeVisible();
  expect(toggle).toHaveAttribute("aria-expanded", "false");
  expect(screen.queryByText("Inside"), "closed content is not rendered at all").toBeNull();
  expect(screen.queryByText("The last day.")).toBeNull();

  await user.click(toggle);
  expect(toggle).toHaveAttribute("aria-expanded", "true");
  expect(screen.getByText("Inside")).toBeVisible();
  expect(screen.getByText("The last day.")).toBeVisible();

  await user.keyboard("{Enter}");
  expect(screen.queryByText("Inside"), "the keyboard closes it again").toBeNull();
});

test("a_plain_section_has_no_toggle", () => {
  render(
    <Section title="Timeline">
      <p>Inside</p>
    </Section>,
  );
  expect(screen.queryByRole("button")).toBeNull();
  expect(screen.getByRole("region", { name: "Timeline" })).toHaveTextContent("Inside");
});

test("a_menu_with_text_is_a_labelled_button_that_explains_itself", async () => {
  const user = userEvent.setup();
  const chosen = vi.fn();
  render(
    <Menu
      text="Mute"
      label="Mute for"
      hint="No alerts for the time you pick."
      items={[
        { label: "For 1 hour", onSelect: () => chosen(60) },
        { label: "For 1 day", onSelect: () => chosen(1440) },
      ]}
    />,
  );

  const trigger = screen.getByRole("button", { name: "Mute for" });
  expect(trigger).toHaveTextContent("Mute");
  expect(trigger).toHaveAccessibleDescription("No alerts for the time you pick.");
  expect(trigger).toHaveAttribute("aria-haspopup", "menu");

  await user.click(trigger);
  await user.keyboard("{ArrowDown}{Enter}");
  expect(chosen).toHaveBeenCalledWith(1440);
  expect(screen.queryByRole("menu")).toBeNull();
  expect(trigger, "focus returns to the button").toHaveFocus();
});

test("data_table_marks_urgent_and_quiet_rows_and_hides_an_actions_header", () => {
  const rows = [
    { id: "a", name: "Broken", state: "urgent" as const },
    { id: "b", name: "Over", state: "quiet" as const },
    { id: "c", name: "Plain", state: undefined },
  ];
  render(
    <DataTable
      caption="Things"
      rows={rows}
      rowKey={(row) => row.id}
      rowState={(row) => row.state}
      columns={[
        { key: "name", label: "Name", cell: (row) => row.name },
        { key: "actions", label: "Actions", hideLabel: true, cell: () => <button>More</button> },
      ]}
    />,
  );

  const row = (name: string) => screen.getByText(name).closest("tr");
  expect(row("Broken")).toHaveAttribute("data-state", "urgent");
  expect(row("Over")).toHaveAttribute("data-state", "quiet");
  expect(row("Plain")).not.toHaveAttribute("data-state");
  const header = screen.getByRole("columnheader", { name: "Actions" });
  expect(within(header).getByText("Actions"), "named, but not shown").toHaveClass(
    "visually-hidden",
  );
  expect(
    screen.getAllByRole("button", { name: "More" })[0]?.closest("td"),
    "no label beside the actions when the table is stacked",
  ).not.toHaveAttribute("data-label");
});

test("timeline_shows_the_newest_entries_then_all_on_request", async () => {
  const user = userEvent.setup();
  const items = Array.from({ length: TIMELINE_PREVIEW + 3 }, (_, n) => ({
    at: `2026-10-31T12:${String(n).padStart(2, "0")}:00Z`,
    meta: timelineMeta({ kind: "incident.grouped", summary: `check.${n}` }),
  }));
  render(<Timeline label="History" items={items} />);

  const list = screen.getByRole("list", { name: "History" });
  expect(within(list).getAllByRole("listitem")).toHaveLength(TIMELINE_PREVIEW);
  expect(within(list).queryByText("check.0")).toBeNull();
  expect(within(list).getByText(`check.${TIMELINE_PREVIEW + 2}`)).toBeVisible();

  await user.click(screen.getByRole("button", { name: `Show all ${items.length}` }));
  expect(within(list).getAllByRole("listitem")).toHaveLength(items.length);
  expect(screen.queryByRole("button"), "nothing left to show").toBeNull();
});

test("a_short_timeline_has_no_show_all_and_each_entry_has_its_time", () => {
  render(
    <Timeline
      label="History"
      items={[
        {
          at: "2026-10-31T12:00:00Z",
          meta: timelineMeta({ kind: "triage.failed", summary: "no model" }),
        },
      ]}
    />,
  );
  expect(screen.queryByRole("button")).toBeNull();
  const entry = screen.getByRole("listitem");
  expect(entry).toHaveTextContent("Triage failed");
  expect(entry).toHaveTextContent("no model");
  expect(
    within(entry).getByText("Triage failed").closest("li")?.querySelector("time"),
  ).toHaveAttribute("datetime", "2026-10-31T12:00:00Z");
  expect(entry.querySelector("[data-tone]")).toHaveAttribute("data-tone", "danger");
});

test("key_value_list_and_text_take_the_mono_and_muted_styles_by_attribute", () => {
  render(
    <>
      <KeyValueList mono items={[{ label: "name", value: "web" }]} />
      <Text muted mono size="sm">
        disk.usage
      </Text>
      <Text block>A paragraph</Text>
    </>,
  );
  expect(screen.getByText("web").closest("dl")).toHaveAttribute("data-mono", "true");
  const run = screen.getByText("disk.usage");
  expect(run.tagName).toBe("SPAN");
  expect(run).toHaveAttribute("data-muted", "true");
  expect(run).toHaveAttribute("data-mono", "true");
  expect(run).toHaveAttribute("data-size", "sm");
  expect(screen.getByText("A paragraph").tagName).toBe("P");
});

function thread(props: Partial<ChatThreadProps>) {
  return render(
    <ChatThread
      turns={[]}
      hint="Read-only."
      label="Ask"
      onSend={() => Promise.resolve()}
      onClear={() => undefined}
      {...props}
    />,
  );
}

test("chat_thread_shows_tools_behind_a_toggle_and_what_an_answer_proposed", async () => {
  const user = userEvent.setup();
  thread({
    turns: [
      { role: "user", text: "Why is it slow?" },
      {
        role: "assistant",
        text: "The disk is full.",
        tools: [{ name: "read logs", host: "nas-01" }, { name: "disk usage" }],
        extra: <p>An action card</p>,
      },
    ],
  });

  expect(screen.getByText("An action card")).toBeVisible();
  const toggle = screen.getByRole("button", { name: "2 tools used" });
  expect(toggle).toHaveAttribute("aria-expanded", "false");
  expect(screen.queryByText("read logs · nas-01")).toBeNull();
  await user.click(toggle);
  expect(screen.getByText("read logs · nas-01")).toBeVisible();
  expect(screen.getByText("disk usage")).toBeVisible();
});

test("chat_thread_offers_clear_only_once_there_is_something_to_clear", () => {
  const empty = thread({ starters: ["Is anything wrong?"] });
  expect(screen.queryByRole("button", { name: "Clear conversation" })).toBeNull();
  expect(screen.getByRole("button", { name: "Is anything wrong?" })).toBeVisible();
  empty.unmount();

  thread({
    turns: [{ role: "user", text: "Why?" }],
    starters: ["Is anything wrong?"],
  });
  expect(screen.getByRole("button", { name: "Clear conversation" })).toBeVisible();
  expect(screen.queryByRole("button", { name: "Is anything wrong?" })).toBeNull();
});
