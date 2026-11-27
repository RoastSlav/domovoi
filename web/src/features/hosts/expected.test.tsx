import { screen } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import { resetApi, route } from "../../test/apiMock";
import { renderAt } from "../../test/render";
import { ExpectedTab } from "./ExpectedTab";

vi.mock("../../api/client", () => import("../../test/apiMock"));

beforeEach(() => resetApi());

test("expected_events_show_what_happened_and_why_it_was_fine", async () => {
  route("GET /api/v1/expected-signals", () => [
    {
      id: "s1",
      host: "nas",
      check: "security.new_port",
      target: "tcp:0.0.0.0:22",
      summary: "new listener 0.0.0.0:22",
      observed_at: "2026-11-01T10:00:00Z",
      rule_id: "r1",
      rule_note: "ssh is meant to listen",
    },
  ]);
  renderAt("/hosts/nas", "/hosts/:name", <ExpectedTab host="nas" />);
  expect(await screen.findByText("new listener 0.0.0.0:22")).toBeVisible();
  expect(screen.getByText("ssh is meant to listen")).toBeVisible();
});

test("no_expected_events_says_nothing_was_raised_for_them", async () => {
  route("GET /api/v1/expected-signals", () => []);
  renderAt("/hosts/nas", "/hosts/:name", <ExpectedTab host="nas" />);
  expect(await screen.findByText("No expected events on nas")).toBeVisible();
});
