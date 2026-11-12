import { act, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { PROFILE_POLL_MS } from "../../api/hooks/profile";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { T0 } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { ProfileTab } from "./ProfileTab";
import { splitProfile } from "./profileText";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const LATER = "2026-11-01T12:00:30Z";
const GET = "GET /api/v1/hosts/{name}/profile";
const POST = "POST /api/v1/hosts/{name}/profile";

let collected = T0;
let agent = "0.6.0";

function profileBody() {
  return {
    collected_at: collected,
    note: "note-nas",
    markdown: [
      `# Host profile: nas`,
      "",
      `Agent version ${agent}.`,
      "",
      "## Summary",
      "",
      "Debian 12 on x86_64, kernel 6.8.0",
      "4 logical CPUs, 16 GiB memory",
      "3 containers (2 running): web (nginx), db (postgres), job (alpine)",
      "read skills: disk-usage, service-status",
      "checks: disk, memory",
      "",
      "## Recent changes",
      "",
      "- Docker was upgraded",
      "",
      "## Details",
      "",
      "### Host overview",
      "",
      "```",
      "Linux nas 6.8",
      "### not a heading",
      "```",
      "",
      "### Overview",
      "",
      "```",
      "uptime: 41 days",
      "load: 0.08 0.11 0.09",
      "memory: 31 GiB total, 6.2 GiB used",
      "disks:",
      "  /srv 2.0T 91%",
      "  /boot 1.0G 40%",
      "```",
      "",
      "### Containers",
      "",
      "```",
      "Docker: 24.0",
      "Storage: overlay2",
      "```",
      "",
      "### Listening ports",
      "",
      "The tool did not answer: ss is not installed",
    ].join("\n"),
    sections: [
      { title: "Host overview", ok: true },
      { title: "Overview", ok: true },
      { title: "Containers", ok: true },
      { title: "Listening ports", ok: false },
    ],
  };
}

beforeEach(() => {
  resetApi();
  collected = T0;
  agent = "0.6.0";
  route(GET, () => profileBody());
  route(POST, () => {
    collected = LATER;
    agent = "0.7.0";
    return profileBody();
  });
});

afterEach(() => {
  vi.useRealTimers();
});

function show(connected = true) {
  renderAt("/hosts/nas", "/hosts/:name", <ProfileTab host="nas" connected={connected} />);
}

test("profile_tab_shows_its_age_and_links_its_generated_note", async () => {
  show();
  expect(await screen.findByText(/^Collected/)).toBeVisible();
  expect(screen.getByRole("link", { name: "Profile note" })).toHaveAttribute(
    "href",
    "/notes/note-nas",
  );
  expect(await screen.findByText("0.6.0")).toBeVisible();
});

test("the_info_tip_says_what_a_profile_is", async () => {
  const user = userEvent.setup();
  show();
  await user.click(await screen.findByRole("button", { name: "About Profile" }));
  expect(
    await screen.findByText(/What this machine looks like when things are normal/),
  ).toBeVisible();
});

test("a_profile_tool_failure_shows_a_warning_message_with_the_reason", async () => {
  show();
  const warning = await screen.findByText("Could not read Listening ports: ss is not installed.");
  expect(warning.closest("[role=status]")).toHaveAttribute("role", "status");
  expect(screen.queryByText(/^Not available:/)).toBeNull();
});

test("profile_sections_are_always_open_cards_with_a_heading", async () => {
  show();
  await screen.findByText("0.6.0");
  const overview = screen.getByRole("group", { name: "Host overview" });
  expect(within(overview).getByText(/Linux nas 6.8/)).toBeVisible();
  expect(screen.queryByRole("button", { name: "Host overview" })).toBeNull();
  expect(screen.queryByRole("button", { name: /expand|collapse/i })).toBeNull();
});

test("the_summary_shows_the_machines_facts_as_tiles", async () => {
  show();
  expect(await screen.findByText("Debian 12")).toBeVisible();
  expect(screen.getByText("6.8.0")).toBeVisible();
  expect(screen.getByText("4")).toBeVisible();
  expect(screen.getByText("2 running")).toBeVisible();
});

test("tool_output_that_reads_as_key_and_value_pairs_is_a_list_and_other_text_a_mono_block", async () => {
  show();
  await screen.findByText("0.6.0");
  const containers = screen.getByRole("group", { name: "Containers" });
  expect(within(containers).getByText("Storage")).toBeVisible();
  expect(within(containers).getByText("overlay2")).toBeVisible();
  expect(screen.getByRole("group", { name: "Host overview" }).querySelector("pre")).not.toBeNull();
});

test("recent_changes_skills_and_checks_get_their_own_cards", async () => {
  show();
  await screen.findByText("0.6.0");
  expect(
    within(screen.getByRole("group", { name: "Recent changes" })).getByText("Docker was upgraded"),
  ).toBeVisible();
  const lists = screen.getByRole("group", { name: "Skills and checks" });
  expect(within(lists).getByText("disk-usage")).toBeVisible();
  expect(within(lists).getByText("memory")).toBeVisible();
});

test("the_profile_text_splits_into_a_summary_and_one_part_per_tool", () => {
  const body = profileBody();
  const { summary, parts } = splitProfile(body.markdown, body.sections);
  expect(summary).toContain("Agent version 0.6.0.");
  expect(summary).not.toContain("Details");
  expect(parts.map((part) => part.title)).toEqual([
    "Host overview",
    "Overview",
    "Containers",
    "Listening ports",
  ]);
  expect(parts[0]?.body).toContain("### not a heading");
  expect(parts[0]?.text).toBe("Linux nas 6.8\n### not a heading");
  expect(parts[3]).toMatchObject({ ok: false, failure: "ss is not installed" });
});

test("profile_refresh_asks_the_agent_and_shows_the_new_profile", async () => {
  const user = userEvent.setup();
  show();
  await screen.findByText("0.6.0");
  await user.click(screen.getByRole("button", { name: "Refresh" }));
  expect(
    calls.filter((c) => c.route === POST),
    "the click sends the hub's refresh",
  ).toHaveLength(1);
  expect(await screen.findByText("0.7.0")).toBeVisible();
  expect(await screen.findByText("Profile collected")).toBeVisible();
});

test("profile_refresh_repeats_that_the_agent_is_not_connected", async () => {
  route(POST, () => failure(409, "conflict", "the agent is not connected"));
  const user = userEvent.setup();
  show();
  await screen.findByText("0.6.0");
  await user.click(screen.getByRole("button", { name: "Refresh" }));
  expect(await screen.findByText("The agent is not connected.")).toBeVisible();
});

test("a_connected_host_without_a_profile_shows_the_baseline_being_built_and_polls_until_it_exists", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  let ready = false;
  route(GET, () => (ready ? profileBody() : failure(404, "not_found", "no profile yet")));
  show();
  const progress = await screen.findByRole("progressbar", {
    name: "Building the baseline. This takes a few minutes after the agent connects.",
  });
  expect(progress).toBeVisible();

  ready = true;
  await act(() => vi.advanceTimersByTimeAsync(PROFILE_POLL_MS + 100));
  expect(await screen.findByText("0.6.0")).toBeVisible();

  const asked = calls.filter((c) => c.route === GET).length;
  await act(() => vi.advanceTimersByTimeAsync(PROFILE_POLL_MS * 3));
  expect(calls.filter((c) => c.route === GET)).toHaveLength(asked);
});

test("a_host_without_an_agent_asks_to_connect_one_before_anything", async () => {
  route(GET, () => failure(404, "not_found", "no profile yet"));
  show(false);
  expect(await screen.findByText("No profile yet")).toBeVisible();
  expect(screen.getByText(/Connect an agent to profile this machine/)).toBeVisible();
  expect(screen.queryByRole("button", { name: "Refresh" })).toBeNull();
  expect(screen.queryByRole("progressbar")).toBeNull();
});

test("a_profile_that_fails_to_load_shows_retry_and_not_a_baseline_message", async () => {
  route(GET, () => failure(500, "internal", "boom"));
  show();
  const retry = await screen.findByRole("button", { name: "Retry" });
  expect(retry).toBeVisible();
  expect(within(document.body).queryByRole("progressbar")).toBeNull();
});
