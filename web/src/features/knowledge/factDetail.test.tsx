import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { failure, resetApi, route } from "../../test/apiMock";
import {
  FACT_PURGED,
  FACT_RETRACTED,
  FACTS_PAGE,
  knowledgeFact,
} from "../../test/knowledgeFixtures";
import { callsTo, lastBody, mockKnowledgeApi } from "../../test/knowledgeRoutes";
import { renderAt } from "../../test/render";
import { renderWithProfile } from "../../test/visibility";
import { FactDetailPage } from "./FactDetailPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const GET_FACT = "GET /api/v1/knowledge/facts/{id}";
const LIST = "GET /api/v1/knowledge/facts";
const ACCEPT = "POST /api/v1/knowledge/facts/{id}/accept";
const VERIFY = "POST /api/v1/knowledge/facts/{id}/verify";
const RETRACT = "POST /api/v1/knowledge/facts/{id}/retract";

const byId = (id: string) => {
  const found = FACTS_PAGE.items.find((fact) => fact.id === id);
  if (!found) {
    throw new Error(`no fixture fact ${id}`);
  }
  return found;
};
const role = byId("fact-role");
const inferredRole = byId("fact-inferred-role");
const inferredCidr = byId("fact-inferred-cidr");
const critA = byId("fact-crit-a");
const critB = byId("fact-crit-b");

/** Serves `fact` at its id; other lists answer `others` (the siblings) or nothing. */
function serve(fact = role, others: typeof FACTS_PAGE.items = []) {
  route(GET_FACT, ({ params }) =>
    params?.path?.id === fact.id ? fact : knowledgeFact({ id: "mine" }),
  );
  route(LIST, ({ params }) =>
    params?.query?.status === "superseded"
      ? { items: [], next_cursor: null }
      : { items: [fact, ...others], next_cursor: null },
  );
}

beforeEach(() => {
  resetApi();
  mockKnowledgeApi();
  route(ACCEPT, () => knowledgeFact({ id: "mine" }));
  route(VERIFY, () => knowledgeFact());
  route(RETRACT, () => knowledgeFact({ status: "retracted" }));
});

function show(fact = role, url?: string) {
  return renderAt(url ?? `/knowledge/facts/${fact.id}`, "/knowledge/facts/:id", <FactDetailPage />);
}

test("fact_detail_shows_status_source_value_and_where_it_came_from", async () => {
  serve(role);
  show(role);

  expect(await screen.findByRole("heading", { level: 1, name: "Role: nas" })).toBeVisible();
  expect(screen.getByText("Current")).toBeVisible();
  expect(screen.getAllByText("You").length).toBeGreaterThan(0);
  expect(screen.getByText("media server")).toBeVisible();
  const origin = screen.getByText("Recorded by").closest("div") as HTMLElement;
  expect(await within(origin).findByText("You")).toBeVisible();
  expect(screen.getByRole("link", { name: "note:note-1" })).toHaveAttribute(
    "href",
    "/notes/note-1",
  );
  for (const label of ["Observed", "Last verified", "Fresh until", "Valid until", "Recorded"]) {
    expect(screen.getByText(label)).toBeVisible();
  }
});

test("fact_detail_for_each_status_shows_the_right_notes_and_actions", async () => {
  const cases = [
    {
      fact: role,
      buttons: ["Verify", "Retract"],
      absent: ["Make it mine", "Use this value"],
    },
    {
      fact: inferredRole,
      buttons: ["Make it mine", "Verify", "Retract"],
      absent: ["Use this value"],
    },
    { fact: critA, buttons: ["Use this value", "Verify", "Retract"], absent: ["Make it mine"] },
    {
      fact: knowledgeFact({ id: "old", status: "superseded", stored_status: "superseded" }),
      buttons: [],
      absent: ["Verify", "Retract", "Make it mine"],
      note: "A newer value replaced this one. See History.",
    },
    {
      fact: FACT_RETRACTED,
      buttons: [],
      absent: ["Verify", "Retract", "Make it mine"],
      note: /^Retracted .*by You\.$/,
    },
    {
      fact: FACT_PURGED,
      buttons: [],
      absent: ["Verify", "Retract"],
      note: "The content was removed after the retention period. Only this record remains.",
    },
  ];
  for (const { fact, buttons, absent, note } of cases) {
    resetApi();
    mockKnowledgeApi();
    serve(fact);
    const view = show(fact);
    await screen.findByRole("heading", { level: 1, name: /: / });
    for (const name of buttons) {
      expect(await screen.findByRole("button", { name })).toBeVisible();
    }
    for (const name of absent) {
      expect(screen.queryByRole("button", { name })).toBeNull();
    }
    if (note) {
      expect(await screen.findByText(note)).toBeVisible();
    }
    view.unmount();
  }
});

test("a_management_path_fact_points_at_the_paths_page_and_offers_no_verify", async () => {
  const path = knowledgeFact({
    id: "path-1",
    predicate: "management_path",
    value: { cidr: "10.8.0.0/24" },
  });
  serve(path);
  show(path);

  expect(await screen.findByText("Confirm this path on the Management paths page.")).toBeVisible();
  expect(await screen.findByRole("link", { name: "Open management paths" })).toHaveAttribute(
    "href",
    "/knowledge/paths?host=nas",
  );
  expect(screen.queryByRole("button", { name: "Verify" })).toBeNull();
  expect(screen.getByText("10.8.0.0/24")).toBeVisible();
});

test("accept_moves_the_page_to_the_new_fact", async () => {
  const user = userEvent.setup();
  serve(inferredRole);
  show(inferredRole);

  await user.click(await screen.findByRole("button", { name: "Make it mine" }));

  expect(
    await screen.findByText("Saved as your own fact. The original stays as history."),
  ).toBeVisible();
  await waitFor(() =>
    expect(screen.getByTestId("location")).toHaveTextContent("/knowledge/facts/mine"),
  );
  expect(lastBody(ACCEPT)).toEqual({ expected_version: inferredRole.version });
});

test("accepting_a_safety_critical_fact_asks_for_the_password_first", async () => {
  const user = userEvent.setup();
  serve(inferredCidr);
  show(inferredCidr);

  await user.click(await screen.findByRole("button", { name: "Make it mine" }));
  const prompt = await screen.findByRole("dialog", { name: "Confirm with your password" });
  expect(callsTo(ACCEPT)).toHaveLength(0);
  await user.type(within(prompt).getByLabelText("Password"), "hunter2");
  await user.click(within(prompt).getByRole("button", { name: "Accept" }));

  await waitFor(() => expect(callsTo(ACCEPT)).toHaveLength(1));
  expect(lastBody(ACCEPT)).toEqual({ expected_version: inferredCidr.version, password: "hunter2" });
});

test("verify_and_retract_send_the_page_version_and_report_conflicts_in_place", async () => {
  const user = userEvent.setup();
  serve(role);
  show(role);

  await user.click(await screen.findByRole("button", { name: "Verify" }));
  expect(await screen.findByText("Marked as checked.")).toBeVisible();
  expect(lastBody(VERIFY)).toEqual({ expected_version: role.version });

  route(VERIFY, () =>
    failure(409, "conflict", "someone changed it meanwhile; reload and try again"),
  );
  const before = callsTo(GET_FACT).length;
  await user.click(screen.getByRole("button", { name: "Verify" }));
  expect(await screen.findByText(/someone changed it meanwhile/i)).toBeVisible();
  await waitFor(() => expect(callsTo(GET_FACT).length).toBeGreaterThan(before));

  await user.click(screen.getByRole("button", { name: "Retract" }));
  const dialog = await screen.findByRole("dialog", { name: "Retract Role for nas?" });
  expect(within(dialog).queryByLabelText("Password")).toBeNull();
  route(RETRACT, () => failure(409, "conflict", "it is not live any more"));
  await user.click(within(dialog).getByRole("button", { name: "Retract" }));
  expect(await within(dialog).findByText(/it is not live any more/i)).toBeVisible();

  route(RETRACT, () => knowledgeFact({ status: "retracted" }));
  await user.click(within(dialog).getByRole("button", { name: "Retract" }));
  expect(await screen.findByText("Retracted. It is no longer used.")).toBeVisible();
  expect(lastBody(RETRACT)).toEqual({ expected_version: role.version });
});

test("siblings_offer_use_this_value_only_when_disputed", async () => {
  const user = userEvent.setup();
  const calm = knowledgeFact({
    id: "calm",
    predicate: "criticality",
    value: "info",
    source: "user",
  });
  serve(critA, [critB, calm]);
  show(critA);

  const list = await screen.findByRole("list", { name: "Other values" });
  const rows = within(list).getAllByRole("listitem");
  expect(rows).toHaveLength(2);
  const disputedRow = rows.find((row) => within(row).queryByText("warning")) as HTMLElement;
  const calmRow = rows.find((row) => within(row).queryByText("info")) as HTMLElement;
  expect(within(calmRow).queryByRole("button")).toBeNull();
  const query = callsTo(LIST).find((call) => call.init.params?.query?.subject)?.init.params?.query;
  expect(query).toMatchObject({ subject: critA.subject, predicate: "criticality" });
  expect(query).not.toHaveProperty("status");

  await user.click(within(disputedRow).getByRole("button", { name: "Use this value" }));
  await waitFor(() => expect(lastBody(ACCEPT)).toEqual({ expected_version: critB.version }));
});

test("a_failed_siblings_load_is_a_load_error_not_nothing", async () => {
  serve(role);
  route(LIST, () => failure(500, "internal", "boom"));
  show(role);

  expect(await screen.findByText(/load the other values/i)).toBeVisible();
  expect(screen.getAllByRole("button", { name: "Retry" }).length).toBeGreaterThan(0);
});

test("history_loads_only_when_opened", async () => {
  const user = userEvent.setup();
  const older = knowledgeFact({
    id: "older",
    value: "file server",
    status: "superseded",
    stored_status: "superseded",
  });
  serve(role);
  route(LIST, ({ params }) =>
    params?.query?.status === "superseded"
      ? { items: [older], next_cursor: null }
      : { items: [role], next_cursor: null },
  );
  renderWithProfile(
    "standard",
    `/knowledge/facts/${role.id}`,
    "/knowledge/facts/:id",
    <FactDetailPage />,
  );

  await screen.findByRole("heading", { level: 1, name: "Role: nas" });
  const superseded = () =>
    callsTo(LIST).filter((call) => call.init.params?.query?.status === "superseded");
  expect(superseded()).toHaveLength(0);

  await user.click(screen.getByRole("button", { name: "History" }));

  expect(await screen.findByText("file server")).toBeVisible();
  expect(superseded()).toHaveLength(1);
  expect(superseded()[0]?.init.params?.query).toMatchObject({
    subject: role.subject,
    predicate: "role",
  });
});

test("no_such_fact_says_so_with_a_link_back", async () => {
  route(GET_FACT, () => failure(404, "not_found", "no such fact"));
  show(role);

  expect(await screen.findByText("No such fact")).toBeVisible();
  expect(screen.getByRole("link", { name: "Back to Knowledge" })).toHaveAttribute(
    "href",
    "/knowledge",
  );
});

test("a_failed_fact_load_is_a_load_error", async () => {
  route(GET_FACT, () => failure(500, "internal", "boom"));
  show(role);

  expect(await screen.findByRole("button", { name: "Retry" })).toBeVisible();
  expect(screen.queryByText("No such fact")).toBeNull();
});
