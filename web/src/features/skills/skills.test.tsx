import { screen, within } from "@testing-library/react";
import { userEvent } from "@testing-library/user-event";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { action, nas } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { SkillDetail } from "./SkillDetail";
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
  manifest_hash: "f".repeat(64),
  ...extra,
});

beforeEach(resetApi);

/** The skill-catalogue requests made so far. */
const skillCalls = () => calls.filter((c) => c.route === "GET /api/v1/hosts/{name}/skills");

/** The skill switches saved so far. */
const switches = () => calls.filter((c) => c.route === "PUT /api/v1/hosts/{name}/skills/{skill}");

/** The row of the skills table whose name is `name`. */
const rowOf = (name: string) =>
  screen.getByRole("button", { name }).closest("tr") as HTMLTableRowElement;

test("skills_page_explains_which_skills_need_approval", async () => {
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/hosts/{name}/skills", () => []);
  renderAt("/skills", "/skills", <SkillsPage />);

  expect(await screen.findByText(/^Scripts an agent can run\./)).toBeVisible();
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

test("skills_under_all_hosts_ask_for_a_host_and_never_show_the_first_one", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/hosts", () => [nas, { ...nas, id: "host-pi", name: "pi" }]);
  route("GET /api/v1/hosts/{name}/skills", () => [skill("docker_ps", "read")]);
  renderAt("/skills", "/skills", <SkillsPage />);

  expect(await screen.findByText("Choose a host to see its skills.")).toBeVisible();
  const hosts = screen.getByRole("list", { name: "Hosts" });
  expect(within(hosts).getByRole("link", { name: "nas" })).toHaveAttribute(
    "href",
    "/skills?host=nas",
  );
  expect(skillCalls(), "no host is guessed").toEqual([]);

  await user.click(within(hosts).getByRole("link", { name: "pi" }));
  await screen.findByText("docker_ps");
  expect(skillCalls().map((c) => c.init.params?.path)).toEqual([{ name: "pi" }]);
});

test("skills_page_says_when_hosts_cannot_load", async () => {
  route("GET /api/v1/hosts", () => failure(503, "unavailable", "the hub is starting"));
  renderAt("/skills", "/skills", <SkillsPage />);

  expect(await screen.findByText("Could not load hosts")).toBeVisible();
  expect(screen.getByRole("button", { name: "Retry" })).toBeVisible();
});

test("skills_list_shows_an_error_with_retry_and_not_an_empty_state_when_it_cannot_load", async () => {
  const user = userEvent.setup();
  let failing = true;
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/hosts/{name}/skills", () =>
    failing ? failure(503, "unavailable", "the hub is starting") : [skill("docker_ps", "read")],
  );
  renderAt("/skills?host=nas", "/skills", <SkillsPage />);

  expect(await screen.findByText("Could not load skills")).toBeVisible();
  expect(screen.queryByText("No skills")).toBeNull();

  failing = false;
  await user.click(screen.getByRole("button", { name: "Retry" }));
  expect(await screen.findByText("docker_ps")).toBeVisible();
});

test("skills_list_says_why_it_is_empty_and_what_to_check", async () => {
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/hosts/{name}/skills", () => []);
  renderAt("/skills?host=nas", "/skills", <SkillsPage />);

  expect(await screen.findByText("No skills")).toBeVisible();
  expect(screen.getByText(/nas has not reported any skills\./)).toBeVisible();
  expect(screen.getByText(/check that nas is connected on the Hosts page/)).toBeVisible();
});

test("skills_list_shows_risk_timeout_and_applies_to_in_columns_ordered_by_risk", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/hosts/{name}/skills", () => [
    skill("prune_snapshots", "destructive", { timeout_secs: 300 }),
    skill("disk_usage", "read", { applies_to: ["disk.usage"] }),
    skill("restart_container", "write"),
    skill("docker_ps", "read"),
  ]);
  renderAt("/skills?host=nas", "/skills", <SkillsPage />);

  await screen.findByText("docker_ps");
  const headers = screen.getAllByRole("columnheader").map((h) => h.textContent);
  expect(headers.slice(0, 5)).toEqual(["Skill", "Description", "Risk", "Timeout", "Applies to"]);
  const names = screen
    .getAllByRole("row")
    .slice(1)
    .map((row) => within(row).getByRole("button", { name: /^[a-z_]+$/ }).textContent);
  expect(names).toEqual(["disk_usage", "docker_ps", "restart_container", "prune_snapshots"]);

  const disk = rowOf("disk_usage");
  expect(within(disk).getByText("Does disk_usage")).toBeVisible();
  expect(within(disk).getByText("Read")).toBeVisible();
  expect(within(disk).getByText("disk.usage")).toBeVisible();
  expect(within(rowOf("docker_ps")).getByText("Any check")).toBeVisible();
  expect(within(rowOf("restart_container")).getByText("Write")).toBeVisible();
  const prune = rowOf("prune_snapshots");
  expect(within(prune).getByText("Destructive")).toBeVisible();
  expect(within(prune).getByText("300 s")).toBeVisible();

  await user.click(screen.getByRole("button", { name: "About Risk" }));
  expect(
    screen.getByText(/Only a signed-in person can approve a write or destructive skill/),
  ).toBeVisible();
});

test("skills_list_shows_when_each_skill_last_ran_and_never_for_those_that_did_not", async () => {
  const fiveMinutesAgo = new Date(Date.now() - 5 * 60_000).toISOString();
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/hosts/{name}/skills", () => [
    skill("docker_ps", "read", { last_used: fiveMinutesAgo }),
    skill("disk_usage", "read", { last_used: null }),
  ]);
  renderAt("/skills?host=nas", "/skills", <SkillsPage />);

  await screen.findByText("docker_ps");
  expect(screen.getByRole("columnheader", { name: /last used/i })).toBeVisible();
  expect(within(rowOf("docker_ps")).getByText("5 minutes ago")).toBeVisible();
  expect(within(rowOf("disk_usage")).getByText("Never")).toBeVisible();
});

test("skill_detail_opens_from_the_list_with_parameters_readme_and_runs", async () => {
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
  renderAt("/skills?host=nas", "/skills", <SkillsPage />);

  await user.click(await screen.findByRole("button", { name: "restart_container" }));
  const dialog = await screen.findByRole("dialog", { name: "restart_container" });
  expect(
    within(dialog).getByText(/^Changes something\. Runs only after you approve it\./),
  ).toBeVisible();

  const params = within(dialog).getByRole("table", { name: "Parameters of restart_container" });
  const headers = within(params)
    .getAllByRole("columnheader")
    .map((h) => h.textContent);
  expect(headers).toEqual(["Name", "Type", "Constraints", "Description"]);
  expect(within(params).getByText("integer")).toBeVisible();
  expect(within(params).getByText("min 1, max 30")).toBeVisible();
  expect(within(params).getByText("Required")).toBeVisible();
  expect(within(params).getByText("Container to restart")).toBeVisible();

  expect(
    await within(dialog).findByText(/Restarts one container managed by compose\./),
  ).toBeVisible();
  expect(within(dialog).getByText(/Re-runs the check after 600 s/)).toBeVisible();
  expect(within(dialog).getByText("0123456789ab")).toHaveAccessibleDescription(
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789ab",
  );
  expect(await within(dialog).findByRole("link", { name: "inc-1" })).toHaveAttribute(
    "href",
    "/incidents/inc-1",
  );
  expect(within(dialog).getByText("Waiting for approval")).toBeVisible();
});

test("skill_detail_says_when_there_is_no_readme_and_no_runs_and_closes_on_escape", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/hosts/{name}/skills", () => [skill("disk_usage", "read")]);
  route("GET /api/v1/actions", () => ({ items: [], next_cursor: null }));
  renderAt("/skills?host=nas", "/skills", <SkillsPage />);

  await user.click(await screen.findByRole("button", { name: "disk_usage" }));
  const dialog = await screen.findByRole("dialog", { name: "disk_usage" });
  expect(within(dialog).getByText(/^Only looks\. Triage may run it on its own\./)).toBeVisible();
  expect(within(dialog).getByText("This skill has no README.")).toBeVisible();
  expect(within(dialog).getByText("This skill takes no parameters.")).toBeVisible();
  expect(await within(dialog).findByText("No runs yet")).toBeVisible();

  await user.keyboard("{Escape}");
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("skill_detail_shows_a_loading_state_while_the_skill_loads", async () => {
  route("GET /api/v1/hosts/{name}/skills", () => new Promise(() => undefined));
  renderAt(
    "/skills?host=nas",
    "/skills",
    <SkillDetail host="nas" name="disk_usage" onClose={() => undefined} />,
  );

  const dialog = await screen.findByRole("dialog", { name: "disk_usage" });
  expect(within(dialog).getByRole("status")).toHaveTextContent("Loading…");
  expect(within(dialog).queryByText("Skill not found")).toBeNull();
});

test("skill_detail_shows_an_error_with_retry_when_the_request_fails", async () => {
  const user = userEvent.setup();
  let failing = true;
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/actions", () => ({ items: [], next_cursor: null }));
  route("GET /api/v1/hosts/{name}/skills", () =>
    failing ? failure(503, "unavailable", "the hub is starting") : [skill("disk_usage", "read")],
  );
  renderAt(
    "/skills?host=nas",
    "/skills",
    <SkillDetail host="nas" name="disk_usage" onClose={() => undefined} />,
  );

  const dialog = await screen.findByRole("dialog", { name: "disk_usage" });
  expect(await within(dialog).findByText("Could not load the skill")).toBeVisible();

  failing = false;
  await user.click(within(dialog).getByRole("button", { name: "Retry" }));
  expect(await within(dialog).findByText("This skill has no README.")).toBeVisible();
});

test("skills_disabled_skill_stays_listed_quiet_and_tagged", async () => {
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/hosts/{name}/skills", () => [
    skill("disk_usage", "read"),
    skill("restart_container", "write", { enabled: false }),
  ]);
  renderAt("/skills?host=nas", "/skills", <SkillsPage />);

  await screen.findByText("disk_usage");
  const off = rowOf("restart_container");
  expect(within(off).getByText("Disabled")).toBeVisible();
  expect(off).toHaveAttribute("data-state", "quiet");
  expect(within(off).getByRole("switch", { name: "Enable restart_container" })).not.toBeChecked();
  expect(within(rowOf("disk_usage")).queryByText("Disabled")).toBeNull();
});

test("switching_a_skill_off_in_its_row_saves_the_change", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/hosts/{name}/skills", () => [skill("docker_ps", "read")]);
  route("PUT /api/v1/hosts/{name}/skills/{skill}", () => undefined);
  renderAt("/skills?host=nas", "/skills", <SkillsPage />);

  await screen.findByText("docker_ps");
  const toggle = within(rowOf("docker_ps")).getByRole("switch", { name: "Enable docker_ps" });
  expect(toggle).toBeChecked();

  await user.click(toggle);
  expect(switches().map((c) => c.init.body)).toEqual([{ enabled: false }]);
  expect(switches().map((c) => c.init.params?.path)).toEqual([{ name: "nas", skill: "docker_ps" }]);
});
