import { screen, within } from "@testing-library/react";
import { calls, resetApi, route } from "../../test/apiMock";
import { nas } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { SkillsPage } from "./SkillsPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const skill = (name: string, risk: string, extra = {}) => ({
  name,
  description: `Does ${name}`,
  risk,
  params: [],
  applies_to: [],
  timeout_secs: 60,
  ...extra,
});

beforeEach(resetApi);

test("skills_grouped_by_risk_with_destructive_marked", async () => {
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/hosts/{name}/skills", () => [
    skill("prune_snapshots", "destructive", {
      params: [
        {
          name: "keep",
          description: "Snapshots to keep",
          required: true,
          kind: { type: "integer", min: 1, max: 30 },
        },
      ],
    }),
    skill("disk_usage", "read", { applies_to: ["disk.usage"] }),
    skill("restart_container", "write"),
    skill("docker_ps", "read"),
  ]);
  renderAt("/skills", "/skills", <SkillsPage />);

  const groups = await screen.findAllByRole("region");
  expect(groups.map((g) => g.getAttribute("aria-label"))).toEqual([
    "Read skills",
    "Write skills",
    "Destructive skills",
  ]);
  const [read, write, destructive] = groups as [HTMLElement, HTMLElement, HTMLElement];
  expect(
    within(read)
      .getAllByRole("heading", { level: 4 })
      .map((h) => h.textContent),
  ).toEqual(["disk_usage", "docker_ps"]);
  expect(within(read).getByText(/Applies to disk.usage/)).toBeVisible();
  expect(within(write).getByText("restart_container")).toBeVisible();
  expect(within(write).queryByText("Destructive")).toBeNull();

  const [prune] = within(destructive).getAllByRole("listitem") as [HTMLElement];
  expect(prune).toHaveAttribute("data-risk", "destructive");
  expect(within(prune).getByText("Destructive")).toBeVisible();
  expect(
    within(prune).getByRole("list", { name: "Parameters of prune_snapshots" }),
  ).toHaveTextContent("keep integer (min 1, max 30), required — Snapshots to keep");
  expect(calls.at(-1)?.init.params?.path).toEqual({ name: "nas" });
});
