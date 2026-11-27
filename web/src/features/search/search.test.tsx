import { act, fireEvent, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Host, OperationSummary } from "../../api/types";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { incident, nas, T0 } from "../../test/fixtures";
import { knowledgeFact } from "../../test/knowledgeFixtures";
import { renderAt } from "../../test/render";
import { SearchPage } from "./SearchPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const HUB_LABEL = "Historical operations from the hub's own record.";
const STATES = [
  "queued",
  "running",
  "waiting_approval",
  "waiting_user",
  "waiting_agent",
  "waiting_timer",
  "paused",
];

function operation(id: string, status: string, goal: string, hosts = [nas.id]): OperationSummary {
  return {
    id,
    kind: "triage",
    goal,
    status,
    waiting_for: [],
    cycle: 1,
    incidents: [],
    hosts,
    created_at: T0,
    started_at: null,
    ended_at: null,
    updated_at: T0,
  } as OperationSummary;
}

interface Serve {
  facts?: unknown;
  hosts?: Host[];
  incidents?: unknown;
  notes?: unknown;
  operations?: Record<string, { items: OperationSummary[]; next?: string }>;
  past?: unknown;
  rules?: unknown;
  expected?: unknown;
  profile?: string;
}

function serve(options: Serve = {}) {
  route("GET /api/v1/hosts", () => options.hosts ?? [nas]);
  route("GET /api/v1/incidents", () => options.incidents ?? { items: [], next_cursor: null });
  route("GET /api/v1/operations", (init) => {
    const status = (init.params?.query as { status?: string } | undefined)?.status ?? "";
    const found = options.operations?.[status];
    return { items: found?.items ?? [], next_cursor: found?.next ?? null };
  });
  route(
    "GET /api/v1/experience/search",
    () => options.past ?? { items: [], label: HUB_LABEL, query: "", scope: "global" },
  );
  route("GET /api/v1/ack-rules", () => options.rules ?? []);
  route("GET /api/v1/expected-signals", () => options.expected ?? []);
  route("GET /api/v1/hosts/{name}/profile", () =>
    options.profile === undefined
      ? failure(404, "not_found", "no profile yet")
      : { collected_at: T0, markdown: options.profile, sections: [] },
  );
  route("GET /api/v1/notes/search", () => options.notes ?? []);
  route("GET /api/v1/knowledge/facts", () => options.facts ?? { items: [], next_cursor: null });
}

const view = (url = "/search?q=disk") => renderAt(url, "/search", <SearchPage />);
const asked = (path: string) => calls.filter((call) => call.route === `GET ${path}`);

beforeEach(() => resetApi());
afterEach(() => vi.useRealTimers());

test("nothing_is_requested_for_fewer_than_two_characters", async () => {
  serve();
  view("/search?q=d");
  expect(await screen.findByText("Type at least 2 characters.")).toBeVisible();
  expect(screen.queryByRole("heading", { name: "Incidents" })).toBeNull();
  expect(screen.queryByText("Nothing found")).toBeNull();
  const other = calls.filter((call) => call.route !== "GET /api/v1/hosts");
  expect(other).toEqual([]);
});

test("search_groups_run_for_a_query_and_show_their_headings_and_the_scope_sentence", async () => {
  serve();
  view();
  for (const name of [
    "Incidents",
    "Active operations",
    "Past operations",
    "Notes",
    "Facts",
    "Hosts",
    "Known conditions",
    "Expected events",
    "Host profiles",
  ]) {
    expect(await screen.findByRole("heading", { name })).toBeVisible();
  }
  expect(
    screen.getByText(
      "Searches incidents, active operations, past operations, notes, facts, hosts, known conditions, expected events and host profiles. " +
        "Active operations and hosts and known conditions and expected events and host profiles are matched here against complete lists; the rest are " +
        "searched by the hub.",
    ),
  ).toBeVisible();
});

test("the_query_lives_in_the_url_after_the_debounce_and_clearing_removes_it_at_once", async () => {
  serve();
  vi.useFakeTimers({ shouldAdvanceTime: true });
  view("/search");
  const box = screen.getByRole("searchbox", { name: "Search" });
  fireEvent.change(box, { target: { value: "  disk full " } });
  expect(screen.getByTestId("location")).toHaveTextContent(/^\/search$/);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(300);
  });
  expect(screen.getByTestId("location")).toHaveTextContent("/search?q=disk+full");
  expect(asked("/api/v1/incidents")[0]?.init.params?.query).toMatchObject({ q: "disk full" });
  fireEvent.change(box, { target: { value: "" } });
  expect(screen.getByTestId("location")).toHaveTextContent(/^\/search$/);
  expect(screen.queryByRole("heading", { name: "Incidents" })).toBeNull();
});

test("a_long_query_in_the_address_is_cut_to_two_hundred_characters", async () => {
  serve();
  view(`/search?q=${"a".repeat(300)}`);
  const box = await screen.findByRole("searchbox", { name: "Search" });
  expect((box as HTMLInputElement).value).toHaveLength(200);
});

test("incidents_group_asks_the_hub_with_q_and_links_to_the_incident_and_the_list", async () => {
  serve({
    incidents: {
      items: [incident({ id: "inc-9", title: "Disk /srv is full" })],
      next_cursor: null,
    },
  });
  view();
  const link = await screen.findByRole("link", { name: "Disk /srv is full" });
  expect(link).toHaveAttribute("href", "/incidents/inc-9");
  expect(screen.getByRole("link", { name: "Open in Incidents" })).toHaveAttribute(
    "href",
    "/incidents?q=disk",
  );
  expect(asked("/api/v1/incidents")[0]?.init.params?.query).toMatchObject({ q: "disk" });
  expect(screen.getByText(/nas/)).toBeVisible();
});

test("active_operations_are_matched_here_against_all_seven_status_lists", async () => {
  const operations: Record<string, { items: OperationSummary[] }> = {};
  STATES.forEach((status, index) => {
    operations[status] = {
      items: [
        operation(`op-${index}`, status, `Clean disk ${index}`),
        operation(`other-${index}`, status, "Rotate keys", []),
      ],
    };
  });
  operations["running"]?.items.push(operation("op-host", "running", "Rotate keys", [nas.id]));
  serve({ operations });
  view("/search?q=disk");
  expect(await screen.findByRole("link", { name: "Clean disk 0" })).toHaveAttribute(
    "href",
    "/operations/op-0",
  );
  expect(asked("/api/v1/operations")).toHaveLength(7);
  expect(screen.queryByText("Rotate keys")).toBeNull();
  const group = screen.getByRole("region", { name: "Active operations" });
  expect(within(group).getByText("Show all 7")).toBeVisible();
});

test("active_operations_match_on_host_names_too", async () => {
  serve({
    operations: { running: { items: [operation("op-n", "running", "Rotate keys", [nas.id])] } },
  });
  view("/search?q=nas+rotate");
  expect(await screen.findByRole("link", { name: "Rotate keys" })).toBeVisible();
});

test("active_operation_search_says_when_it_could_not_see_everything", async () => {
  serve({
    operations: { running: { items: [operation("op-1", "running", "Clean disk")], next: "c2" } },
  });
  view();
  expect(
    await screen.findByText("Only the newest 50 operations in each state were searched."),
  ).toBeVisible();
});

test("past_operations_come_from_experience_search_with_the_historical_label", async () => {
  serve({
    past: {
      label: HUB_LABEL,
      query: "disk",
      scope: "global",
      items: [
        {
          match_pass: "goal",
          snippet: "freed the disk",
          operation: {
            operation_id: "op-old",
            goal: "Clean disk",
            ended_at: T0,
            outcome: "succeeded",
            verified: true,
            hosts: [{ id: nas.id, name: "nas" }],
            contracts: [],
            narrative: [],
          },
        },
      ],
    },
  });
  view();
  expect(await screen.findByRole("link", { name: "Clean disk" })).toHaveAttribute(
    "href",
    "/operations/op-old",
  );
  expect(screen.getByText(HUB_LABEL)).toBeVisible();
  expect(screen.getByText("Verified")).toBeVisible();
  expect(screen.getByText(/freed the/)).toBeVisible();
  expect(asked("/api/v1/experience/search")[0]?.init.params?.query).toMatchObject({
    q: "disk",
    scope: "global",
  });
  const group = screen.getByRole("region", { name: "Past operations" });
  expect(within(group).queryByRole("button")).toBeNull();
});

test("notes_group_links_to_the_note_and_shows_the_excerpt", async () => {
  serve({
    notes: [
      {
        score: 1,
        snippet: "mount the disk first",
        note: {
          id: "n1",
          title: "Disk runbook",
          body_markdown: "",
          host_id: null,
          tags: [],
          updated_at: T0,
        },
      },
    ],
  });
  view();
  expect(await screen.findByRole("link", { name: "Disk runbook" })).toHaveAttribute(
    "href",
    "/notes/n1",
  );
  expect(screen.getByText("mount the disk first")).toBeVisible();
  expect(asked("/api/v1/notes/search")[0]?.init.params?.query).toMatchObject({ q: "disk" });
});

test("facts_group_searches_the_knowledge_route_and_links_nowhere", async () => {
  serve({
    facts: {
      next_cursor: null,
      items: [
        knowledgeFact({ id: "f1", value: "media disk" }),
        knowledgeFact({ id: "f2", predicate: "layout", value: { disks: 2 }, status: "stale" }),
      ],
    },
  });
  view();
  expect(await screen.findByText("media disk")).toBeVisible();
  expect(screen.getByText('{"disks":2}')).toBeVisible();
  expect(screen.getByText("May be out of date")).toBeVisible();
  const group = screen.getByRole("region", { name: "Facts" });
  expect(within(group).queryByRole("link")).toBeNull();
  expect(asked("/api/v1/knowledge/facts")[0]?.init.params?.query).toMatchObject({ q: "disk" });
});

test("hosts_are_matched_in_the_browser_from_the_complete_list", async () => {
  serve({ hosts: [nas, { ...nas, id: "host-web", name: "web" }] });
  view("/search?q=NA");
  expect(await screen.findByRole("link", { name: "nas" })).toHaveAttribute("href", "/hosts/nas");
  const group = screen.getByRole("region", { name: "Hosts" });
  expect(within(group).queryByRole("link", { name: "web" })).toBeNull();
  expect(asked("/api/v1/hosts")[0]?.init.params).toBeUndefined();
});

test("each_group_shows_five_and_show_all_reveals_the_loaded_rest_without_a_request", async () => {
  const user = userEvent.setup();
  const items = Array.from({ length: 7 }, (_, i) => incident({ id: `i${i}`, title: `Disk ${i}` }));
  serve({ incidents: { items, next_cursor: null } });
  view();
  await screen.findByRole("link", { name: "Disk 0" });
  const group = screen.getByRole("region", { name: "Incidents" });
  expect(within(group).getAllByRole("listitem")).toHaveLength(5);
  const before = calls.length;
  await user.click(within(group).getByRole("button", { name: "Show all 7" }));
  expect(within(group).getAllByRole("listitem")).toHaveLength(7);
  expect(calls).toHaveLength(before);
});

test("a_failed_group_shows_its_error_and_the_rest_show_results_never_nothing_found", async () => {
  serve({
    hosts: [nas],
  });
  route("GET /api/v1/notes/search", () => failure(500, "internal", "boom"));
  view("/search?q=nas");
  const group = await screen.findByRole("region", { name: "Notes" });
  expect(await within(group).findByRole("button", { name: "Retry" })).toBeVisible();
  expect(await screen.findByText("Some sources could not be searched")).toBeVisible();
  expect(screen.getByText("Results from the others are shown.")).toBeVisible();
  expect(screen.getByRole("link", { name: "nas" })).toBeVisible();
  expect(screen.queryByText("Nothing found")).toBeNull();
});

test("no_match_anywhere_says_nothing_found_only_when_every_group_succeeded", async () => {
  serve();
  view("/search?q=zzzz");
  expect(await screen.findByText("Nothing found")).toBeVisible();
  expect(screen.getByText("Nothing matches “zzzz” in the sources above.")).toBeVisible();
  expect(screen.getAllByText(/match\./)).toHaveLength(9);
});

test("nothing_found_waits_while_a_group_is_still_loading", async () => {
  serve();
  route("GET /api/v1/notes/search", () => new Promise(() => undefined));
  view("/search?q=zzzz");
  expect(await screen.findByRole("region", { name: "Facts" })).toBeVisible();
  await screen.findByText("No facts match.");
  expect(screen.queryByText("Nothing found")).toBeNull();
});

test("result_text_is_literal_and_highlighted_never_interpreted", async () => {
  serve({
    incidents: {
      items: [incident({ id: "x", title: "<img src=x onerror=alert(1)> a.b disk" })],
      next_cursor: null,
    },
  });
  const { container } = view("/search?q=a.b");
  const link = await screen.findByRole("link", { name: /onerror/ });
  expect(link.querySelector("img")).toBeNull();
  expect(container.querySelector("img")).toBeNull();
  expect(link.querySelector("mark")).toHaveTextContent("a.b");
});

test("search_ignores_the_host_selector_and_says_so", async () => {
  serve();
  view("/search?q=disk&host=nas");
  expect(await screen.findByText("Search covers every host.")).toBeVisible();
  await screen.findByRole("heading", { name: "Hosts" });
  const sent = calls.filter(
    (call) => call.route.startsWith("GET /api/v1/") && call.init.params?.query,
  );
  expect(sent.length).toBeGreaterThan(0);
  for (const call of sent) {
    expect(call.init.params?.query).not.toHaveProperty("host");
  }
});

test("expected_events_are_searchable_with_the_reason_they_were_fine", async () => {
  serve({
    expected: [
      {
        id: "s1",
        host: "nas",
        check: "security.new_port",
        target: "tcp:0.0.0.0:22",
        summary: "new listener 0.0.0.0:22",
        observed_at: T0,
        rule_id: "r1",
        rule_note: "ssh is meant to listen",
      },
    ],
  });
  view("/search?q=listener");
  expect(await screen.findByText(/ssh is meant to listen/)).toBeVisible();
  expect(screen.getByRole("heading", { name: "Expected events" })).toBeVisible();
});

test("a_host_profile_is_found_by_a_word_in_it_and_is_not_a_note", async () => {
  serve({ profile: "# Host profile: nas\n\nDebian GNU/Linux 13 on x86_64\nkernel 6.12.0" });
  view("/search?q=debian");
  expect(await screen.findByRole("heading", { name: "Host profiles" })).toBeVisible();
  const group = screen.getByRole("heading", { name: "Host profiles" }).closest("section");
  expect(await within(group as HTMLElement).findByRole("link", { name: "nas" })).toBeVisible();
  expect(group).toHaveTextContent("Debian GNU/Linux 13 on x86_64");
  expect(
    screen.queryByRole("heading", { name: "Notes" })?.closest("section"),
  ).not.toHaveTextContent("Debian");
});
