import { act, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { MemoryRouter } from "react-router";
import {
  Button,
  ConfirmDialog,
  Field,
  FormErrorSummary,
  IconButton,
  InfoTip,
  Menu,
  OptionPicker,
  QueryBoundary,
  StatusPill,
  Tabs,
  TextInput,
  Tooltip,
} from ".";
import { verdictMeta } from "../design/tone";

test("tooltip_appears_on_focus_describes_its_child_and_hides_on_escape", async () => {
  const user = userEvent.setup();
  render(
    <Tooltip content="Stops reminders.">
      <button type="button">Ack</button>
    </Tooltip>,
  );
  const button = screen.getByRole("button", { name: "Ack" });
  expect(screen.queryByRole("tooltip")).toBeNull();
  expect(button, "described while closed too").toHaveAccessibleDescription("Stops reminders.");

  await user.tab();
  const tip = screen.getByRole("tooltip");
  expect(tip).toHaveTextContent("Stops reminders.");
  expect(button).toHaveAttribute("aria-describedby", tip.id);
  expect(button).toHaveAccessibleDescription("Stops reminders.");

  await user.keyboard("{Escape}");
  expect(screen.queryByRole("tooltip")).toBeNull();
});

test("tooltip_waits_before_showing_on_hover_and_never_shows_for_touch", () => {
  vi.useFakeTimers();
  try {
    render(
      <Tooltip content="Exact time">
        <span>5 minutes ago</span>
      </Tooltip>,
    );
    const anchor = screen.getByText("5 minutes ago").parentElement as HTMLElement;
    fireEvent.pointerEnter(anchor, { pointerType: "mouse" });
    act(() => void vi.advanceTimersByTime(300));
    expect(screen.queryByRole("tooltip"), "not yet").toBeNull();
    act(() => void vi.advanceTimersByTime(150));
    expect(screen.getByRole("tooltip")).toHaveTextContent("Exact time");
    fireEvent.pointerLeave(anchor);
    expect(screen.queryByRole("tooltip")).toBeNull();

    fireEvent.pointerEnter(anchor, { pointerType: "touch" });
    act(() => void vi.advanceTimersByTime(1000));
    expect(screen.queryByRole("tooltip"), "touch has no hover").toBeNull();
  } finally {
    vi.useRealTimers();
  }
});

test("status_pill_shows_its_label_tone_and_explains_itself", () => {
  render(<StatusPill meta={verdictMeta("ACTION_NEEDED")} />);
  const pill = screen.getByText("Action needed");
  expect(pill).toHaveAttribute("data-tone", "danger");
  expect(pill).toHaveAccessibleDescription("Something is broken or will break soon; act now.");
});

test("disabled_button_with_a_reason_stays_focusable_explains_itself_and_ignores_clicks", async () => {
  const user = userEvent.setup();
  const onClick = vi.fn();
  render(
    <Button disabledReason="The agent is offline." onClick={onClick}>
      Refresh
    </Button>,
  );
  const button = screen.getByRole("button", { name: "Refresh" });
  expect(button).toHaveAttribute("aria-disabled", "true");
  expect(button).not.toBeDisabled();

  await user.tab();
  expect(button).toHaveFocus();
  expect(screen.getByRole("tooltip")).toHaveTextContent("The agent is offline.");
  expect(button).toHaveAccessibleDescription("The agent is offline.");

  await user.click(button);
  await user.keyboard("{Enter}");
  expect(onClick).not.toHaveBeenCalled();
});

test("loading_button_is_busy_keeps_its_name_and_ignores_clicks", async () => {
  const user = userEvent.setup();
  const onClick = vi.fn();
  render(
    <Button loading onClick={onClick}>
      Save
    </Button>,
  );
  const button = screen.getByRole("button", { name: "Save" });
  expect(button).toHaveAttribute("aria-busy", "true");
  await user.click(button);
  expect(onClick).not.toHaveBeenCalled();
});

test("a_blocked_submit_button_does_not_submit_its_form", async () => {
  const user = userEvent.setup();
  const onSubmit = vi.fn((event: React.FormEvent) => event.preventDefault());
  render(
    <form onSubmit={onSubmit}>
      <Button type="submit" disabledReason="Give it a title first.">
        Save
      </Button>
    </form>,
  );
  await user.click(screen.getByRole("button", { name: "Save" }));
  expect(onSubmit).not.toHaveBeenCalled();
});

test("icon_button_is_named_by_its_label_which_is_also_its_tooltip", async () => {
  const user = userEvent.setup();
  render(<IconButton icon="refresh" label="Refresh" />);
  await user.tab();
  expect(screen.getByRole("button", { name: "Refresh" })).toHaveFocus();
  expect(screen.getByRole("tooltip")).toHaveTextContent("Refresh");
});

test("tabs_arrow_home_and_end_keys_select_and_focus_the_tab", async () => {
  const user = userEvent.setup();
  render(
    <Tabs
      label="Host details"
      tabs={[
        { label: "Overview", content: <p>overview panel</p> },
        { label: "Checks", content: <p>checks panel</p>, count: { value: 2, label: "failing" } },
        { label: "Logs", content: <p>logs panel</p> },
      ]}
    />,
  );
  const tab = (name: RegExp) => screen.getByRole("tab", { name });
  expect(tab(/Checks/), "the count is part of the name").toHaveAccessibleName("Checks 2 failing");

  await user.click(tab(/Overview/));
  await user.keyboard("{ArrowRight}");
  expect(tab(/Checks/)).toHaveFocus();
  expect(tab(/Checks/)).toHaveAttribute("aria-selected", "true");
  expect(screen.getByRole("tabpanel")).toHaveTextContent("checks panel");

  await user.keyboard("{End}");
  expect(tab(/Logs/)).toHaveFocus();
  await user.keyboard("{ArrowRight}");
  expect(tab(/Overview/), "wraps around").toHaveFocus();
  await user.keyboard("{ArrowLeft}");
  expect(tab(/Logs/)).toHaveFocus();
  await user.keyboard("{Home}");
  expect(tab(/Overview/)).toHaveFocus();
  expect(screen.getByRole("tabpanel")).toHaveTextContent("overview panel");
});

function Confirming({ onConfirm }: { onConfirm: () => void }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button type="button" onClick={() => setOpen(true)}>
        Revoke
      </button>
      {open && (
        <ConfirmDialog
          title="Revoke agent nas-01?"
          consequence="It disconnects now and needs a new invite to come back."
          confirmLabel="Revoke agent"
          tone="danger"
          requireText="nas-01"
          onConfirm={onConfirm}
          onCancel={() => setOpen(false)}
        />
      )}
    </>
  );
}

test("dialog_takes_focus_closes_on_escape_and_returns_focus", async () => {
  const user = userEvent.setup();
  render(<Confirming onConfirm={vi.fn()} />);
  const opener = screen.getByRole("button", { name: "Revoke" });
  await user.click(opener);

  const dialog = screen.getByRole("dialog", { name: "Revoke agent nas-01?" });
  expect(dialog).toHaveTextContent("It disconnects now and needs a new invite to come back.");
  expect(dialog.contains(document.activeElement), "focus moved inside").toBe(true);

  await user.keyboard("{Escape}");
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(opener, "focus returns to the opener").toHaveFocus();
});

test("confirm_dialog_with_require_text_stays_blocked_until_the_name_is_typed", async () => {
  const user = userEvent.setup();
  const onConfirm = vi.fn();
  render(<Confirming onConfirm={onConfirm} />);
  await user.click(screen.getByRole("button", { name: "Revoke" }));
  const dialog = screen.getByRole("dialog");
  const confirm = () => within(dialog).getByRole("button", { name: "Revoke agent" });

  expect(confirm()).toHaveAttribute("aria-disabled", "true");
  await user.click(confirm());
  expect(onConfirm).not.toHaveBeenCalled();

  await user.type(within(dialog).getByLabelText("Type nas-01 to confirm"), "nas-0");
  expect(confirm()).toHaveAttribute("aria-disabled", "true");
  await user.keyboard("1");
  expect(confirm()).not.toHaveAttribute("aria-disabled");
  await user.click(confirm());
  expect(onConfirm).toHaveBeenCalledOnce();
});

test("field_marks_its_input_invalid_and_the_summary_link_focuses_it", async () => {
  const user = userEvent.setup();
  const issues = [
    { path: "web.port", message: "Port must be between 1 and 65535." },
    { path: "", message: "Two settings contradict each other." },
  ];
  render(
    <>
      <FormErrorSummary issues={issues} />
      <Field label="Port" path="web.port" error="Port must be between 1 and 65535.">
        <TextInput defaultValue="70000" />
      </Field>
      <Field label="Name" path="hub.name" hint="Shown in messages.">
        <TextInput defaultValue="home" />
      </Field>
    </>,
  );
  const port = screen.getByLabelText("Port");
  expect(port).toHaveAttribute("aria-invalid", "true");
  expect(port).toHaveAccessibleDescription("Port must be between 1 and 65535.");
  const name = screen.getByLabelText("Name");
  expect(name).not.toHaveAttribute("aria-invalid");
  expect(name, "the hint describes a valid field").toHaveAccessibleDescription(
    "Shown in messages.",
  );

  const summary = screen.getByText("2 problems to fix").closest('[role="alert"]') as HTMLElement;
  expect(within(summary).getByText("Two settings contradict each other.").tagName).toBe("LI");
  await user.click(
    within(summary).getByRole("link", { name: "Port must be between 1 and 65535." }),
  );
  expect(port).toHaveFocus();
});

describe("QueryBoundary", () => {
  const base = { data: undefined, error: null, isPending: false, refetch: () => undefined };
  const show = (query: Parameters<typeof QueryBoundary<string[]>>[0]["query"]) =>
    render(
      <QueryBoundary
        query={query}
        context="load hosts"
        isEmpty={(hosts) => hosts.length === 0}
        empty={<p>No hosts yet</p>}
      >
        {(hosts) => <p>Hosts: {hosts.join(", ")}</p>}
      </QueryBoundary>,
    );

  test("shows_loading_while_the_query_is_pending", () => {
    show({ ...base, isPending: true });
    expect(screen.getByRole("status")).toBeInTheDocument();
    expect(screen.queryByText(/No hosts yet|Hosts:/)).toBeNull();
  });

  test("shows_the_error_with_retry_and_never_the_empty_state", async () => {
    const user = userEvent.setup();
    const refetch = vi.fn();
    show({ ...base, error: new TypeError("Failed to fetch"), refetch });
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("Could not load hosts");
    expect(alert).toHaveTextContent("The hub did not answer.");
    expect(screen.queryByText("No hosts yet")).toBeNull();
    await user.click(within(alert).getByRole("button", { name: "Retry" }));
    expect(refetch).toHaveBeenCalledOnce();
  });

  test("shows_the_empty_state_when_there_is_nothing", () => {
    show({ ...base, data: [] });
    expect(screen.getByText("No hosts yet")).toBeVisible();
  });

  test("shows_the_content_and_keeps_it_when_a_refresh_fails", () => {
    show({ ...base, data: ["nas-01"], error: new TypeError("Failed to fetch") });
    expect(screen.getByText("Hosts: nas-01")).toBeVisible();
    expect(screen.queryByRole("alert")).toBeNull();
  });
});

test("info_tip_opens_on_click_and_closes_on_escape_returning_focus", async () => {
  const user = userEvent.setup();
  render(
    <InfoTip title="Known condition" learnMore="concepts/incidents.html">
      Matching incidents never alert you.
    </InfoTip>,
  );
  const button = screen.getByRole("button", { name: "About Known condition" });
  expect(button).toHaveAttribute("aria-expanded", "false");
  await user.click(button);
  const popover = screen.getByRole("dialog", { name: "Known condition" });
  expect(popover).toHaveTextContent("Matching incidents never alert you.");
  expect(
    within(popover).getByRole("link", { name: /Learn more about Known condition/ }),
  ).toHaveAttribute("href", "https://RoastSlav.github.io/domovoi/concepts/incidents.html");
  await user.keyboard("{Escape}");
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(button).toHaveFocus();
});

test("menu_moves_with_arrow_keys_and_runs_the_chosen_entry", async () => {
  const user = userEvent.setup();
  const onEdit = vi.fn();
  const onDelete = vi.fn();
  render(
    <MemoryRouter>
      <Menu
        items={[
          { label: "Edit", onSelect: onEdit },
          { label: "Delete", onSelect: onDelete, tone: "danger" },
        ]}
      />
    </MemoryRouter>,
  );
  const more = screen.getByRole("button", { name: "More actions" });
  await user.click(more);
  expect(screen.getByRole("menuitem", { name: "Edit" })).toHaveFocus();
  await user.keyboard("{ArrowDown}");
  expect(screen.getByRole("menuitem", { name: "Delete" })).toHaveFocus();
  await user.keyboard("{Enter}");
  expect(onDelete).toHaveBeenCalledOnce();
  expect(onEdit).not.toHaveBeenCalled();
  expect(screen.queryByRole("menu")).toBeNull();
  expect(more).toHaveFocus();
});

test("menu_shows_a_header_groups_entries_under_a_heading_and_marks_the_chosen_one", async () => {
  const user = userEvent.setup();
  const onDark = vi.fn();
  render(
    <Menu
      label="Account"
      icon="user"
      header="Signed in as admin"
      items={[
        { label: "Light", group: "Theme", checked: true, onSelect: () => undefined },
        { label: "Dark", group: "Theme", checked: false, onSelect: onDark },
        { label: "Log out", onSelect: () => undefined },
      ]}
    />,
  );
  await user.click(screen.getByRole("button", { name: "Account" }));

  expect(screen.getByText("Signed in as admin")).toBeInTheDocument();
  const theme = within(screen.getByRole("group", { name: "Theme" }));
  expect(theme.getByRole("menuitemradio", { name: "Light" })).toBeChecked();
  expect(theme.getByRole("menuitemradio", { name: "Light" })).toHaveFocus();
  expect(
    within(screen.getByRole("group", { name: "Theme" })).queryByRole("menuitem", {
      name: "Log out",
    }),
    "an entry without the group sits outside it",
  ).toBeNull();
  await user.keyboard("{ArrowDown}{ArrowDown}");
  expect(screen.getByRole("menuitem", { name: "Log out" })).toHaveFocus();
  await user.click(screen.getByRole("menuitemradio", { name: "Dark" }));
  expect(onDark).toHaveBeenCalledOnce();
});

function Picker({ count = 3 }: { count?: number }) {
  const [value, setValue] = useState("");
  const hosts = Array.from({ length: count }, (_, index) => ({
    value: `host-${index + 1}`,
    label: `host-${index + 1}`,
  }));
  return (
    <OptionPicker
      label="Host"
      value={value}
      options={[{ value: "", label: "All hosts" }, ...hosts]}
      onChange={setValue}
    />
  );
}

test("option_picker_opens_on_the_current_choice_and_arrow_keys_and_enter_choose", async () => {
  const user = userEvent.setup();
  render(<Picker />);
  const picker = screen.getByRole("combobox", { name: "Host" });
  expect(picker).toHaveTextContent("All hosts");
  expect(picker).toHaveAttribute("aria-expanded", "false");

  await user.click(picker);
  expect(picker).toHaveAttribute("aria-expanded", "true");
  expect(screen.getByRole("option", { name: "All hosts" })).toHaveFocus();
  expect(screen.getByRole("option", { name: "All hosts" })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  expect(screen.queryByRole("searchbox")).toBeNull();

  await user.keyboard("{ArrowDown}{ArrowDown}{Enter}");
  expect(screen.queryByRole("listbox")).toBeNull();
  expect(picker).toHaveTextContent("host-2");
  expect(picker).toHaveFocus();
});

test("option_picker_closes_on_escape_without_changing_the_choice", async () => {
  const user = userEvent.setup();
  render(<Picker />);
  const picker = screen.getByRole("combobox", { name: "Host" });
  await user.click(picker);
  await user.keyboard("{ArrowDown}{Escape}");

  expect(screen.queryByRole("listbox")).toBeNull();
  expect(picker).toHaveTextContent("All hosts");
  expect(picker).toHaveFocus();
});

test("option_picker_with_many_options_filters_as_you_type", async () => {
  const user = userEvent.setup();
  render(<Picker count={12} />);
  await user.click(screen.getByRole("combobox", { name: "Host" }));

  const filter = screen.getByRole("searchbox", { name: "Filter: Host" });
  expect(filter).toHaveFocus();
  await user.type(filter, "host-1");
  expect(screen.getAllByRole("option").map((option) => option.textContent)).toEqual([
    "host-1",
    "host-10",
    "host-11",
    "host-12",
  ]);
  await user.type(filter, "2{Enter}");
  expect(screen.getByRole("combobox", { name: "Host" })).toHaveTextContent("host-12");

  await user.click(screen.getByRole("combobox", { name: "Host" }));
  await user.type(screen.getByRole("searchbox", { name: "Filter: Host" }), "zzz");
  expect(screen.getByText("Nothing matches.")).toBeInTheDocument();
});
