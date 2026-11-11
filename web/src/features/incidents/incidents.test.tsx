import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { IncidentSummary, Turn } from "../../api/types";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { detail, incident, nas } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { MAX_QUESTION_CHARS } from "./IncidentChat";
import { IncidentDetailPage } from "./IncidentDetailPage";
import { IncidentListPage } from "./IncidentListPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

beforeEach(() => {
  resetApi();
  route("GET /api/v1/hosts", () => [nas, { ...nas, id: "host-pi", name: "pi" }]);
  route("GET /api/v1/incidents/{id}/followups", () => ({ turns: [] }));
});

function incidentQueries() {
  return calls
    .filter((call) => call.route === "GET /api/v1/incidents")
    .map((call) => call.init.params?.query);
}

/** One page of incidents for each status, so each one lands in its own group. */
function routeByStatus(incidents: Record<string, IncidentSummary[]>) {
  route("GET /api/v1/incidents", ({ params }) => ({
    items: incidents[String(params?.query?.status)] ?? [],
    next_cursor: null,
  }));
}

/** The group whose heading is `heading`, as a query scope. */
function group(heading: string) {
  const section = screen.getByRole("heading", { name: heading }).closest("section");
  return within(section as HTMLElement);
}

test("incident_list_renders_filters_from_url_and_paginates", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/incidents", ({ params }) =>
    params?.query?.cursor === "c1"
      ? { items: [incident({ id: "inc-old", title: "Older one" })], next_cursor: null }
      : {
          items: [incident({ verdict: "MONITOR", occurrences: 3 })],
          next_cursor: "c1",
        },
  );
  renderAt("/incidents?status=open&severity=critical", "/incidents", <IncidentListPage />);

  expect(await screen.findByRole("link", { name: "Disk /srv is 93% full" })).toHaveAttribute(
    "href",
    "/incidents/inc-1",
  );
  expect(screen.getByLabelText("Status")).toHaveValue("open");
  expect(screen.getByLabelText("Severity")).toHaveValue("critical");
  expect(screen.queryByLabelText("Host"), "the sidebar owns the host choice").toBeNull();
  expect(screen.getByText("Monitor")).toBeVisible();
  expect(screen.getByText(/nas · open · last fired/)).toHaveTextContent("3 times");
  expect(incidentQueries()).toEqual([{ status: "open", severity: "critical", limit: 50 }]);

  await user.click(screen.getByRole("button", { name: "Load more" }));
  expect(await screen.findByRole("link", { name: "Older one" })).toBeVisible();
  expect(incidentQueries()[1]).toEqual({
    status: "open",
    severity: "critical",
    cursor: "c1",
    limit: 50,
  });
  expect(screen.queryByRole("button", { name: "Load more" }), "last page").toBeNull();

  await user.selectOptions(screen.getByLabelText("Status"), "");
  expect(screen.getByTestId("location")).toHaveTextContent("/incidents?severity=critical");
  await screen.findAllByRole("link");
  // Grouping asks once per state; "open" may also refetch the page it already holds.
  const grouped = incidentQueries().slice(-5) as { status: string; severity: string }[];
  expect(grouped.map((query) => query.status).sort()).toEqual([
    "acked",
    "muted",
    "open",
    "pending",
    "resolved",
  ]);
  expect(grouped.every((query) => query.severity === "critical" && !("host" in query))).toBe(true);
});

test("incident_list_explains_what_the_page_is", async () => {
  route("GET /api/v1/incidents", () => ({ items: [incident()], next_cursor: null }));
  renderAt("/incidents?status=open", "/incidents", <IncidentListPage />);

  expect(await screen.findByText(/^Problems Domovoi found on your hosts\./)).toBeVisible();
  const link = screen.getByRole("link", {
    name: "Learn more about incidents (opens in a new tab)",
  });
  expect(link).toHaveAttribute(
    "href",
    "https://RoastSlav.github.io/domovoi/concepts/incidents.html",
  );
});

test("incident_list_follows_the_host_in_the_url_and_keeps_it_when_a_filter_changes", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/incidents", () => ({ items: [incident()], next_cursor: null }));
  renderAt("/incidents?status=open&host=pi", "/incidents", <IncidentListPage />);

  await screen.findByRole("link", { name: "Disk /srv is 93% full" });
  expect(incidentQueries()).toEqual([{ status: "open", host: "pi", limit: 50 }]);

  await user.selectOptions(screen.getByLabelText("Severity"), "critical");
  expect(screen.getByTestId("location")).toHaveTextContent(
    "/incidents?status=open&host=pi&severity=critical",
  );
});

test("incident_list_uses_the_remembered_host_when_the_url_has_none", async () => {
  localStorage.setItem("selected-host", "pi");
  route("GET /api/v1/incidents", () => ({ items: [incident()], next_cursor: null }));
  renderAt("/incidents?status=open", "/incidents", <IncidentListPage />);

  await screen.findByRole("link", { name: "Disk /srv is 93% full" });
  expect(incidentQueries()).toEqual([{ status: "open", host: "pi", limit: 50 }]);
});

test("incident_list_groups_by_state", async () => {
  routeByStatus({
    open: [incident({ id: "inc-open", title: "Open one", status: "open" })],
    acked: [incident({ id: "inc-acked", title: "Acked one", status: "acked" })],
    muted: [
      incident({
        id: "inc-muted",
        title: "Muted one",
        status: "muted",
        last_firing_at: "2026-11-01T12:00:00Z",
      }),
    ],
    pending: [incident({ id: "inc-pending", title: "Pending one", status: "pending" })],
    resolved: [incident({ id: "inc-resolved", title: "Resolved one", status: "resolved" })],
  });
  renderAt("/incidents", "/incidents", <IncidentListPage />);

  await screen.findByRole("heading", { name: "Needs attention (1)" });
  expect(group("Needs attention (1)").getByRole("link", { name: "Open one" })).toBeVisible();
  expect(
    group("Needs attention (1)").getByText(
      "New problems nobody has acknowledged. Expected ones are under Being handled.",
    ),
  ).toBeVisible();

  await screen.findByRole("heading", { name: "Being handled (2)" });
  expect(
    group("Being handled (2)")
      .getAllByRole("link")
      .map((link) => link.textContent),
    "acked and muted merge into one group, newest firing first",
  ).toEqual(["Muted one", "Acked one"]);

  await screen.findByRole("heading", { name: "Waiting for confirmation (1)" });
  expect(
    group("Waiting for confirmation (1)").getByRole("link", { name: "Pending one" }),
  ).toBeVisible();

  await screen.findByRole("heading", { name: "Resolved (1)" });
  const details = screen
    .getByRole("heading", { name: "Resolved (1)" })
    .closest("details") as HTMLElement;
  expect(details, "the resolved group starts closed").not.toHaveAttribute("open");
  expect(within(details).getByRole("link", { name: "Resolved one" })).not.toBeVisible();
  expect(within(details).getByRole("link", { name: "All resolved incidents" })).toHaveAttribute(
    "href",
    "/incidents?status=resolved",
  );
});

test("dismissed_incidents_are_being_handled", async () => {
  routeByStatus({
    open: [
      incident({
        id: "inc-dismissed",
        title: "Dismissed one",
        verdict: "EXPECTED",
        last_firing_at: "2026-11-01T12:00:00Z",
      }),
      incident({
        id: "inc-open",
        title: "Open one",
        last_firing_at: "2026-10-30T12:00:00Z",
      }),
    ],
    acked: [incident({ id: "inc-acked", title: "Acked one", status: "acked" })],
  });
  renderAt("/incidents", "/incidents", <IncidentListPage />);

  await screen.findByRole("heading", { name: "Needs attention (1)" });
  expect(group("Needs attention (1)").getByRole("link", { name: "Open one" })).toBeVisible();
  expect(
    group("Needs attention (1)").queryByRole("link", { name: "Dismissed one" }),
    "a dismissed incident is not asking for attention",
  ).toBeNull();

  await screen.findByRole("heading", { name: "Being handled (2)" });
  expect(
    group("Being handled (2)").getByText(
      "Acknowledged, muted, or expected: dismissed as a known condition, or judged expected by triage. " +
        "A condition that returns after the reopen window is a new incident, and lands under Needs attention.",
    ),
    "the hint says what the grouping means, and what happens when the condition comes back late",
  ).toBeVisible();
  expect(
    group("Being handled (2)")
      .getAllByRole("link")
      .map((link) => link.textContent),
    "the dismissed incident joins the handled ones, newest firing first",
  ).toEqual(["Dismissed one", "Acked one"]);
});

test("incident_list_says_nothing_needs_attention", async () => {
  routeByStatus({});
  renderAt("/incidents", "/incidents", <IncidentListPage />);

  expect(await screen.findByText("Nothing needs attention.")).toBeVisible();
  expect(screen.getByRole("heading", { name: "Needs attention (0)" })).toBeVisible();
  expect(
    screen.queryByRole("heading", { name: "Being handled (0)" }),
    "empty groups hide",
  ).toBeNull();
  expect(
    screen.queryByRole("heading", { name: "Resolved (0)" }),
    "no empty resolved group",
  ).toBeNull();
});

test("incident_list_says_it_could_not_load_incidents", async () => {
  route("GET /api/v1/incidents", () => failure(500, "internal", "the hub is sick"));
  renderAt("/incidents", "/incidents", <IncidentListPage />);

  expect(await screen.findByText("Could not load incidents")).toBeVisible();
  expect(
    screen.queryByRole("heading", { name: "Needs attention (0)" }),
    "no group behind a failed request",
  ).toBeNull();
  expect(
    screen.queryByText("Nothing needs attention."),
    "a failed load is never read as all clear",
  ).toBeNull();
});

test("incident_list_keeps_its_groups_when_a_refresh_fails", async () => {
  const requests = new Map<string, number>();
  route("GET /api/v1/incidents", ({ params }) => {
    const status = String(params?.query?.status);
    const seen = (requests.get(status) ?? 0) + 1;
    requests.set(status, seen);
    if (seen > 1) {
      return failure(500, "internal", "the hub is sick");
    }
    return status === "open"
      ? {
          items: [incident({ id: "inc-open", title: "Open one", status: "open" })],
          next_cursor: null,
        }
      : { items: [], next_cursor: null };
  });
  const { client } = renderAt("/incidents", "/incidents", <IncidentListPage />);

  await screen.findByRole("heading", { name: "Needs attention (1)" });

  client.invalidateQueries({ queryKey: ["incidents"] });

  expect(await screen.findByText("Could not refresh")).toBeVisible();
  expect(
    group("Needs attention (1)").getByRole("link", { name: "Open one" }),
    "the rows stay over a failed refetch",
  ).toBeVisible();
});

test("resolved_incidents_show_a_resolved_badge", async () => {
  routeByStatus({ resolved: [incident({ status: "resolved", verdict: "ACTION_NEEDED" })] });
  renderAt("/incidents", "/incidents", <IncidentListPage />);

  const heading = await screen.findByRole("heading", { name: "Resolved (1)" });
  const details = heading.closest("details") as HTMLElement;
  expect(heading).toBeVisible();
  expect(within(details).getByText("Resolved")).toBeInTheDocument();
  expect(within(details).queryByText("Action needed")).toBeNull();
  expect(within(details).getByText(/nas · resolved · last fired/)).toHaveTextContent(
    "triage said Action needed",
  );
});

test("resolved_incident_without_verdict_has_no_triage_note", async () => {
  routeByStatus({ resolved: [incident({ status: "resolved", verdict: null })] });
  renderAt("/incidents", "/incidents", <IncidentListPage />);

  const heading = await screen.findByRole("heading", { name: "Resolved (1)" });
  const details = heading.closest("details") as HTMLElement;
  expect(within(details).getByText(/nas · resolved · last fired/)).toHaveTextContent(
    "nas · resolved · last fired",
  );
  expect(within(details).queryByText(/triage said/)).toBeNull();
});

test("resolved_card_shows_resolved_badge_and_keeps_the_verdict", async () => {
  const d = detail();
  route("GET /api/v1/incidents/{id}", () => ({
    ...d,
    incident: { ...d.incident, status: "resolved" },
    card: { ...d.card, resolved: true },
  }));
  renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);

  const card = await screen.findByRole("region", { name: "Summary" });
  expect(within(card).getByText("Resolved")).toBeVisible();
  expect(card).toHaveTextContent("Triage said Action needed");
});

test("resolved_card_without_triage_claims_no_verdict", async () => {
  const d = detail();
  route("GET /api/v1/incidents/{id}", () => ({
    ...d,
    incident: incident({ status: "resolved", verdict: null }),
    report: null,
    card: { ...d.card, badge: "warning", resolved: true },
  }));
  renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);

  const card = await screen.findByRole("region", { name: "Summary" });
  expect(within(card).getByText("Resolved")).toBeVisible();
  expect(within(card).queryByText(/Triage said/)).toBeNull();
});

test("incident_detail_shows_card_fields_and_timeline", async () => {
  route("GET /api/v1/incidents/{id}", () => detail());
  renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);

  const card = await screen.findByRole("region", { name: "Summary" });
  expect(within(card).getByText("Action needed")).toBeVisible();
  expect(within(card).getByRole("heading", { name: "Backups filled /srv" })).toBeVisible();
  expect(
    within(card)
      .getAllByRole("listitem")
      .map((li) => li.textContent),
  ).toEqual(["93% used, 70 GB free", "Grew 40 GB since 02:00"]);
  expect(within(card).getByText(/Prune old snapshots/)).toBeVisible();
  expect(calls[0]?.init.params?.path).toEqual({ id: "inc-1" });

  expect(screen.getByText("every").tagName, "details render as Markdown").toBe("STRONG");
  const timeline = screen.getByRole("region", { name: "Timeline" });
  expect(within(timeline).getByText("incident.opened")).toBeVisible();
  expect(within(timeline).getByText("Disk /srv crossed 90%")).toBeVisible();
  expect(within(timeline).getAllByRole("listitem")).toHaveLength(2);

  const actions = screen.getByRole("region", { name: "Actions" });
  expect(within(actions).getByText("restart_container")).toBeVisible();
  expect(within(actions).getByRole("button", { name: "Approve" })).toBeVisible();
  expect(screen.getByRole("button", { name: "Ack" })).toBeVisible();
  expect(screen.getByRole("button", { name: "Mute" })).toBeVisible();
});

test("check_by_hand_list_renders_under_the_card", async () => {
  const d = detail();
  route("GET /api/v1/incidents/{id}", () => ({
    ...d,
    report: {
      headline: "The AC was unplugged",
      summary: "The power supply check failed on this host.",
      cause: "The mains cable came out of the wall bracket.",
      evidence: [],
      verdict: "ACTION_NEEDED",
      next_step: "Reconnect the cable and power the machine on.",
      manual_steps: [
        "Check the cable is seated in the wall socket.",
        "Check the PSU inlet on the case.",
        "Look at the UPS display for a standby mode.",
      ],
      proposals: [],
      details: "",
    },
  }));
  renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);

  const checklist = await screen.findByRole("region", { name: "Check by hand" });
  expect(checklist).toBeVisible();
  expect(
    within(checklist).getByText("Written by the model; Domovoi does not run these."),
    "the steps are marked as the model's words",
  ).toBeVisible();
  expect(
    within(checklist)
      .getAllByRole("listitem")
      .map((li) => li.textContent),
    "the steps keep the order the model wrote them",
  ).toEqual([
    "Check the cable is seated in the wall socket.",
    "Check the PSU inlet on the case.",
    "Look at the UPS display for a standby mode.",
  ]);
});

test("check_by_hand_is_absent_when_the_report_has_no_steps", async () => {
  route("GET /api/v1/incidents/{id}", () => detail());
  renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);

  const card = await screen.findByRole("region", { name: "Summary" });
  expect(within(card).getByText("Backups filled /srv")).toBeVisible();
  expect(screen.queryByRole("region", { name: "Check by hand" })).toBeNull();
});

test("incident_detail_lists_skills_and_tools_used_with_counts", async () => {
  route("GET /api/v1/incidents/{id}", () => ({
    ...detail(),
    tools_used: [
      {
        name: "journal",
        kind: "tool",
        calls: 3,
        failures: 1,
        refused: 2,
        last_used: "2026-10-31T12:05:00Z",
      },
      {
        name: "disk_usage",
        kind: "skill",
        calls: 2,
        failures: 0,
        refused: 1,
        last_used: "2026-10-31T12:04:00Z",
      },
    ],
  }));
  renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);

  const used = await screen.findByRole("region", { name: "Skills and tools used" });
  expect(
    within(used).getByText("Read tools and read skills only look; they never change the host."),
  ).toBeVisible();
  expect(within(used).getByText("Skills proposed: restart_container.")).toBeVisible();

  const rows = within(used).getAllByRole("row");
  expect(within(rows[0] as HTMLElement).getByText("Not run")).toBeVisible();
  expect(within(rows[1] as HTMLElement).getByText("journal")).toBeVisible();
  expect(within(rows[1] as HTMLElement).getByText("tool")).toBeVisible();
  expect(within(rows[1] as HTMLElement).getByText("3")).toBeVisible();
  expect(within(rows[1] as HTMLElement).getByText("1"), "one failed call").toBeVisible();
  expect(
    within(rows[1] as HTMLElement).getByText("2"),
    "two calls not run on budget",
  ).toBeVisible();
  expect(within(rows[2] as HTMLElement).getByText("disk_usage")).toBeVisible();
  expect(within(rows[2] as HTMLElement).getByText("skill")).toBeVisible();
  expect(within(rows[2] as HTMLElement).getByText("0"), "no failed calls").toBeVisible();
  expect(within(rows[2] as HTMLElement).getByText("1"), "one call not run").toBeVisible();
});

test("incident_detail_says_nothing_was_used_yet", async () => {
  route("GET /api/v1/incidents/{id}", () => ({ ...detail(), actions: [] }));
  renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);

  const used = await screen.findByRole("region", { name: "Skills and tools used" });
  expect(within(used).getByText("Nothing used yet.")).toBeVisible();
  expect(within(used).queryByRole("table")).toBeNull();
});

test("ack_and_mute_say_what_they_do", async () => {
  route("GET /api/v1/incidents/{id}", () => detail());
  renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);

  const ack = await screen.findByRole("button", { name: "Ack" });
  expect(ack).toHaveAccessibleDescription(
    "Stops reminders; the incident stays listed until the problem clears.",
  );
  expect(screen.getByRole("button", { name: "Mute" })).toHaveAccessibleDescription(
    "No alerts or reminders until the time you pick.",
  );
});

test("chat_hint_says_the_model_cannot_change_anything", async () => {
  route("GET /api/v1/incidents/{id}", () => detail());
  renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);

  const hint = await screen.findByText("Uses read-only tools on nas. It cannot change anything.");
  expect(hint).toBeVisible();
  expect(screen.getByLabelText("Ask about this incident")).toHaveAccessibleDescription(
    "Uses read-only tools on nas. It cannot change anything.",
  );
});

test("timeline_says_it_starts_with_the_oldest_entry", async () => {
  route("GET /api/v1/incidents/{id}", () => detail());
  renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);

  const timeline = await screen.findByRole("region", { name: "Timeline" });
  expect(
    within(timeline).getByText("Everything that happened to this incident, oldest first."),
  ).toBeVisible();
});

test("incident_detail_retries_triage_and_shows_how_it_went", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/incidents/{id}", () => detail());
  route("POST /api/v1/incidents/{id}/triage", () => ({
    kind: "failed",
    reason: "the model is not available",
  }));
  renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);

  await user.click(await screen.findByRole("button", { name: "Run triage again" }));

  expect(await screen.findByText(/Triage failed: the model is not available/)).toBeVisible();
  const retry = calls.find((call) => call.route === "POST /api/v1/incidents/{id}/triage");
  expect(retry?.init.params?.query).toEqual({ fresh: true });
  expect(screen.getByText(/Counts against the hourly triage limit/)).toBeVisible();
});

test("incident_detail_retries_triage_without_a_report_without_forcing_fresh", async () => {
  const user = userEvent.setup();
  const d = detail();
  route("GET /api/v1/incidents/{id}", () => ({
    ...d,
    report: null,
    triage: { kind: "failed", reason: "the model is not available" },
  }));
  route("POST /api/v1/incidents/{id}/triage", () => ({
    kind: "failed",
    reason: "the model is not available",
  }));
  renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);

  await user.click(await screen.findByRole("button", { name: "Retry triage" }));

  const retry = calls.find((call) => call.route === "POST /api/v1/incidents/{id}/triage");
  expect(retry?.init.params?.query).toEqual({ fresh: false });
});

test("incident_detail_ack_and_ask", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/incidents/{id}", () => detail());
  route("POST /api/v1/incidents/{id}/ack", () => undefined);
  route("POST /api/v1/incidents/{id}/ask", () =>
    failure(503, "triage_off", "triage is not configured"),
  );
  renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);

  await user.click(await screen.findByRole("button", { name: "Ack" }));
  expect(calls.map((c) => c.route)).toContain("POST /api/v1/incidents/{id}/ack");

  await user.type(screen.getByLabelText("Ask about this incident"), "Why now?");
  await user.click(screen.getByRole("button", { name: "Send" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Triage is off");
  expect(calls.at(-1)?.init.body).toEqual({ question: "Why now?" });
});

test("chat_shows_the_saved_turns", async () => {
  const turns: Turn[] = [
    {
      at: "2026-10-31T12:02:00Z",
      asked_by: "user:1",
      question: "Was it the backup job?",
      answer: "Yes, the **backup job** grew it.",
    },
    {
      at: "2026-10-31T12:05:00Z",
      asked_by: "user:1",
      question: "Should I prune them?",
      answer: "Wait for the **last sync** to finish.",
    },
  ];
  route("GET /api/v1/incidents/{id}", () => detail());
  route("GET /api/v1/incidents/{id}/followups", () => ({ turns: [...turns] }));
  renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);

  const chat = await screen.findByRole("region", { name: "Chat" });
  expect(await within(chat).findByText("Was it the backup job?")).toBeVisible();
  expect(within(chat).getByText("Should I prune them?")).toBeVisible();
  expect(within(chat).getByText("backup job").tagName).toBe("STRONG");
  expect(within(chat).getByText("last sync").tagName).toBe("STRONG");
});

test("chat_shows_the_question_then_its_answer", async () => {
  const user = userEvent.setup();
  const turns: Turn[] = [];
  const answer = "Because the nightly backup kept every snapshot.";
  route("GET /api/v1/incidents/{id}", () => detail());
  route("GET /api/v1/incidents/{id}/followups", () => ({ turns: [...turns] }));
  route("POST /api/v1/incidents/{id}/ask", (init) => {
    turns.push({
      at: "2026-10-31T12:07:00Z",
      asked_by: "user:1",
      question: (init.body as { question: string }).question,
      answer,
    });
    return { answer };
  });
  renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);

  const textarea = await screen.findByLabelText("Ask about this incident");
  await user.type(textarea, "Why now?");
  await user.click(screen.getByRole("button", { name: "Send" }));

  expect(
    calls.filter((call) => call.route === "POST /api/v1/incidents/{id}/ask"),
    "the hub got the question once",
  ).toHaveLength(1);
  expect(screen.getByText(/Why now\?/, { selector: "span" })).toBeVisible();
  const answered = await screen.findByText(/^Because the nightly backup kept every snapshot\.$/);
  expect(answered, "the answer lands when the remembered turns refresh").toBeVisible();
});

test("chat_clear_button_forgets_the_conversation_after_confirming", async () => {
  const user = userEvent.setup();
  const turns: Turn[] = [
    {
      at: "2026-10-31T12:02:00Z",
      asked_by: "user:1",
      question: "Was it the backup job?",
      answer: "Yes.",
    },
  ];
  route("GET /api/v1/incidents/{id}", () => detail());
  route("GET /api/v1/incidents/{id}/followups", () => ({ turns: [...turns] }));
  route("POST /api/v1/incidents/{id}/followups/clear", () => {
    turns.length = 0;
  });
  renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);

  const chat = await screen.findByRole("region", { name: "Chat" });
  expect(await within(chat).findByText("Was it the backup job?")).toBeVisible();

  await user.click(await screen.findByRole("button", { name: "Clear conversation" }));
  const dialog = screen.getByRole("dialog", { name: "Clear the conversation?" });
  expect(
    within(dialog).getByText(
      "The model forgets this conversation. The questions stay in the timeline.",
    ),
    "the dialog says what clearing does",
  ).toBeVisible();

  await user.click(within(dialog).getByRole("button", { name: "Clear conversation" }));
  expect(
    calls.filter((call) => call.route === "POST /api/v1/incidents/{id}/followups/clear"),
    "the hub cleared the conversation",
  ).toHaveLength(1);
  expect(
    screen.queryByRole("dialog", { name: "Clear the conversation?" }),
    "the dialog closes once the hub clears",
  ).toBeNull();
});

test("chat_enter_sends_the_question_without_inserting_a_newline", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/incidents/{id}", () => detail());
  route("POST /api/v1/incidents/{id}/ask", () => failure(500, "internal", "the hub is sick"));
  renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);

  const textarea = await screen.findByLabelText("Ask about this incident");
  await user.type(textarea, "Why now?");
  await user.keyboard("{Enter}");
  expect(
    calls.filter((call) => call.route === "POST /api/v1/incidents/{id}/ask"),
    "Enter alone sent the question",
  ).toHaveLength(1);
  expect((textarea as HTMLTextAreaElement).value, "Enter did not also leave a newline").toBe(
    "Why now?",
  );
});

test("chat_shift_enter_stays_in_the_text", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/incidents/{id}", () => detail());
  renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);

  const textarea = await screen.findByLabelText("Ask about this incident");
  await user.type(textarea, "First line");
  await user.keyboard("{Shift>}{Enter}{/Shift}");
  expect(
    calls.filter((call) => call.route === "POST /api/v1/incidents/{id}/ask"),
    "Shift+Enter is only a line break",
  ).toHaveLength(0);
  expect((textarea as HTMLTextAreaElement).value, "Shift+Enter broke the line in the text").toBe(
    "First line\n",
  );
});

test("chat_caps_the_question_at_the_hub_limit", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/incidents/{id}", () => detail());
  renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);

  const textarea = await screen.findByLabelText("Ask about this incident");
  expect(textarea).toHaveAttribute("maxlength", String(MAX_QUESTION_CHARS));
  await user.type(textarea, "a".repeat(MAX_QUESTION_CHARS + 1));
  expect(
    (textarea as HTMLTextAreaElement).value,
    "the character past 1000 is dropped",
  ).toHaveLength(MAX_QUESTION_CHARS);
}, 30000);

test("chat_empties_the_composer_once_the_hub_answers", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/incidents/{id}", () => detail());
  route("POST /api/v1/incidents/{id}/ask", () => ({ answer: "Noted." }));
  renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);

  const textarea = await screen.findByLabelText("Ask about this incident");
  await user.type(textarea, "Why now?");
  await user.click(screen.getByRole("button", { name: "Send" }));
  expect(await screen.findByText(/^Noted\.$/)).toBeVisible();
  expect((textarea as HTMLTextAreaElement).value, "the composer is emptied").toBe("");
  expect(screen.getByRole("button", { name: "Send" }), "Send cannot resubmit").toBeDisabled();
});

test("chat_keeps_the_question_when_the_answer_fails", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/incidents/{id}", () => detail());
  route("POST /api/v1/incidents/{id}/ask", () => failure(500, "internal", "the hub is sick"));
  renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);

  const textarea = await screen.findByLabelText("Ask about this incident");
  await user.type(textarea, "Why now?");
  await user.click(screen.getByRole("button", { name: "Send" }));
  expect(
    calls.filter((call) => call.route === "POST /api/v1/incidents/{id}/ask"),
    "the hub got the question once",
  ).toHaveLength(1);
  expect(await screen.findByText("Could not get an answer. Try again.")).toBeVisible();
  expect((textarea as HTMLTextAreaElement).value, "the question stays for a retry").toBe(
    "Why now?",
  );
  expect(screen.getByRole("button", { name: "Send" }), "the retry is open").toBeEnabled();
});

test("chat_shows_the_answer_from_the_response_when_the_hub_never_records_the_turn", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/incidents/{id}", () => detail());
  route("POST /api/v1/incidents/{id}/ask", () => ({ answer: "Noted." }));
  // the beforeEach follow-ups route always answers with no turns: the hub never
  // records the question, exactly the race where storing it failed
  renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);

  const chat = await screen.findByRole("region", { name: "Chat" });
  await user.type(await screen.findByLabelText("Ask about this incident"), "Why now?");
  await user.click(screen.getByRole("button", { name: "Send" }));
  expect(within(chat).getByText("Why now?"), "the asked question is shown").toBeVisible();
  const answered = await within(chat).findByText(/^Noted\.$/);
  expect(answered, "the answer shows even though the history never got it").toBeVisible();
  expect(screen.queryByText("Thinking…"), "the chat is not stuck on Thinking").toBeNull();
});

test("chat_says_when_the_conversation_could_not_be_loaded", async () => {
  route("GET /api/v1/incidents/{id}", () => detail());
  route("GET /api/v1/incidents/{id}/followups", () => failure(500, "internal", "the hub is sick"));
  renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);

  expect(await screen.findByText("Could not load the conversation.")).toBeVisible();
  expect(
    screen.queryByText("Thinking…"),
    "a failed history is not read as an empty conversation with a stuck answer",
  ).toBeNull();
});

test("timeline_hides_the_question_text_of_a_followup_entry", async () => {
  const withFollowUp = {
    ...detail(),
    timeline: [
      ...detail().timeline,
      {
        at: "2026-10-31T12:02:00Z",
        kind: "triage.followup",
        summary: "Was it the backup job?",
      },
    ],
  };
  route("GET /api/v1/incidents/{id}", () => withFollowUp);
  renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);

  const timeline = await screen.findByRole("region", { name: "Timeline" });
  expect(within(timeline).getByText("triage.followup")).toBeVisible();
  expect(
    within(timeline).queryByText("Was it the backup job?"),
    "the chat shows the question, the timeline only its kind",
  ).toBeNull();
});

test("dismiss_dialog_sends_the_reason_and_the_period", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/incidents/{id}", () => detail());
  route("POST /api/v1/incidents/{id}/dismiss", () => undefined);
  renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);

  await user.click(await screen.findByRole("button", { name: "Dismiss" }));
  const dialog = screen.getByRole("dialog", { name: "Dismiss as a known condition" });
  expect(
    within(dialog).getByText(
      "The incident closes as an expected condition, so it stops alerting and no triage runs. If the check keeps failing, the incident reopens quietly: no alert and no triage. The rule covers every severity of this check on this target, so a critical later on the same mount is quiet too.",
    ),
    "saying what a dismissal does to the alerts",
  ).toBeVisible();

  await user.type(
    within(dialog).getByLabelText("Why is this not a real problem?"),
    "Backups fill it, and it clears afterwards.",
  );
  await user.selectOptions(within(dialog).getByLabelText("Keep the check quiet for"), "1 day");
  await user.click(within(dialog).getByRole("button", { name: "Dismiss" }));

  const dismiss = calls.find((call) => call.route === "POST /api/v1/incidents/{id}/dismiss");
  expect(dismiss?.init.params?.path).toEqual({ id: "inc-1" });
  expect(dismiss?.init.body).toEqual({
    reason: "Backups fill it, and it clears afterwards.",
    ignore_for_minutes: 1440,
  });
  expect(
    screen.queryByRole("dialog", { name: "Dismiss as a known condition" }),
    "the dialog closes once the hub takes the dismissal",
  ).toBeNull();
});

test("dismiss_dialog_without_a_period_sends_no_rule", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/incidents/{id}", () => detail());
  route("POST /api/v1/incidents/{id}/dismiss", () => undefined);
  renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);

  await user.click(await screen.findByRole("button", { name: "Dismiss" }));
  const dialog = screen.getByRole("dialog", { name: "Dismiss as a known condition" });
  expect(
    within(dialog).getByRole("button", { name: "Dismiss" }),
    "an empty reason cannot be sent",
  ).toHaveAttribute("aria-disabled", "true");

  await user.type(
    within(dialog).getByLabelText("Why is this not a real problem?"),
    "The nightly backup fills it, and it clears on its own.",
  );
  await user.selectOptions(within(dialog).getByLabelText("Keep the check quiet for"), "No");
  await user.click(within(dialog).getByRole("button", { name: "Dismiss" }));

  const dismiss = calls.find((call) => call.route === "POST /api/v1/incidents/{id}/dismiss");
  expect(dismiss?.init.body).toEqual({
    reason: "The nightly backup fills it, and it clears on its own.",
    ignore_for_minutes: null,
  });
});

test("dismiss_is_offered_only_after_the_incident_is_confirmed", async () => {
  route("GET /api/v1/incidents/{id}", () => ({
    ...detail(),
    incident: incident({ status: "pending" }),
  }));
  renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);

  expect(
    await screen.findByRole("heading", { name: "Backups filled /srv" }),
    "the page shows the pending incident",
  ).toBeVisible();
  expect(
    screen.queryByRole("button", { name: "Dismiss" }),
    "dismissing a pending incident is refused, so the page offers no refusal",
  ).toBeNull();
});
