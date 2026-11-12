import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { IncidentSummary } from "../../api/types";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { incident, nas, overview, stubMedia } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { IncidentListPage } from "./IncidentListPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

beforeEach(() => {
  resetApi();
  route("GET /api/v1/hosts", () => [nas, { ...nas, id: "host-pi", name: "pi" }]);
  route("GET /api/v1/overview", () => overview());
});

afterEach(() => {
  vi.unstubAllGlobals();
});

function incidentQueries() {
  return calls
    .filter((call) => call.route === "GET /api/v1/incidents")
    .map((call) => call.init.params?.query);
}

/** One page of incidents for each group, as the hub's `group` filter would answer. */
function routeByGroup(incidents: Record<string, IncidentSummary[]>) {
  route("GET /api/v1/incidents", ({ params }) => ({
    items: incidents[String(params?.query?.group)] ?? [],
    next_cursor: null,
  }));
}

/** The section whose heading is `heading`, as a query scope. */
function section(heading: string) {
  return within(screen.getByRole("region", { name: heading }));
}

const TITLE = "Disk /srv is 93% full";

test("incident_list_is_a_table_on_a_wide_screen", async () => {
  stubMedia(true);
  routeByGroup({ needs_action: [incident({ occurrences: 3 })] });
  renderAt("/incidents", "/incidents", <IncidentListPage />);

  const table = await screen.findByRole("table", { name: "Action needed" });
  expect(
    within(table)
      .getAllByRole("columnheader")
      .map((th) => th.textContent),
  ).toEqual(["Incident", "Host", "Check", "Severity", "Last fired", "Times", "Actions"]);
  const link = within(table).getByRole("link", { name: TITLE });
  expect(link, "the title is the link and keeps its name").toHaveAttribute(
    "href",
    "/incidents/inc-1",
  );
  const row = link.closest("tr") as HTMLElement;
  expect(
    within(table).queryByRole("columnheader", { name: "Triage" }),
    "no row has a triage view, so the column is not drawn",
  ).toBeNull();
  expect(row, "no red wash on a row, however many need action").not.toHaveAttribute("data-state");
  expect(row, "the stripe follows severity, not status").toHaveAttribute("data-tone", "warning");
  expect(
    within(table).queryByRole("columnheader", { name: "Status" }),
    "every row in Action needed is open, and the heading says so",
  ).toBeNull();
  expect(within(row).getByText("nas")).toBeVisible();
  expect(within(row).getByText("disk.usage /srv")).toBeVisible();
  expect(within(row).getByText("Warning")).toBeVisible();
  expect(within(row).getByText("3")).toBeVisible();
});

test("incident_list_is_cards_on_a_narrow_screen", async () => {
  stubMedia(false);
  routeByGroup({ needs_action: [incident({ occurrences: 3 })] });
  renderAt("/incidents", "/incidents", <IncidentListPage />);

  const list = await screen.findByRole("list", { name: "Action needed" });
  expect(screen.queryByRole("table")).toBeNull();
  const card = within(list).getByRole("listitem");
  expect(within(card).getByRole("link", { name: TITLE })).toHaveAttribute(
    "href",
    "/incidents/inc-1",
  );
  expect(card, "the same facts as the table").toHaveTextContent(
    /Disk \/srv is 93% full.*nas.*disk\.usage \/srv.*Warning.*Last fired.*3 times/,
  );
});

test("incident_list_groups_by_what_each_incident_asks", async () => {
  routeByGroup({
    needs_action: [incident({ id: "a", title: "Broken one" })],
    monitor: [incident({ id: "m", title: "Watched one", verdict: "MONITOR" })],
    pending: [incident({ id: "p", title: "Pending one", status: "pending" })],
    expected: [incident({ id: "e", title: "Expected one", verdict: "EXPECTED" })],
    handled: [
      incident({ id: "h1", title: "Acked one", status: "acked" }),
      incident({ id: "h2", title: "Muted one", status: "muted" }),
    ],
    resolved: [incident({ id: "r", title: "Resolved one", status: "resolved" })],
  });
  const user = userEvent.setup();
  renderAt("/incidents", "/incidents", <IncidentListPage />);

  await screen.findByRole("heading", { name: "Action needed" });
  expect(
    screen.getAllByRole("heading", { level: 2 }).map((heading) => heading.textContent),
    "the most urgent group comes first, resolved last",
  ).toEqual([
    "Action needed",
    "Monitor",
    "Pending",
    "Expected",
    "Acknowledged or muted",
    "Resolved",
  ]);
  expect(section("Action needed").getByRole("link", { name: "Broken one" })).toBeVisible();
  expect(
    section("Action needed").getByText(/^Open, and nobody has taken it\./),
    "each group says what it means",
  ).toBeVisible();
  expect(section("Acknowledged or muted").getAllByRole("link")).toHaveLength(2);
  expect(
    incidentQueries()
      .map((query) => query?.group)
      .sort(),
    "each group is filtered on the hub",
  ).toEqual(["expected", "handled", "monitor", "needs_action", "pending", "resolved"]);

  const resolved = screen.getByRole("button", { name: "Resolved" });
  expect(resolved, "the resolved group starts closed").toHaveAttribute("aria-expanded", "false");
  expect(screen.queryByRole("link", { name: "Resolved one" })).toBeNull();
  await user.click(resolved);
  expect(section("Resolved").getByRole("link", { name: "Resolved one" })).toBeVisible();
});

test("filter_chips_update_the_url_and_the_list", async () => {
  const user = userEvent.setup();
  routeByGroup({
    needs_action: [incident({ id: "a", title: "Broken one" })],
    monitor: [incident({ id: "m", title: "Watched one", verdict: "MONITOR" })],
  });
  renderAt("/incidents?host=pi", "/incidents", <IncidentListPage />);

  await screen.findByRole("link", { name: "Broken one" });
  expect(screen.getByRole("radio", { name: "All" })).toBeChecked();

  await user.click(screen.getByRole("radio", { name: "Monitor" }));
  expect(
    screen.getByTestId("location"),
    "the filter is in the URL, beside the host",
  ).toHaveTextContent("/incidents?host=pi&group=monitor");
  expect(await screen.findByRole("link", { name: "Watched one" })).toBeVisible();
  expect(screen.queryByRole("link", { name: "Broken one" })).toBeNull();
  expect(incidentQueries().at(-1)).toEqual({ host: "pi", group: "monitor", limit: 50 });

  await user.selectOptions(screen.getByLabelText("Severity"), "critical");
  expect(screen.getByTestId("location")).toHaveTextContent(
    "/incidents?host=pi&group=monitor&severity=critical",
  );

  await user.click(screen.getByRole("radio", { name: "All" }));
  expect(screen.getByTestId("location")).toHaveTextContent("/incidents?host=pi&severity=critical");
});

test("incident_list_reads_its_filters_from_the_url", async () => {
  routeByGroup({ handled: [incident({ status: "acked" })] });
  renderAt("/incidents?group=handled&q=disk&severity=warning", "/incidents", <IncidentListPage />);

  expect(await screen.findByRole("link", { name: TITLE })).toBeVisible();
  expect(screen.getByRole("radio", { name: "Acknowledged or muted" })).toBeChecked();
  expect(screen.getByLabelText("Search")).toHaveValue("disk");
  expect(screen.getByLabelText("Severity")).toHaveValue("warning");
  expect(incidentQueries()).toEqual([
    { group: "handled", q: "disk", severity: "warning", limit: 50 },
  ]);
});

test("search_asks_the_hub_once_typing_rests", async () => {
  const user = userEvent.setup();
  routeByGroup({ needs_action: [incident()] });
  renderAt("/incidents?group=needs_action", "/incidents", <IncidentListPage />);

  await screen.findByRole("link", { name: TITLE });
  await user.type(screen.getByLabelText("Search"), "srv");
  await vi.waitFor(() =>
    expect(screen.getByTestId("location")).toHaveTextContent("/incidents?group=needs_action&q=srv"),
  );
  await vi.waitFor(() =>
    expect(incidentQueries().at(-1)).toEqual({ group: "needs_action", q: "srv", limit: 50 }),
  );
  expect(
    incidentQueries().filter((query) => query?.q !== undefined && query.q !== "srv"),
    "no request per keystroke",
  ).toEqual([]);
});

test("counts_show_once_on_the_chips_and_nowhere_else", async () => {
  route("GET /api/v1/overview", () =>
    overview({
      incident_groups: { needs_action: 2, monitor: 3, pending: 1, expected: 0, handled: 4 },
    }),
  );
  routeByGroup({});
  renderAt("/incidents", "/incidents", <IncidentListPage />);

  const chip = (name: string) => screen.getByRole("radio", { name }).closest("label");
  expect(await screen.findByRole("radio", { name: "Action needed 2 incidents" })).toBeVisible();
  expect(chip("Action needed 2 incidents")).toHaveTextContent("Action needed2");
  expect(chip("Acknowledged or muted 4 incidents")).toBeVisible();
  expect(chip("Expected"), "a zero is not shown").toHaveTextContent(/^Expected$/);
  expect(screen.queryByText("2 need action"), "no second count in the header").toBeNull();
  expect(screen.queryByText("Nothing needs you")).toBeNull();
});

test("a_group_heading_carries_no_count", async () => {
  routeByGroup({ needs_action: [incident(), incident({ id: "b", title: "Other" })] });
  renderAt("/incidents", "/incidents", <IncidentListPage />);

  expect(await screen.findByRole("heading", { name: "Action needed" })).toBeVisible();
  expect(screen.queryByRole("heading", { name: /\(\d+\+?\)/ })).toBeNull();
});

test("a_whole_row_opens_the_incident_but_the_menu_does_not", async () => {
  stubMedia(true);
  const user = userEvent.setup();
  routeByGroup({ needs_action: [incident()] });
  renderAt("/incidents", "/incidents", <IncidentListPage />);

  const row = (await screen.findByRole("link", { name: TITLE })).closest("tr") as HTMLElement;
  expect(row).toHaveAttribute("data-clickable", "true");
  await user.click(within(row).getByText("nas"));
  expect(screen.getByTestId("location")).toHaveTextContent("/incidents/inc-1");
});

test("the_triage_column_shows_a_dash_with_a_reason_when_only_some_rows_have_a_view", async () => {
  stubMedia(true);
  const user = userEvent.setup();
  routeByGroup({
    handled: [
      incident({ id: "a", title: "Taken", status: "acked", verdict: "ACTION_NEEDED" }),
      incident({ id: "b", title: "Taken blind", status: "acked", verdict: null }),
    ],
  });
  renderAt("/incidents?group=handled", "/incidents", <IncidentListPage />);

  const table = await screen.findByRole("table", { name: "Acknowledged or muted" });
  expect(within(table).getByRole("columnheader", { name: "Triage" })).toBeVisible();
  const blind = screen.getByRole("link", { name: "Taken blind" }).closest("tr") as HTMLElement;
  const dash = within(blind).getByText("–");
  await user.hover(dash);
  expect(await screen.findByRole("tooltip")).toHaveTextContent(/triage has not looked at it/);
});

test("incident_list_follows_the_host_in_the_url", async () => {
  routeByGroup({ needs_action: [incident()] });
  renderAt("/incidents?group=needs_action&host=pi", "/incidents", <IncidentListPage />);

  await screen.findByRole("link", { name: TITLE });
  expect(incidentQueries()).toEqual([{ host: "pi", group: "needs_action", limit: 50 }]);
  expect(calls.find((call) => call.route === "GET /api/v1/overview")?.init.params?.query).toEqual({
    host: "pi",
  });
  expect(screen.queryByLabelText("Host"), "the top bar owns the host choice").toBeNull();
});

test("incident_list_uses_the_remembered_host_when_the_url_has_none", async () => {
  localStorage.setItem("selected-host", "pi");
  routeByGroup({ needs_action: [incident()] });
  renderAt("/incidents?group=needs_action", "/incidents", <IncidentListPage />);

  await screen.findByRole("link", { name: TITLE });
  expect(incidentQueries()).toEqual([{ host: "pi", group: "needs_action", limit: 50 }]);
});

test("incident_list_explains_what_the_page_is", async () => {
  routeByGroup({});
  renderAt("/incidents", "/incidents", <IncidentListPage />);

  expect(await screen.findByText(/^Problems Domovoi found on your hosts\.$/)).toBeVisible();
  expect(
    screen.getByRole("link", { name: "Learn more about incidents (opens in a new tab)" }),
  ).toHaveAttribute("href", "https://RoastSlav.github.io/domovoi/concepts/incidents.html");
});

test("a_first_run_with_no_incidents_points_to_hosts", async () => {
  routeByGroup({});
  renderAt("/incidents", "/incidents", <IncidentListPage />);

  expect(await screen.findByText("No incidents yet")).toBeVisible();
  expect(screen.getByRole("link", { name: "Go to hosts" })).toHaveAttribute("href", "/hosts");
  expect(screen.queryByRole("heading", { level: 2 }), "empty groups hide").toBeNull();
});

test("with_only_resolved_incidents_everything_is_healthy", async () => {
  routeByGroup({ resolved: [incident({ status: "resolved" })] });
  renderAt("/incidents", "/incidents", <IncidentListPage />);

  expect(await screen.findByText("No open incidents")).toBeVisible();
  expect(screen.getByText("Everything your agents watch is healthy.")).toBeVisible();
  expect(screen.getByRole("button", { name: "Resolved" })).toBeVisible();
});

test("an_empty_filter_says_so_and_links_to_everything", async () => {
  routeByGroup({});
  renderAt("/incidents?group=needs_action", "/incidents", <IncidentListPage />);

  expect(await screen.findByText("Nothing needs action right now.")).toBeVisible();
  expect(screen.getByRole("link", { name: "Show all incidents" })).toHaveAttribute(
    "href",
    "/incidents",
  );
});

test("incident_list_says_it_could_not_load_incidents", async () => {
  route("GET /api/v1/incidents", () => failure(500, "internal", "the hub is sick"));
  renderAt("/incidents", "/incidents", <IncidentListPage />);

  expect(await screen.findByText("Could not load incidents")).toBeVisible();
  expect(screen.getByRole("button", { name: "Retry" })).toBeVisible();
  expect(screen.queryByText("No incidents yet"), "a failed load is never read as empty").toBeNull();
  expect(screen.queryByText("No open incidents")).toBeNull();
});

test("incident_list_keeps_its_groups_when_a_refresh_fails", async () => {
  const requests = new Map<string, number>();
  route("GET /api/v1/incidents", ({ params }) => {
    const group = String(params?.query?.group);
    const seen = (requests.get(group) ?? 0) + 1;
    requests.set(group, seen);
    if (seen > 1) {
      return failure(500, "internal", "the hub is sick");
    }
    return {
      items: group === "needs_action" ? [incident({ title: "Broken one" })] : [],
      next_cursor: null,
    };
  });
  const { client } = renderAt("/incidents", "/incidents", <IncidentListPage />);

  await screen.findByRole("heading", { name: "Action needed" });
  void client.invalidateQueries({ queryKey: ["incidents"] });

  expect(await screen.findByText("Could not refresh")).toBeVisible();
  expect(
    section("Action needed").getByRole("link", { name: "Broken one" }),
    "the rows stay over a failed refetch",
  ).toBeVisible();
});

test("load_more_shows_the_next_page_and_its_own_error", async () => {
  const user = userEvent.setup();
  let sick = true;
  route("GET /api/v1/incidents", ({ params }) => {
    if (params?.query?.cursor !== "c1") {
      return { items: [incident()], next_cursor: "c1" };
    }
    return sick
      ? failure(500, "internal", "the hub is sick")
      : { items: [incident({ id: "inc-old", title: "Older one" })], next_cursor: null };
  });
  renderAt("/incidents?group=needs_action", "/incidents", <IncidentListPage />);

  await user.click(await screen.findByRole("button", { name: "Load more" }));
  expect(await screen.findByText("Could not load more incidents")).toBeVisible();
  expect(screen.getByRole("link", { name: TITLE }), "the first page stays").toBeVisible();

  sick = false;
  await user.click(screen.getByRole("button", { name: "Retry" }));
  expect(await screen.findByRole("link", { name: "Older one" })).toBeVisible();
  expect(incidentQueries().at(-1)).toEqual({ group: "needs_action", cursor: "c1", limit: 50 });
  expect(screen.queryByRole("button", { name: "Load more" }), "last page").toBeNull();
});

test("resolved_incidents_show_a_resolved_badge_and_what_triage_said", async () => {
  stubMedia(true);
  routeByGroup({
    resolved: [
      incident({ status: "resolved", verdict: "ACTION_NEEDED" }),
      incident({ id: "inc-2", title: "Never triaged", status: "resolved", verdict: null }),
    ],
  });
  renderAt("/incidents?group=resolved", "/incidents", <IncidentListPage />);

  const row = (await screen.findByRole("link", { name: TITLE })).closest("tr") as HTMLElement;
  expect(within(row).getByText("Resolved")).toBeVisible();
  expect(within(row).queryByText("Action needed")).toBeNull();
  expect(within(row).getByText("Triage said Action needed")).toBeVisible();
  expect(row, "what is over reads quieter").toHaveAttribute("data-state", "quiet");

  const untriaged = screen.getByRole("link", { name: "Never triaged" }).closest("tr");
  expect(within(untriaged as HTMLElement).queryByText(/Triage said/)).toBeNull();
});

async function openMenu(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByRole("button", { name: `Actions for ${TITLE}` }));
  return screen.getByRole("menu");
}

test("quick_ack_from_the_row_says_it_worked", async () => {
  const user = userEvent.setup();
  routeByGroup({ needs_action: [incident()] });
  route("POST /api/v1/incidents/{id}/ack", () => undefined);
  renderAt("/incidents?group=needs_action", "/incidents", <IncidentListPage />);

  const menu = await openMenu(user);
  expect(
    within(menu)
      .getAllByRole("menuitem")
      .map((item) => item.textContent),
  ).toEqual([
    "Acknowledge",
    "Mute for 1 hour",
    "Mute for 4 hours",
    "Mute for 1 day",
    "Mute for 1 week",
  ]);
  await user.click(within(menu).getByRole("menuitem", { name: "Acknowledge" }));

  expect(await screen.findByText("Incident acknowledged")).toBeVisible();
  expect(
    calls.find((c) => c.route === "POST /api/v1/incidents/{id}/ack")?.init.params?.path,
  ).toEqual({ id: "inc-1" });
});

test("a_failed_quick_ack_says_why_and_puts_the_row_back", async () => {
  const user = userEvent.setup();
  routeByGroup({ monitor: [incident({ verdict: "MONITOR" })] });
  let release: (value: unknown) => void = () => undefined;
  route("POST /api/v1/incidents/{id}/ack", () => new Promise((resolve) => (release = resolve)));
  renderAt("/incidents?group=monitor", "/incidents", <IncidentListPage />);

  const menu = await openMenu(user);
  await user.click(within(menu).getByRole("menuitem", { name: "Acknowledge" }));
  expect(await screen.findByText("Acknowledged"), "it reads as acknowledged at once").toBeVisible();

  release(failure(409, "conflict", "the incident already resolved"));
  expect(await screen.findByText("Could not acknowledge the incident")).toBeVisible();
  expect(screen.getByText("The incident already resolved.")).toBeVisible();
  expect(
    within(await screen.findByRole("list", { name: "Monitor" })).getByText("Monitor"),
    "the row is back as it was",
  ).toBeVisible();
  expect(screen.queryByText("Acknowledged")).toBeNull();
});

test("quick_mute_offers_undo", async () => {
  const user = userEvent.setup();
  routeByGroup({ needs_action: [incident()] });
  route("POST /api/v1/incidents/{id}/mute", () => undefined);
  route("POST /api/v1/incidents/{id}/unmute", () => undefined);
  renderAt("/incidents?group=needs_action", "/incidents", <IncidentListPage />);

  const menu = await openMenu(user);
  await user.click(within(menu).getByRole("menuitem", { name: "Mute for 4 hours" }));
  expect(await screen.findByText("Incident muted for 4 hours")).toBeVisible();
  expect(calls.find((c) => c.route === "POST /api/v1/incidents/{id}/mute")?.init.body).toEqual({
    minutes: 240,
  });

  await user.click(screen.getByRole("button", { name: "Undo" }));
  expect(await screen.findByText("Incident unmuted")).toBeVisible();
});

test("a_muted_row_offers_unmute_and_a_resolved_one_only_open", async () => {
  const user = userEvent.setup();
  routeByGroup({ handled: [incident({ status: "muted" })] });
  renderAt("/incidents?group=handled", "/incidents", <IncidentListPage />);

  const menu = await openMenu(user);
  expect(
    within(menu)
      .getAllByRole("menuitem")
      .map((item) => item.textContent),
  ).toEqual(["Acknowledge", "Unmute"]);
});

test("a_resolved_incident_has_no_row_menu_since_the_row_itself_opens_it", async () => {
  const user = userEvent.setup();
  routeByGroup({ resolved: [incident({ id: "r", title: "Resolved one", status: "resolved" })] });
  renderAt("/incidents", "/incidents", <IncidentListPage />);

  await user.click(await screen.findByRole("button", { name: "Resolved" }));
  expect(await screen.findAllByText("Resolved one")).not.toHaveLength(0);
  expect(screen.queryByRole("button", { name: /^Actions for/ })).toBeNull();
});

test("the_status_column_stays_where_status_varies", async () => {
  stubMedia(true);
  routeByGroup({ handled: [incident({ status: "acked" })] });
  renderAt("/incidents?group=handled", "/incidents", <IncidentListPage />);

  const table = await screen.findByRole("table", { name: "Acknowledged or muted" });
  expect(within(table).getByRole("columnheader", { name: "Status" })).toBeVisible();
});

test("every_group_table_has_the_same_fixed_columns", async () => {
  stubMedia(true);
  routeByGroup({
    needs_action: [incident({ id: "a" })],
    handled: [incident({ id: "h", status: "acked", verdict: "ACTION_NEEDED" })],
  });
  renderAt("/incidents", "/incidents", <IncidentListPage />);

  await screen.findByRole("table", { name: "Acknowledged or muted" });
  const widths = (name: string) =>
    [...screen.getByRole("table", { name }).querySelectorAll("col")]
      .map((col) => col.style.width)
      .filter(Boolean);
  const act = widths("Action needed");
  const handled = widths("Acknowledged or muted");
  expect(act, "one decision about Triage for the whole page").toContain("7rem");
  expect(handled.slice(-act.length), "the right-hand columns line up").toEqual(act);
  expect(screen.getByRole("table", { name: "Action needed" })).toHaveAttribute(
    "data-layout",
    "fixed",
  );
});

test("a_long_title_and_check_are_cut_but_keep_their_full_text", async () => {
  stubMedia(true);
  const title = "Backups have not run for a very long time on the storage box and need a look";
  routeByGroup({
    needs_action: [
      incident({
        title,
        check: "backup.age",
        target: "nightly-offsite-backup-to-the-storage-box-23",
      }),
    ],
  });
  renderAt("/incidents", "/incidents", <IncidentListPage />);

  const link = await screen.findByRole("link", { name: title });
  expect(link.closest("span")?.className, "two lines at most").toMatch(/clamp2/);
  const check = screen.getByText("backup.age nightly-offsite-backup-to-the-storage-box-23");
  expect(check.closest('[data-truncate="ellipsis"]'), "one line").not.toBeNull();
});
