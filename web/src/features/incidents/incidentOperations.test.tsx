import { screen, within } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import type { OperationSummary } from "../../api/types";
import { resetApi, route } from "../../test/apiMock";
import { detail, nas } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { IncidentDetailPage } from "./IncidentDetailPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

beforeEach(() => {
  resetApi();
  sessionStorage.clear();
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/incidents/{id}/followups", () => ({ turns: [] }));
  route("GET /api/v1/settings", () => ({ triage: { enabled: true } }));
});

const operation = {
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

test("incident_page_lists_its_operations_and_nothing_when_there_are_none", async () => {
  route("GET /api/v1/incidents/{id}", () => ({ ...detail(), operations: [operation] }));
  renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);
  const section = await screen.findByRole("region", { name: "Investigations" });
  expect(within(section).getByRole("link", { name: operation.goal })).toHaveAttribute(
    "href",
    "/operations/op-1",
  );
  expect(within(section).getByText("Waiting for approval")).toBeVisible();
});

test("incident_page_has_no_investigations_section_without_operations", async () => {
  route("GET /api/v1/incidents/{id}", () => detail());
  renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);
  await screen.findByRole("heading", { level: 1 });
  expect(screen.queryByRole("region", { name: "Investigations" })).toBeNull();
});
