import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Fact, Host, Skill } from "../../api/types";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { T0, incident, nas } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { HostDetailPage } from "./HostDetailPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

beforeEach(resetApi);

const HUB: Host = {
  ...nas,
  id: "host-hub",
  name: "hub",
  kind: "local",
  connected: false,
  agent_enrolled: false,
  version: null,
  open_incidents: 0,
};

/** Answers the requests the host page makes with `host` and empty lists, overridable. */
function serve(host: Host = nas, facts: Fact[] = []) {
  route("GET /api/v1/hosts/{name}", () => ({ host, facts }));
  route("GET /api/v1/incidents", () => ({ items: [], next_cursor: null }));
  route("GET /api/v1/hosts/{name}/skills", () => []);
  route("GET /api/v1/hosts/{name}/checks", () => ({ connected: true, checks: [] }));
}

function show(url = "/hosts/nas") {
  return renderAt(url, "/hosts/:name", <HostDetailPage />);
}

function fact(key: string, label: string, value: string, unit?: string): Fact {
  return { key, label, value, unit: unit ?? null, observed_at: T0 };
}

function skill(name: string, risk: string, enabled = true): Skill {
  return {
    name,
    risk,
    enabled,
    description: `Does ${name}`,
    applies_to: [],
    manifest_hash: "abc",
    params: [],
    timeout_secs: 30,
  };
}

test("the_header_names_the_host_and_says_how_it_is_doing_on_one_line", async () => {
  serve({
    ...nas,
    kind: "agent",
    version: "0.0.1",
    outdated: true,
    open_incidents: 2,
    connected_since: "2026-10-31T12:00:00Z",
  });
  show();

  const title = await screen.findByRole("heading", { level: 1, name: "nas" });
  const header = title.closest("header") as HTMLElement;
  expect(within(header).getByText("Connected")).toBeVisible();
  expect(within(header).getByText("Agent")).toBeVisible();
  expect(within(header).getByText("Agent 0.0.1")).toBeVisible();
  expect(within(header).getByText("Update available")).toBeVisible();
  expect(within(header).getByRole("link", { name: "2 open incidents" })).toHaveAttribute(
    "href",
    "/incidents?host=nas",
  );
  expect(within(header).getByText(/^Connected since/)).toBeVisible();
});

test("an_offline_host_shows_when_it_was_last_seen_in_the_header", async () => {
  serve({
    ...nas,
    connected: false,
    version: null,
    last_seen: "2026-10-31T09:00:00Z",
  });
  show();
  const header = (await screen.findByRole("heading", { level: 1 })).closest(
    "header",
  ) as HTMLElement;
  expect(within(header).getByText("Offline")).toBeVisible();
  expect(within(header).getByText(/^Last seen/)).toBeVisible();
});

test("hub_host_without_agent_is_shown_as_hub_only", async () => {
  serve(HUB);
  show("/hosts/hub");
  expect(await screen.findByText("Hub only, no agent")).toBeVisible();
  expect(screen.getByText(/^This is the hub/)).toBeVisible();
});

test("host_detail_page_hints_how_to_cover_the_hub_machine", async () => {
  serve(HUB);
  show("/hosts/hub");
  const hint = (await screen.findByText(/^This is the hub/)).closest("[role=status]");
  expect(hint).toHaveTextContent(
    "install an agent on it and enroll it under the name hub. The hub can also run the agent " +
      "in its own process with --with-local-agent /etc/domovoi/agent.toml, where checks that " +
      "need root report errors.",
  );
  expect(
    within(hint as HTMLElement).getByText("--with-local-agent /etc/domovoi/agent.toml"),
  ).toBeVisible();
});

test("hub_machine_with_a_never_seen_agent_still_shows_an_agent", async () => {
  serve({ ...HUB, agent_enrolled: true });
  show("/hosts/hub");
  await screen.findByRole("heading", { level: 1, name: "hub" });
  expect(screen.queryByText(/^This is the hub/)).toBeNull();
  expect(screen.getByText("Offline")).toBeVisible();
  expect(screen.queryByText("Hub only, no agent")).toBeNull();
});

test("an_unknown_host_says_so_and_links_back_to_the_list", async () => {
  route("GET /api/v1/hosts/{name}", () => failure(404, "not_found", "no such host"));
  show("/hosts/ghost");
  expect(await screen.findByText("No such host")).toBeVisible();
  expect(screen.getByRole("link", { name: "Back to hosts" })).toHaveAttribute("href", "/hosts");
});

test("a_host_that_fails_to_load_shows_retry_not_an_empty_page", async () => {
  route("GET /api/v1/hosts/{name}", () => failure(500, "internal", "boom"));
  show();
  expect(await screen.findByRole("button", { name: "Retry" })).toBeVisible();
});

test("the_tab_follows_the_url_and_the_url_follows_the_tab", async () => {
  serve();
  route("GET /api/v1/hosts/{name}/logs", () => []);
  const user = userEvent.setup();
  show("/hosts/nas?tab=logs");

  expect(await screen.findByRole("tab", { name: "Logs", selected: true })).toBeVisible();
  await user.click(screen.getByRole("tab", { name: "SSH logins" }));
  expect(screen.getByTestId("location")).toHaveTextContent("/hosts/nas?tab=ssh");
  expect(screen.getByRole("tab", { name: "SSH logins", selected: true })).toBeVisible();

  await user.click(screen.getByRole("tab", { name: "Overview" }));
  expect(screen.getByTestId("location")).toHaveTextContent(/^\/hosts\/nas$/);
});

test("arrow_keys_move_focus_between_the_tabs_and_open_them", async () => {
  serve();
  const user = userEvent.setup();
  show();
  await user.click(await screen.findByRole("tab", { name: "Overview" }));
  await user.keyboard("{ArrowRight}");
  expect(screen.getByRole("tab", { name: /^Profile/ })).toHaveFocus();
  expect(screen.getByTestId("location")).toHaveTextContent("tab=profile");
});

test("the_failing_checks_count_shows_on_the_checks_tab", async () => {
  serve();
  route("GET /api/v1/incidents", () => ({
    items: [incident(), incident({ id: "inc-2", check: "memory.available", status: "resolved" })],
    next_cursor: null,
  }));
  route("GET /api/v1/hosts/{name}/checks", () => ({
    connected: true,
    checks: [
      { key: "disk.usage", reported: { enabled: true, interval_seconds: 60 }, overrides: null },
      {
        key: "memory.available",
        reported: { enabled: true, interval_seconds: 60 },
        overrides: null,
      },
    ],
  }));
  show();
  expect(await screen.findByRole("tab", { name: "Checks 1 failing" })).toBeVisible();
});

test("a_recent_incidents_request_failure_shows_retry_and_not_no_incidents", async () => {
  serve();
  route("GET /api/v1/incidents", () => failure(500, "internal", "boom"));
  show();
  const section = (await screen.findByRole("heading", { name: "Recent incidents" })).closest(
    "section",
  ) as HTMLElement;
  expect(await within(section).findByRole("button", { name: "Retry" })).toBeVisible();
  expect(screen.queryByText(/No incidents/)).toBeNull();
});

test("recent_incidents_with_none_say_why_it_is_empty", async () => {
  serve();
  show();
  expect(await screen.findByText("No incidents on nas")).toBeVisible();
});

test("recent_incidents_show_a_resolved_pill_beside_each_incident", async () => {
  serve();
  route("GET /api/v1/incidents", () => ({
    items: [incident({ status: "resolved", verdict: "ACTION_NEEDED" })],
    next_cursor: null,
  }));
  show();
  const table = await screen.findByRole("table", { name: "Recent incidents on nas" });
  const row = within(table).getByRole("link", { name: /Disk \/srv is 93% full/ });
  expect(row).toHaveAttribute("href", "/incidents/inc-1");
  expect(within(table).getByText("Resolved")).toBeVisible();
});

test("facts_are_grouped_into_cards_by_what_they_describe_with_the_rest_under_other", async () => {
  serve(nas, [
    fact("disk./srv.free_gib", "Free on /srv", "120", "GiB"),
    fact("memory.available_gib", "Memory available", "6.5", "GiB"),
    fact("system.uptime_days", "Uptime", "12", "days"),
    fact("net.eth0.speed", "Link speed", "1000", "Mb/s"),
    fact("zzz.custom", "Custom thing", "7"),
  ]);
  show();
  for (const [group, label, value] of [
    ["Disk", "Free on /srv", "120 GiB"],
    ["Memory", "Memory available", "6.5 GiB"],
    ["System", "Uptime", "12 days"],
    ["Network", "Link speed", "1000 Mb/s"],
    ["Other", "Custom thing", "7"],
  ] as const) {
    const card = await screen.findByRole("group", { name: group });
    expect(within(card).getByText(label)).toBeVisible();
    expect(within(card).getByText(new RegExp(`^${value}`))).toBeVisible();
  }
});

test("a_host_without_facts_says_why_and_what_to_check", async () => {
  serve();
  show();
  expect(await screen.findByText("No facts in the last day")).toBeVisible();
});

test("skills_are_a_compact_list_with_a_risk_pill_each", async () => {
  serve();
  route("GET /api/v1/hosts/{name}/skills", () => [
    skill("restart_container", "write"),
    skill("disk_report", "read", false),
  ]);
  show();
  const list = await screen.findByRole("list", { name: "Skills on nas" });
  const restart = within(list).getByText("restart_container").closest("li") as HTMLElement;
  expect(within(restart).getByText("Write")).toBeVisible();
  const report = within(list).getByText("disk_report").closest("li") as HTMLElement;
  expect(within(report).getByText("Read")).toBeVisible();
  expect(within(report).getByText("Off")).toBeVisible();
  expect(screen.getByRole("link", { name: "Manage skills on nas" })).toHaveAttribute(
    "href",
    "/skills?host=nas",
  );
});

test("a_host_name_with_a_slash_is_encoded_in_the_links_it_builds", async () => {
  serve({ ...nas, name: "lab/one", open_incidents: 1 });
  show("/hosts/lab%2Fone");
  expect(await screen.findByRole("link", { name: "1 open incidents" })).toHaveAttribute(
    "href",
    "/incidents?host=lab%2Fone",
  );
});

test("revoking_the_agent_asks_first_and_then_goes_back_to_the_host_list", async () => {
  serve();
  let enrolled = true;
  route("GET /api/v1/hosts/{name}", () => ({
    host: { ...nas, agent_enrolled: enrolled, connected: enrolled },
    facts: [],
  }));
  route("POST /api/v1/hosts/{name}/revoke", () => {
    enrolled = false;
    return { revoked: true };
  });
  const user = userEvent.setup();
  show();
  await user.click(await screen.findByRole("button", { name: "Actions for nas" }));
  await user.click(screen.getByRole("menuitem", { name: "Revoke agent" }));
  expect(calls.filter((c) => c.route === "POST /api/v1/hosts/{name}/revoke")).toHaveLength(0);

  await user.type(screen.getByLabelText(/Type nas to confirm/), "nas");
  await user.click(screen.getByRole("button", { name: "Revoke agent" }));

  expect(await screen.findByText("other page")).toBeVisible();
  expect(screen.getByTestId("location")).toHaveTextContent("/hosts");
});

test("cancelling_the_revoke_stays_on_the_host", async () => {
  serve();
  const user = userEvent.setup();
  show();
  await user.click(await screen.findByRole("button", { name: "Actions for nas" }));
  await user.click(screen.getByRole("menuitem", { name: "Revoke agent" }));
  await user.click(screen.getByRole("button", { name: "Cancel" }));
  expect(calls.filter((c) => c.route === "POST /api/v1/hosts/{name}/revoke")).toHaveLength(0);
  expect(screen.getByRole("heading", { level: 1, name: "nas" })).toBeVisible();
});

test("host_logs_tab_shows_the_recent_lines_that_agent_kept", async () => {
  serve();
  route("GET /api/v1/hosts/{name}/logs", () => [
    { at: T0, level: "warn", target: "agent.checks", message: "disk.usage on /srv is 97% full" },
  ]);
  show("/hosts/nas?tab=logs");

  const log = await screen.findByRole("log", { name: "Log lines on nas" });
  const line = await within(log).findByText(/disk\.usage on \/srv is 97% full/);
  expect(line.closest("[data-level]")).toHaveAttribute("data-level", "warn");
  expect(calls.filter((c) => c.route === "GET /api/v1/hosts/{name}/logs")[0]?.init.params).toEqual({
    path: { name: "nas" },
    query: { level: "info", lines: 200 },
  });
});
