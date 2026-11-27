import { screen } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import type { OperationSummary } from "../../api/types";
import { resetApi } from "../../test/apiMock";
import { renderAt } from "../../test/render";
import { IncidentOperations } from "./IncidentOperations";

vi.mock("../../api/client", () => import("../../test/apiMock"));

beforeEach(resetApi);

function operation(id: string, status: string): OperationSummary {
  return {
    id,
    goal: `Goal ${id}`,
    status,
    updated_at: "2026-11-01T10:00:00Z",
  } as OperationSummary;
}

test("a_question_waiting_for_a_person_is_said_on_the_incident_with_a_link_to_answer_it", () => {
  renderAt(
    "/incidents/inc-1",
    "/incidents/:id",
    <IncidentOperations operations={[operation("o1", "waiting_user")]} />,
  );
  expect(screen.getByText("Triage is waiting for your answer")).toBeVisible();
  expect(screen.getByRole("link", { name: "Read the question and answer it" })).toHaveAttribute(
    "href",
    "/operations/o1",
  );
});

test("no_banner_when_nothing_waits_for_an_answer", () => {
  renderAt(
    "/incidents/inc-1",
    "/incidents/:id",
    <IncidentOperations operations={[operation("o1", "running")]} />,
  );
  expect(screen.queryByText("Triage is waiting for your answer")).toBeNull();
});
