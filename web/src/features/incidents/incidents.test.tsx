import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
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
  expect(incidentQueries().at(-1)).toEqual({ severity: "critical", host: "pi", limit: 50 });
});

test("incident_list_shows_empty_state", async () => {
  route("GET /api/v1/incidents", () => ({ items: [], next_cursor: null }));
  renderAt("/incidents", "/incidents", <IncidentListPage />);
  expect(await screen.findByText("No incidents")).toBeVisible();
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
