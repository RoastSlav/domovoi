import { expect, test } from "vitest";
import type { IncidentDetail } from "../../api/types";
import { investigationState } from "./WhatsGoingOn";

const detail = (...statuses: string[]) =>
  ({
    operations: statuses.map((status, i) => ({ id: `o${i}`, kind: "investigate", status })),
  }) as unknown as IncidentDetail;

test("an_open_investigation_is_said_instead_of_triage_has_not_run", () => {
  expect(investigationState(detail("running"))).toBe("Triage is running.");
  expect(investigationState(detail("waiting_user"))).toMatch(/waiting for your answer/);
  expect(investigationState(detail("waiting_timer"))).toMatch(/model is unreachable/);
  expect(investigationState(detail("queued"))).toMatch(/queued/);
  expect(investigationState(detail("paused"))).toBe("Triage is paused.");
});

test("a_finished_or_absent_investigation_says_nothing", () => {
  expect(investigationState(detail("succeeded", "failed", "cancelled"))).toBeUndefined();
  expect(investigationState(detail())).toBeUndefined();
});
