import { screen, within } from "@testing-library/react";
import { userEvent } from "@testing-library/user-event";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { nas } from "../../test/fixtures";
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
  const user = userEvent.setup();
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/hosts/{name}/skills", () => []);
  renderAt("/skills", "/skills", <SkillsPage />);

  expect(
    await screen.findByText(
      /^Scripts an agent can run; write and destructive ones need your approval\./,
    ),
  ).toBeVisible();
  expect(
    screen.getByRole("link", { name: "Learn more about skills (opens in a new tab)" }),
  ).toHaveAttribute("href", "https://RoastSlav.github.io/domovoi/guide/skills.html");

  await user.click(screen.getByRole("button", { name: "About risk levels" }));
  expect(
    screen.getByText(/Only a signed-in person can approve a write or destructive skill/),
  ).toBeVisible();
  expect(
    screen.getByText(/A disabled skill stays on the host but Domovoi never runs or suggests it\./),
  ).toBeVisible();
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
  const hosts = screen.getByRole("navigation", { name: "Hosts" });
  expect(within(hosts).getByRole("link", { name: /^nas / })).toHaveAttribute(
    "href",
    "/skills?host=nas",
  );
  expect(skillCalls(), "no host is guessed").toEqual([]);

  await user.click(within(hosts).getByRole("link", { name: /^pi / }));
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

test("skills_list_groups_skills_by_risk_with_a_pill_and_what_it_allows_per_group", async () => {
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/hosts/{name}/skills", () => [
    skill("prune_snapshots", "destructive", { timeout_secs: 300 }),
    skill("disk_usage", "read", { applies_to: ["disk.usage"] }),
    skill("restart_container", "write"),
    skill("docker_ps", "read"),
  ]);
  renderAt("/skills?host=nas", "/skills", <SkillsPage />);

  await screen.findByText("docker_ps");
  const groups = screen.getAllByRole("region").map((group) => ({
    risk: within(group).getByRole("heading").textContent,
    names: within(group)
      .getAllByRole("button", { name: /^[a-z_]+$/ })
      .map((button) => button.textContent),
  }));
  expect(groups).toEqual([
    { risk: "Read", names: ["disk_usage", "docker_ps"] },
    { risk: "Write", names: ["restart_container"] },
    { risk: "Destructive", names: ["prune_snapshots"] },
  ]);
  const read = screen.getByRole("table", { name: "Read skills on nas" });
  expect(
    within(read)
      .getAllByRole("columnheader")
      .map((h) => h.textContent),
  ).toEqual(["Skill", "Description", "Timeout", "Applies to", "Enabled"]);
  expect(screen.getByText(/^Only looks\. Triage may run it on its own\./)).toBeVisible();

  const disk = rowOf("disk_usage");
  expect(within(disk).getByText("Does disk_usage")).toBeVisible();
  expect(within(disk).getByText("disk.usage")).toBeVisible();
  expect(within(rowOf("docker_ps")).getByText("Any check")).toBeVisible();
  expect(within(rowOf("prune_snapshots")).getByText("300 s")).toBeVisible();
});

test("a_skill_row_opens_the_skill_when_clicked_anywhere", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/hosts/{name}/skills", () => [skill("docker_ps", "read")]);
  renderAt("/skills?host=nas", "/skills", <SkillsPage />);

  await user.click(await screen.findByText("Does docker_ps"));
  expect(await screen.findByRole("dialog", { name: "docker_ps" })).toBeVisible();
});

test("a_hub_only_host_sorts_last_under_all_hosts_and_its_card_is_not_a_link", async () => {
  route("GET /api/v1/hosts", () => [
    {
      ...nas,
      id: "host-hub",
      name: "alpha-hub",
      kind: "local",
      agent_enrolled: false,
      connected: false,
      last_seen: null,
    },
    { ...nas, id: "host-pi", name: "pi" },
    nas,
  ]);
  renderAt("/skills", "/skills", <SkillsPage />);

  const hosts = await screen.findByRole("navigation", { name: "Hosts" });
  const links = within(hosts).getAllByRole("link");
  expect(links).toHaveLength(2);
  expect(hosts.textContent?.indexOf("alpha-hub")).toBeGreaterThan(
    hosts.textContent?.indexOf("pi") ?? 0,
  );
  expect(within(hosts).queryByRole("link", { name: /alpha-hub/ })).toBeNull();
  expect(within(hosts).getByText("No agent here, so no skills.")).toBeVisible();
});

test("a_connected_host_is_listed_before_one_whose_agent_is_offline", async () => {
  route("GET /api/v1/hosts", () => [{ ...nas, id: "host-a", name: "aaa", connected: false }, nas]);
  renderAt("/skills", "/skills", <SkillsPage />);

  const hosts = await screen.findByRole("navigation", { name: "Hosts" });
  expect(hosts.textContent?.indexOf("nas")).toBeLessThan(hosts.textContent?.indexOf("aaa") ?? 0);
});

test("skill_detail_opens_from_the_list_with_parameters_and_readme", async () => {
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

  // The renderer is a separate download; until it arrives the raw text shows in a fallback
  // paragraph, so wait for the rendered heading before reading the body.
  expect(await within(dialog).findByRole("heading", { name: "Restart container" })).toBeVisible();
  expect(within(dialog).getByText("Restarts one container managed by compose.")).toBeVisible();
  expect(within(dialog).getByText(/Re-runs the check after 600 s/)).toBeVisible();
  expect(within(dialog).getByText("0123456789ab")).toHaveAccessibleDescription(
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789ab",
  );
});

test("skill_detail_says_when_there_is_no_readme_and_closes_on_escape", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/hosts/{name}/skills", () => [skill("disk_usage", "read")]);
  renderAt("/skills?host=nas", "/skills", <SkillsPage />);

  await user.click(await screen.findByRole("button", { name: "disk_usage" }));
  const dialog = await screen.findByRole("dialog", { name: "disk_usage" });
  expect(within(dialog).getByText(/^Only looks\. Triage may run it on its own\./)).toBeVisible();
  expect(within(dialog).getByText("This skill has no README.")).toBeVisible();
  expect(within(dialog).getByText("This skill takes no parameters.")).toBeVisible();

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

test("every_risk_group_uses_the_same_fixed_column_widths_so_columns_line_up", async () => {
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/hosts/{name}/skills", () => [
    skill("docker_ps", "read", { description: "A very long description ".repeat(20) }),
    skill("restart_service", "write"),
    skill("wipe_volume", "destructive"),
  ]);
  renderAt("/skills?host=nas", "/skills", <SkillsPage />);

  await screen.findByText("docker_ps");
  const tables = screen.getAllByRole("table");
  expect(tables).toHaveLength(3);
  const widths = tables.map((table) =>
    [...table.querySelectorAll("col")].map((col) => col.style.width),
  );
  expect(widths[0]).toContain("16rem");
  expect(widths[1]).toEqual(widths[0]);
  expect(widths[2]).toEqual(widths[0]);
  expect(tables.every((table) => table.getAttribute("data-layout") === "fixed")).toBe(true);
});

/** A capability row as the hub reports it, with the runner words under test. */
const capabilityRow = (runner: boolean, root: boolean) => ({
  host: "nas",
  entries: [
    {
      contract: "skill.docker_ps",
      version: 1,
      title: "docker_ps",
      category: "skill.observe",
      effect: "read",
      target_kind: "none",
      execution: "skillrunner",
      state: "available",
      reason: null,
      describe: "available",
      catalogue: { state: "complete", gap: null, hash: "abcd" },
      root_agent: root,
      runner_installed: runner,
      from_hello_skills: true,
    },
  ],
  unknown_contracts: [],
});

test("skills_page_warns_when_the_host_has_no_skill_runner", async () => {
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/hosts/{name}/skills", () => [skill("docker_ps", "read")]);
  route("GET /api/v1/hosts/{name}/capabilities", () => capabilityRow(false, false));
  renderAt("/skills?host=nas", "/skills", <SkillsPage />);
  expect(await screen.findByText("Skills cannot run on this host yet")).toBeVisible();
  expect(screen.getByText("No isolated skill runner is installed on this host.")).toBeVisible();
});

test("skills_page_stays_quiet_when_a_runner_or_root_agent_can_run_them", async () => {
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/hosts/{name}/skills", () => [skill("docker_ps", "read")]);
  route("GET /api/v1/hosts/{name}/capabilities", () => capabilityRow(false, true));
  renderAt("/skills?host=nas", "/skills", <SkillsPage />);
  await screen.findByText("docker_ps");
  expect(screen.queryByText("Skills cannot run on this host yet")).toBeNull();
});
