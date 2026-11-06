import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { IncidentSummary } from "../../api/types";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { detail, incident, nas } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { IncidentDetailPage } from "./IncidentDetailPage";
import { IncidentListPage } from "./IncidentListPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

beforeEach(() => {
  resetApi();
  route("GET /api/v1/hosts", () => [nas, { ...nas, id: "host-pi", name: "pi" }]);
});

function incidentQueries() {
  return calls
    .filter((call) => call.route === "GET /api/v1/incidents")
    .map((call) => call.init.params?.query);
}

/** One page of incidents for each status, so each one lands in its own group. */
function routeByStatus(incidents: Record<string, IncidentSummary[]>) {
  route("GET /api/v1/incidents", ({ params }) => ({
    items: incidents[String(params?.query?.status)] ?? [],
    next_cursor: null,
  }));
}

/** The group whose heading is `heading`, as a query scope. */
function group(heading: string) {
  const section = screen.getByRole("heading", { name: heading }).closest("section");
  return within(section as HTMLElement);
}

test("incident_list_renders_filters_from_url_and_paginates", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/incidents", ({ params }) =>
    params?.query?.cursor === "c1"
      ? { items: [incident({ id: "inc-old", title: "Older one" })], next_cursor: null }
      : {
          items: [incident({ verdict: "MONITOR", occurrences: 3 })],
          next_cursor: "c1",
        },
  );
  renderAt("/incidents?status=open&severity=critical", "/incidents", <IncidentListPage />);

  expect(await screen.findByRole("link", { name: "Disk /srv is 93% full" })).toHaveAttribute(
    "href",
    "/incidents/inc-1",
  );
  expect(screen.getByLabelText("Status")).toHaveValue("open");
  expect(screen.getByLabelText("Severity")).toHaveValue("critical");
  expect(screen.getByLabelText("Host")).toHaveValue("");
  expect(screen.getByText("Monitor")).toBeVisible();
  expect(screen.getByText(/nas · open · last fired/)).toHaveTextContent("3 times");
  expect(incidentQueries()).toEqual([{ status: "open", severity: "critical", limit: 50 }]);

  await user.click(screen.getByRole("button", { name: "Load more" }));
  expect(await screen.findByRole("link", { name: "Older one" })).toBeVisible();
  expect(incidentQueries()[1]).toEqual({
    status: "open",
    severity: "critical",
    cursor: "c1",
    limit: 50,
  });
  expect(screen.queryByRole("button", { name: "Load more" }), "last page").toBeNull();

  await screen.findByRole("option", { name: "pi" });
  await user.selectOptions(screen.getByLabelText("Host"), "pi");
  await user.selectOptions(screen.getByLabelText("Status"), "");
  expect(screen.getByTestId("location")).toHaveTextContent("/incidents?severity=critical&host=pi");
  await screen.findAllByRole("link");
  expect(incidentQueries().slice(-5)).toEqual(
    ["open", "acked", "muted", "pending", "resolved"].map((status) => ({
      status,
      severity: "critical",
      host: "pi",
      limit: 50,
    })),
  );
});

test("incident_list_groups_by_state", async () => {
  routeByStatus({
    open: [incident({ id: "inc-open", title: "Open one", status: "open" })],
    acked: [incident({ id: "inc-acked", title: "Acked one", status: "acked" })],
    muted: [
      incident({
        id: "inc-muted",
        title: "Muted one",
        status: "muted",
        last_firing_at: "2026-11-01T12:00:00Z",
      }),
    ],
    pending: [incident({ id: "inc-pending", title: "Pending one", status: "pending" })],
    resolved: [incident({ id: "inc-resolved", title: "Resolved one", status: "resolved" })],
  });
  renderAt("/incidents", "/incidents", <IncidentListPage />);

  await screen.findByRole("heading", { name: "Needs attention (1)" });
  expect(group("Needs attention (1)").getByRole("link", { name: "Open one" })).toBeVisible();
  expect(
    group("Needs attention (1)").getByText("New problems nobody has acknowledged."),
  ).toBeVisible();

  await screen.findByRole("heading", { name: "Being handled (2)" });
  expect(
    group("Being handled (2)")
      .getAllByRole("link")
      .map((link) => link.textContent),
    "acked and muted merge into one group, newest firing first",
  ).toEqual(["Muted one", "Acked one"]);

  await screen.findByRole("heading", { name: "Waiting for confirmation (1)" });
  expect(
    group("Waiting for confirmation (1)").getByRole("link", { name: "Pending one" }),
  ).toBeVisible();

  await screen.findByRole("heading", { name: "Resolved (1)" });
  const details = screen
    .getByRole("heading", { name: "Resolved (1)" })
    .closest("details") as HTMLElement;
  expect(details, "the resolved group starts closed").not.toHaveAttribute("open");
  expect(within(details).getByRole("link", { name: "Resolved one" })).not.toBeVisible();
  expect(within(details).getByRole("link", { name: "All resolved incidents" })).toHaveAttribute(
    "href",
    "/incidents?status=resolved",
  );
});

test("incident_list_says_nothing_needs_attention", async () => {
  routeByStatus({});
  renderAt("/incidents", "/incidents", <IncidentListPage />);

  expect(await screen.findByText("Nothing needs attention.")).toBeVisible();
  expect(screen.getByRole("heading", { name: "Needs attention (0)" })).toBeVisible();
  expect(
    screen.queryByRole("heading", { name: "Being handled (0)" }),
    "empty groups hide",
  ).toBeNull();
  expect(
    screen.queryByRole("heading", { name: "Resolved (0)" }),
    "no empty resolved group",
  ).toBeNull();
});

test("incident_list_says_it_could_not_load_incidents", async () => {
  route("GET /api/v1/incidents", () => failure(500, "internal", "the hub is sick"));
  renderAt("/incidents", "/incidents", <IncidentListPage />);

  expect(await screen.findByText("Could not load incidents")).toBeVisible();
  expect(
    screen.queryByRole("heading", { name: "Needs attention (0)" }),
    "no group behind a failed request",
  ).toBeNull();
  expect(
    screen.queryByText("Nothing needs attention."),
    "a failed load is never read as all clear",
  ).toBeNull();
});

test("resolved_incidents_show_a_resolved_badge", async () => {
  routeByStatus({ resolved: [incident({ status: "resolved", verdict: "ACTION_NEEDED" })] });
  renderAt("/incidents", "/incidents", <IncidentListPage />);

  const heading = await screen.findByRole("heading", { name: "Resolved (1)" });
  const details = heading.closest("details") as HTMLElement;
  expect(heading).toBeVisible();
  expect(within(details).getByText("Resolved")).toBeInTheDocument();
  expect(within(details).queryByText("Action needed")).toBeNull();
  expect(within(details).getByText(/nas · resolved · last fired/)).toHaveTextContent(
    "triage said Action needed",
  );
});

test("resolved_incident_without_verdict_has_no_triage_note", async () => {
  routeByStatus({ resolved: [incident({ status: "resolved", verdict: null })] });
  renderAt("/incidents", "/incidents", <IncidentListPage />);

  const heading = await screen.findByRole("heading", { name: "Resolved (1)" });
  const details = heading.closest("details") as HTMLElement;
  expect(within(details).getByText(/nas · resolved · last fired/)).toHaveTextContent(
    "nas · resolved · last fired",
  );
  expect(within(details).queryByText(/triage said/)).toBeNull();
});

test("resolved_card_shows_resolved_badge_and_keeps_the_verdict", async () => {
  const d = detail();
  route("GET /api/v1/incidents/{id}", () => ({
    ...d,
    incident: { ...d.incident, status: "resolved" },
    card: { ...d.card, resolved: true },
  }));
  renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);

  const card = await screen.findByRole("region", { name: "Summary" });
  expect(card).toHaveTextContent("Resolved Triage said Action needed");
});

test("resolved_card_without_triage_claims_no_verdict", async () => {
  const d = detail();
  route("GET /api/v1/incidents/{id}", () => ({
    ...d,
    incident: incident({ status: "resolved", verdict: null }),
    report: null,
    card: { ...d.card, badge: "warning", resolved: true },
  }));
  renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);

  const card = await screen.findByRole("region", { name: "Summary" });
  expect(within(card).getByText("Resolved")).toBeVisible();
  expect(within(card).queryByText(/Triage said/)).toBeNull();
});

test("incident_detail_shows_card_fields_and_timeline", async () => {
  route("GET /api/v1/incidents/{id}", () => detail());
  renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);

  const card = await screen.findByRole("region", { name: "Summary" });
  expect(within(card).getByText("Action needed")).toBeVisible();
  expect(within(card).getByRole("heading", { name: "Backups filled /srv" })).toBeVisible();
  expect(
    within(card)
      .getAllByRole("listitem")
      .map((li) => li.textContent),
  ).toEqual(["93% used, 70 GB free", "Grew 40 GB since 02:00"]);
  expect(within(card).getByText(/Prune old snapshots/)).toBeVisible();
  expect(calls[0]?.init.params?.path).toEqual({ id: "inc-1" });

  expect(screen.getByText("every").tagName, "details render as Markdown").toBe("STRONG");
  const timeline = screen.getByRole("region", { name: "Timeline" });
  expect(within(timeline).getByText("incident.opened")).toBeVisible();
  expect(within(timeline).getByText("Disk /srv crossed 90%")).toBeVisible();
  expect(within(timeline).getAllByRole("listitem")).toHaveLength(2);

  const actions = screen.getByRole("region", { name: "Actions" });
  expect(within(actions).getByText("restart_container")).toBeVisible();
  expect(within(actions).getByRole("button", { name: "Approve" })).toBeVisible();
  expect(screen.getByRole("button", { name: "Ack" })).toBeVisible();
  expect(screen.getByRole("button", { name: "Mute" })).toBeVisible();
});

test("incident_detail_retries_triage_and_shows_how_it_went", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/incidents/{id}", () => detail());
  route("POST /api/v1/incidents/{id}/triage", () => ({
    kind: "failed",
    reason: "the model is not available",
  }));
  renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);

  await user.click(await screen.findByRole("button", { name: "Retry triage" }));

  expect(await screen.findByText(/Triage failed: the model is not available/)).toBeVisible();
  expect(calls.map((call) => call.route)).toContain("POST /api/v1/incidents/{id}/triage");
});

test("incident_detail_ack_and_ask", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/incidents/{id}", () => detail());
  route("POST /api/v1/incidents/{id}/ack", () => undefined);
  route("POST /api/v1/incidents/{id}/ask", () =>
    failure(503, "unavailable", "triage is not configured"),
  );
  renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);

  await user.click(await screen.findByRole("button", { name: "Ack" }));
  expect(calls.map((c) => c.route)).toContain("POST /api/v1/incidents/{id}/ack");

  await user.type(screen.getByLabelText("Ask about this incident"), "Why now?");
  await user.click(screen.getByRole("button", { name: "Ask" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Triage is off");
  expect(calls.at(-1)?.init.body).toEqual({ question: "Why now?" });
});
