import { screen, within } from "@testing-library/react";
import { userEvent } from "@testing-library/user-event";
import { calls, resetApi, route } from "../../test/apiMock";
import { action, nas } from "../../test/fixtures";
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
  enabled: true,
  ...extra,
});

beforeEach(resetApi);

/** The skill-catalogue requests made so far. */
const skillCalls = () => calls.filter((c) => c.route === "GET /api/v1/hosts/{name}/skills");

/** The skill switches saved so far. */
const switches = () => calls.filter((c) => c.route === "PUT /api/v1/hosts/{name}/skills/{skill}");

test("skills_page_explains_which_skills_need_approval", async () => {
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/hosts/{name}/skills", () => []);
  renderAt("/skills", "/skills", <SkillsPage />);

  expect(await screen.findByText(/^Scripts an agent can run\./)).toBeVisible();
  // Says what a write skill means.
  expect(
    screen.getByText(/Write and destructive skills change things and need your approval\./),
  ).toBeVisible();
  expect(
    screen.getByText(/Disabled skills stay on the host but Domovoi never runs or suggests them\./),
  ).toBeVisible();
  expect(
    screen.getByRole("link", { name: "Learn more about skills (opens in a new tab)" }),
  ).toHaveAttribute("href", "https://RoastSlav.github.io/domovoi/guide/skills.html");
});

test("skills_follow_the_host_selected_in_the_sidebar", async () => {
  route("GET /api/v1/hosts", () => [nas, { ...nas, id: "host-pi", name: "pi" }]);
  route("GET /api/v1/hosts/{name}/skills", () => [skill("docker_ps", "read")]);
  renderAt("/skills?host=pi", "/skills", <SkillsPage />);

  await screen.findByText("docker_ps");
  expect(skillCalls().map((c) => c.init.params?.path)).toEqual([{ name: "pi" }]);
  expect(screen.queryByLabelText("Host"), "the sidebar owns the host choice").toBeNull();
  expect(screen.queryByText(/Showing the skills of/)).toBeNull();
});

test("skills_show_the_first_host_when_all_hosts_are_selected", async () => {
  route("GET /api/v1/hosts", () => [nas, { ...nas, id: "host-pi", name: "pi" }]);
  route("GET /api/v1/hosts/{name}/skills", () => [skill("docker_ps", "read")]);
  renderAt("/skills", "/skills", <SkillsPage />);

  await screen.findByText("docker_ps");
  expect(skillCalls().map((c) => c.init.params?.path)).toEqual([{ name: "nas" }]);
  expect(screen.getByText(/Showing the skills of nas/)).toBeVisible();
});

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

test("skill_detail_dialog_opens_with_parameters_and_readme", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/hosts/{name}/skills", () => [
    skill("restart_container", "write", {
      params: [
        {
          name: "name",
          description: "Container to restart",
          required: true,
          kind: { type: "integer", min: 1, max: 30 },
        },
      ],
      manifest_hash: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789ab",
      verify: { recheck: true, delay_secs: 600 },
      readme: "# Restart container\n\nRestarts one container managed by compose.",
    }),
  ]);
  route("GET /api/v1/actions", () => ({ items: [action()], next_cursor: null }));
  renderAt("/skills", "/skills", <SkillsPage />);

  await user.click(await screen.findByRole("button", { name: "restart_container" }));
  const dialog = await screen.findByRole("dialog", { name: "restart_container" });
  expect(
    within(dialog).getByText(/^Risk write: Changes something\. Runs only after you approve it\./),
  ).toBeVisible();
  expect(within(dialog).getByText(/Container to restart/)).toBeVisible();
  expect(within(dialog).getByText(/min 1, max 30/)).toBeVisible();
  expect(within(dialog).getByText(/^required$/)).toBeVisible();
  expect(within(dialog).getByText(/Restarts one container managed by compose\./)).toBeVisible();
  expect(within(dialog).getByText(/re-runs the check after 600 s/)).toBeVisible();
  const hashCell = dialog.querySelector("code[title]");
  expect(hashCell).toHaveTextContent(/^0123456789ab$/);
  expect(hashCell).toHaveAttribute(
    "title",
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789ab",
  );
  expect(within(dialog).getByRole("link", { name: "inc-1" })).toHaveAttribute(
    "href",
    "/incidents/inc-1",
  );
  expect(within(dialog).getByText(/proposed/)).toBeVisible();
});

test("skill_detail_dialog_says_no_readme_when_absent", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/hosts/{name}/skills", () => [
    skill("disk_usage", "read", { manifest_hash: "f".repeat(64) }),
  ]);
  route("GET /api/v1/actions", () => ({ items: [], next_cursor: null }));
  renderAt("/skills", "/skills", <SkillsPage />);

  await user.click(await screen.findByRole("button", { name: "disk_usage" }));
  const dialog = await screen.findByRole("dialog", { name: "disk_usage" });
  expect(
    within(dialog).getByText(/^Risk read: Only looks\. Triage may run it on its own\./),
  ).toBeVisible();
  expect(within(dialog).getByText("No README")).toBeVisible();
  expect(within(dialog).getByText(/No runs yet\./)).toBeVisible();

  await user.tab();
  await user.keyboard("{Escape}");
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("skills_disabled_skill_stays_listed_greyed_and_tagged", async () => {
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/hosts/{name}/skills", () => [
    skill("disk_usage", "read"),
    skill("restart_container", "write", { enabled: false }),
  ]);
  renderAt("/skills", "/skills", <SkillsPage />);

  const groups = await screen.findAllByRole("region");
  expect(groups.map((g) => g.getAttribute("aria-label"))).toEqual(["Read skills", "Write skills"]);
  const [read, write] = groups as [HTMLElement, HTMLElement];
  const [on] = within(read).getAllByRole("listitem") as [HTMLElement];
  const [off] = within(write).getAllByRole("listitem") as [HTMLElement];

  expect(within(read).getByText("disk_usage")).toBeVisible();
  expect(within(write).getByText("restart_container")).toBeVisible();
  expect(on.getAttribute("data-disabled")).toBeNull();
  expect(off).toHaveAttribute("data-disabled", "true");
  expect(within(off).getByText("Disabled")).toBeVisible();
});

test("switching_a_skill_off_on_its_card_saves_the_change", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/hosts/{name}/skills", () => [skill("docker_ps", "read")]);
  route("PUT /api/v1/hosts/{name}/skills/{skill}", () => undefined);
  renderAt("/skills", "/skills", <SkillsPage />);

  await screen.findByText("docker_ps");
  const [card] = (await screen.findAllByRole("listitem")) as [HTMLElement];
  const checkbox = within(card).getByRole("checkbox");
  expect(checkbox).toBeChecked();

  await user.click(checkbox);
  expect(switches().map((c) => c.init.body)).toEqual([{ enabled: false }]);
  expect(switches().map((c) => c.init.params?.path)).toEqual([{ name: "nas", skill: "docker_ps" }]);
});
