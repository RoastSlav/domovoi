import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, test, vi } from "vitest";
import type { OperationSummary, SimilarOperations as Similar } from "../../api/types";
import { HISTORICAL_LABEL } from "../../design/knowledgeText";
import type { ProfileId } from "../../design/visibility";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import summaryReady from "../../test/fixtures/experience/summary-ready.json";
import similarJson from "../../test/fixtures/experience/similar.json";
import { detail, nas } from "../../test/fixtures";
import { operationDetail } from "../../test/operationFixtures";
import { showOperation } from "../../test/operationPage";
import { renderAt } from "../../test/render";
import { renderWithProfile } from "../../test/visibility";
import { IncidentDetailPage } from "../incidents/IncidentDetailPage";
import { SimilarOperations } from "./SimilarOperations";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const SIMILAR = similarJson as unknown as Similar;
const OP_SIMILAR = "GET /api/v1/operations/{id}/similar";
const INCIDENT_SIMILAR = "GET /api/v1/incidents/{id}/similar-operations";
const callsTo = (key: string) => calls.filter((call) => call.route === key);

beforeEach(() => {
  resetApi();
  sessionStorage.clear();
  route("GET /api/v1/hosts", () => [nas]);
});

function show(profile: ProfileId = "detailed") {
  return renderWithProfile(
    profile,
    "/operations/op-1",
    "/operations/:id",
    <SimilarOperations source={{ operation: "op-1" }} />,
  );
}

test("similar_operations_show_reason_tags_and_do_not_load_until_opened_in_standard", async () => {
  const user = userEvent.setup();
  route(OP_SIMILAR, () => SIMILAR);
  show("standard");
  const toggle = await screen.findByRole("button", { name: /Similar past operations/ });
  expect(callsTo(OP_SIMILAR)).toHaveLength(0);
  await user.click(toggle);
  const group = await screen.findByRole("group", { name: "Similar past operations" });
  expect(await within(group).findByText("Same check")).toBeVisible();
  expect(within(group).getByText("Same target")).toBeVisible();
  expect(within(group).getByText("Same action: skill.restart_container")).toBeVisible();
  expect(callsTo(OP_SIMILAR)).toHaveLength(1);
  expect(callsTo(OP_SIMILAR)[0]?.init.params?.path).toEqual({ id: "op-1" });
});

test("detailed_requests_similar_operations_at_once_and_repeats_the_label_sentence", async () => {
  route(OP_SIMILAR, () => SIMILAR);
  show("detailed");
  expect(await screen.findByText("Same check")).toBeVisible();
  expect(screen.getByRole("heading", { name: "Similar past operations" })).toBeVisible();
  expect(screen.getByText(new RegExp(`^${HISTORICAL_LABEL}`, "i"))).toBeVisible();
  expect(SIMILAR.label).toBe(HISTORICAL_LABEL);
});

test("an_unknown_reason_code_shows_as_it_came_and_a_failure_is_not_an_empty_list", async () => {
  route(OP_SIMILAR, () => ({
    ...SIMILAR,
    items: [{ ...SIMILAR.items[0], reasons: [{ code: "same_weather", detail: null }] }],
  }));
  const view = show();
  expect(await screen.findByText("same_weather")).toBeVisible();
  view.unmount();

  route(OP_SIMILAR, () => failure(503, "unavailable", "down"));
  show();
  expect(await screen.findByRole("button", { name: "Retry" })).toBeVisible();
  expect(screen.queryByText("No similar past operations yet.")).toBeNull();
});

test("no_similar_operations_says_so", async () => {
  route(OP_SIMILAR, () => ({ ...SIMILAR, items: [] }));
  show();
  expect(await screen.findByText("No similar past operations yet.")).toBeVisible();
});

const investigation = {
  id: "op-1",
  kind: "investigate",
  goal: "find out why plex keeps going down",
  status: "waiting_approval",
  waiting_for: ["approval"],
  cycle: 0,
  incidents: ["inc-1"],
  hosts: [nas.id],
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
} as OperationSummary;

test("the_incident_page_shows_similar_operations_only_when_it_has_investigations", async () => {
  route("GET /api/v1/incidents/{id}/followups", () => ({ turns: [] }));
  route("GET /api/v1/settings", () => ({ triage: { enabled: true } }));
  route(INCIDENT_SIMILAR, () => SIMILAR);
  route("GET /api/v1/incidents/{id}", () => detail());
  const view = renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);
  await screen.findByRole("heading", { level: 1 });
  expect(screen.queryByRole("heading", { name: "Similar past operations" })).toBeNull();
  expect(callsTo(INCIDENT_SIMILAR)).toHaveLength(0);
  view.unmount();

  route("GET /api/v1/incidents/{id}", () => ({ ...detail(), operations: [investigation] }));
  renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);
  expect(await screen.findByRole("heading", { name: "Similar past operations" })).toBeVisible();
  expect(await screen.findByText("Same check")).toBeVisible();
  expect(callsTo(INCIDENT_SIMILAR)[0]?.init.params?.path).toEqual({ id: "inc-1" });
});

test("the_operation_page_mounts_the_summary_only_for_a_finished_operation", async () => {
  const base = operationDetail("verified-success");
  const withStatus = (status: string) => ({ ...base, operation: { ...base.operation, status } });
  const SUMMARY = "GET /api/v1/operations/{id}/summary";
  for (const [status, expected] of [
    ["succeeded", 1],
    ["running", 0],
    ["a_status_from_the_future", 0],
  ] as const) {
    resetApi();
    const detailFor = withStatus(status);
    const view = showOperation(detailFor, {
      summary: () => summaryReady,
      similar: () => SIMILAR,
    });
    await screen.findByRole("heading", { level: 1, name: base.operation.goal });
    await screen.findByRole("heading", { name: "Similar past operations" });
    await waitFor(() => expect(callsTo(SUMMARY)).toHaveLength(expected));
    if (expected === 1) {
      expect(await screen.findByRole("heading", { name: "Resolution summary" })).toBeVisible();
    } else {
      expect(screen.queryByRole("heading", { name: "Resolution summary" })).toBeNull();
    }
    view.unmount();
  }
});
