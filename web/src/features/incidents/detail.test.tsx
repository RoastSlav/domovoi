import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { action, detail, incident, nas, stubMedia } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { IncidentDetailPage } from "./IncidentDetailPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

beforeEach(() => {
  resetApi();
  sessionStorage.clear();
  route("GET /api/v1/hosts", () => [nas, { ...nas, id: "host-pi", name: "pi" }]);
  route("GET /api/v1/incidents/{id}/followups", () => ({ turns: [] }));
  route("GET /api/v1/settings", () => ({ triage: { enabled: true } }));
});

afterEach(() => {
  vi.unstubAllGlobals();
});

function open() {
  return renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);
}

const REPORT = {
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
  proposals: [{ skill: "restart_container", args: { name: "web" }, reason: "It is stuck." }],
  details: "",
};

test("incident_detail_shows_the_header_summary_and_actions", async () => {
  route("GET /api/v1/incidents/{id}", () => detail());
  open();

  expect(
    await screen.findByRole("heading", { level: 1, name: "Disk /srv is 93% full" }),
  ).toBeVisible();
  expect(calls[0]?.init.params?.path).toEqual({ id: "inc-1" });
  const header = within(screen.getByRole("banner"));
  expect(header.getByText("Open"), "status").toBeVisible();
  expect(header.getByText("Action needed"), "verdict").toBeVisible();
  expect(header.getByText("Warning"), "severity").toBeVisible();
  expect(header.getByRole("link", { name: "nas" })).toHaveAttribute("href", "/hosts/nas");
  expect(header.getByText("disk.usage /srv")).toBeVisible();
  expect(header.getByText(/^Opened/)).toBeVisible();
  expect(header.getByText(/^Last fired/)).toBeVisible();

  const card = within(screen.getByRole("group", { name: "Summary" }));
  expect(card.getByText("Action needed")).toBeVisible();
  expect(card.getByText("Backups filled /srv")).toBeVisible();
  expect(card.getAllByRole("listitem").map((li) => li.textContent)).toEqual([
    "93% used, 70 GB free",
    "Grew 40 GB since 02:00",
  ]);
  expect(card.getByText(/Prune old snapshots/)).toBeVisible();
  expect(await screen.findByText("every"), "details render as Markdown").toHaveProperty(
    "tagName",
    "STRONG",
  );

  const actions = within(screen.getByRole("region", { name: "Proposed actions" }));
  expect(actions.getByText("restart_container")).toBeVisible();
  expect(actions.getByRole("button", { name: "Approve" })).toBeVisible();
});

test("a_host_name_in_the_link_is_encoded", async () => {
  route("GET /api/v1/incidents/{id}", () => ({ ...detail(), host: "nas 01/a" }));
  open();
  expect(await screen.findByRole("link", { name: "nas 01/a" })).toHaveAttribute(
    "href",
    "/hosts/nas%2001%2Fa",
  );
});

test("incident_detail_has_two_columns_on_a_wide_screen", async () => {
  stubMedia(true);
  route("GET /api/v1/incidents/{id}", () => detail());
  open();

  const side = await screen.findByRole("complementary", { name: "Actions and history" });
  expect(side.parentElement).toHaveAttribute("data-columns", "2");
  expect(within(side).getByRole("region", { name: "Proposed actions" })).toBeVisible();
  expect(within(side).getByRole("region", { name: "Timeline" })).toBeVisible();
  expect(
    within(side).queryByRole("region", { name: "Chat" }),
    "the story is on the left",
  ).toBeNull();
});

test("incident_detail_is_one_column_on_a_narrow_screen", async () => {
  stubMedia(false);
  route("GET /api/v1/incidents/{id}", () => detail());
  open();

  const side = await screen.findByRole("complementary", { name: "Actions and history" });
  expect(side.parentElement).toHaveAttribute("data-columns", "1");
  const order = [...(side.parentElement as HTMLElement).querySelectorAll("section")].map(
    (section) => section.querySelector("h2")?.textContent,
  );
  expect(order.indexOf("Proposed actions"), "the next step comes before the story").toBeLessThan(
    order.indexOf("Triage"),
  );
});

test("resolved_card_shows_resolved_badge_and_keeps_the_verdict", async () => {
  const d = detail();
  route("GET /api/v1/incidents/{id}", () => ({
    ...d,
    incident: { ...d.incident, status: "resolved" },
    card: { ...d.card, resolved: true },
  }));
  open();

  const card = await screen.findByRole("group", { name: "Summary" });
  expect(within(card).getByText("Resolved")).toBeVisible();
  expect(card).toHaveTextContent("Triage said Action needed");
  expect(
    screen.queryByRole("button", { name: "Acknowledge" }),
    "a resolved incident has nothing left to do",
  ).toBeNull();
});

test("resolved_card_without_triage_claims_no_verdict", async () => {
  const d = detail();
  route("GET /api/v1/incidents/{id}", () => ({
    ...d,
    incident: incident({ status: "resolved", verdict: null }),
    report: null,
    card: { ...d.card, badge: "warning", resolved: true },
  }));
  open();

  const card = await screen.findByRole("group", { name: "Summary" });
  expect(within(card).getByText("Resolved")).toBeVisible();
  expect(within(card).queryByText(/Triage said/)).toBeNull();
});

test("a_missing_incident_says_so_and_a_failed_load_offers_retry", async () => {
  route("GET /api/v1/incidents/{id}", () => failure(404, "not_found", "no such incident"));
  const first = open();
  expect(await screen.findByText("No such incident")).toBeVisible();
  expect(screen.getByRole("link", { name: "Back to incidents" })).toHaveAttribute(
    "href",
    "/incidents",
  );
  first.unmount();

  route("GET /api/v1/incidents/{id}", () => failure(500, "internal", "the hub is sick"));
  open();
  expect(await screen.findByText("Could not load the incident")).toBeVisible();
  expect(screen.getByRole("button", { name: "Retry" })).toBeVisible();
});

test("check_by_hand_is_a_checklist_kept_for_the_visit", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/incidents/{id}", () => ({ ...detail(), report: REPORT }));
  const first = open();

  const checklist = within(await screen.findByRole("region", { name: "Check by hand" }));
  expect(
    checklist.getByText(/^Written by the model; Domovoi does not run these\./),
    "the steps are marked as the model's words",
  ).toBeVisible();
  const steps = checklist.getAllByRole("checkbox");
  expect(
    steps.map((step) => step.closest("label")?.textContent),
    "the steps keep the order the model wrote them",
  ).toEqual(REPORT.manual_steps);

  await user.click(steps[1] as HTMLElement);
  expect(steps[1]).toBeChecked();
  expect(
    calls.filter((call) => call.route.startsWith("POST")),
    "a tick is never sent to the hub",
  ).toEqual([]);

  first.unmount();
  open();
  const again = within(await screen.findByRole("region", { name: "Check by hand" }));
  expect(
    again.getAllByRole("checkbox").map((step) => (step as HTMLInputElement).checked),
    "the tick is still there after leaving and coming back",
  ).toEqual([false, true, false]);
});

test("check_by_hand_is_absent_when_the_report_has_no_steps", async () => {
  route("GET /api/v1/incidents/{id}", () => detail());
  open();

  await screen.findByRole("group", { name: "Summary" });
  expect(screen.queryByRole("region", { name: "Check by hand" })).toBeNull();
});

test("an_action_shows_why_the_model_proposed_it", async () => {
  route("GET /api/v1/incidents/{id}", () => ({ ...detail(), report: REPORT }));
  open();

  const actions = within(await screen.findByRole("region", { name: "Proposed actions" }));
  expect(actions.getByText("It is stuck.")).toBeVisible();
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
  open();

  const used = await screen.findByRole("region", { name: "Skills and tools used" });
  expect(
    within(used).getByText("Read tools and read skills only look; they never change the host."),
  ).toBeVisible();
  expect(within(used).getByText("Skills proposed: restart_container.")).toBeVisible();

  const rows = within(used).getAllByRole("row");
  expect(within(rows[0] as HTMLElement).getByText("Not run")).toBeVisible();
  const cells = (row: number) =>
    within(rows[row] as HTMLElement)
      .getAllByRole("cell")
      .slice(0, 5)
      .map((cell) => cell.textContent);
  expect(cells(1), "name, kind, calls, failures, not run").toEqual([
    "journal",
    "tool",
    "3",
    "1",
    "2",
  ]);
  expect(cells(2)).toEqual(["disk_usage", "skill", "2", "0", "1"]);
});

test("incident_detail_says_nothing_was_used_yet", async () => {
  route("GET /api/v1/incidents/{id}", () => ({ ...detail(), actions: [] }));
  open();

  const used = await screen.findByRole("region", { name: "Skills and tools used" });
  expect(within(used).getByText("Nothing used yet.")).toBeVisible();
  expect(within(used).queryByRole("table")).toBeNull();
  expect(
    within(screen.getByRole("region", { name: "Proposed actions" })).getByText(
      /^No actions proposed\./,
    ),
  ).toBeVisible();
});

test("the_main_actions_say_what_they_do_and_what_they_do_not", async () => {
  route("GET /api/v1/incidents/{id}", () => detail());
  open();

  expect(await screen.findByRole("button", { name: "Acknowledge" })).toHaveAccessibleDescription(
    "Stops reminders. The incident stays open and still resolves by itself.",
  );
  expect(screen.getByRole("button", { name: "Mute for" })).toHaveAccessibleDescription(
    "No alerts or reminders for the time you pick. The incident stays open.",
  );
  expect(screen.getByRole("button", { name: "Dismiss" })).toHaveAccessibleDescription(
    "Closes it as a known condition. You say why, and whether the check stays quiet.",
  );
});

test("acknowledge_and_mute_report_their_result", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/incidents/{id}", () => detail());
  route("POST /api/v1/incidents/{id}/ack", () => undefined);
  route("POST /api/v1/incidents/{id}/mute", () => undefined);
  open();

  await user.click(await screen.findByRole("button", { name: "Acknowledge" }));
  expect(await screen.findByText("Incident acknowledged")).toBeVisible();

  await user.click(screen.getByRole("button", { name: "Mute for" }));
  await user.click(screen.getByRole("menuitem", { name: "For 1 day" }));
  expect(await screen.findByText("Incident muted for 1 day")).toBeVisible();
  expect(calls.find((c) => c.route === "POST /api/v1/incidents/{id}/mute")?.init.body).toEqual({
    minutes: 1440,
  });
  expect(screen.getByRole("button", { name: "Undo" })).toBeVisible();
});

test("a_muted_incident_offers_unmute", async () => {
  const d = detail();
  route("GET /api/v1/incidents/{id}", () => ({
    ...d,
    incident: { ...d.incident, status: "muted" },
  }));
  open();

  expect(await screen.findByRole("button", { name: "Unmute" })).toBeVisible();
  expect(screen.queryByRole("button", { name: "Mute for" })).toBeNull();
});

test("a_status_change_from_elsewhere_is_announced", async () => {
  const d = detail();
  let status = "open";
  // An incident no other test has touched: the page's own recent changes are not news.
  route("GET /api/v1/incidents/{id}", () => ({
    ...d,
    incident: { ...d.incident, id: "inc-news", status },
  }));
  const { client } = renderAt("/incidents/inc-news", "/incidents/:id", <IncidentDetailPage />);
  await screen.findByRole("heading", { level: 1 });

  status = "resolved";
  void client.invalidateQueries({ queryKey: ["incidents"] });
  expect(await screen.findByText("This incident resolved")).toBeVisible();
});

test("the_persons_own_change_is_not_announced_twice", async () => {
  const user = userEvent.setup();
  const d = detail();
  let status = "open";
  route("GET /api/v1/incidents/{id}", () => ({ ...d, incident: { ...d.incident, status } }));
  route("POST /api/v1/incidents/{id}/ack", () => {
    status = "acked";
  });
  open();

  await user.click(await screen.findByRole("button", { name: "Acknowledge" }));
  expect(await screen.findByText("Incident acknowledged")).toBeVisible();
  expect(screen.queryByText("This incident was acknowledged")).toBeNull();
});

test("triage_that_failed_shows_the_reason_and_retry", async () => {
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
  open();

  const panel = within(await screen.findByRole("region", { name: "Triage" }));
  expect(panel.getByText("Triage failed")).toBeVisible();
  expect(panel.getByText("The model is not available.")).toBeVisible();
  const retry = panel.getByRole("button", { name: "Retry triage" });
  expect(retry).toHaveAccessibleDescription(/Counts against the hourly triage limit\./);

  await user.click(retry);
  const sent = calls.find((call) => call.route === "POST /api/v1/incidents/{id}/triage");
  expect(sent?.init.params?.query, "without a report there is nothing to bypass").toEqual({
    fresh: false,
  });
  expect(screen.queryByText("Triage finished"), "a failure is not a success toast").toBeNull();
});

test("triage_that_was_skipped_says_why", async () => {
  const d = detail();
  route("GET /api/v1/incidents/{id}", () => ({
    ...d,
    report: null,
    triage: { kind: "skipped", reason: "too many incidents waiting for triage" },
  }));
  open();

  const panel = within(await screen.findByRole("region", { name: "Triage" }));
  expect(panel.getByText("Triage was skipped")).toBeVisible();
  expect(panel.getByText("Too many incidents waiting for triage.")).toBeVisible();
});

test("running_triage_again_asks_for_a_fresh_run_and_shows_progress", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/incidents/{id}", () => detail());
  let finish: (value: unknown) => void = () => undefined;
  route("POST /api/v1/incidents/{id}/triage", () => new Promise((resolve) => (finish = resolve)));
  open();

  await user.click(await screen.findByRole("button", { name: "Run triage again" }));
  expect(await screen.findByText("Triage is reading the logs…")).toBeVisible();
  const sent = calls.find((call) => call.route === "POST /api/v1/incidents/{id}/triage");
  expect(sent?.init.params?.query).toEqual({ fresh: true });

  finish({ kind: "report" });
  expect(await screen.findByText("Triage finished")).toBeVisible();
});

test("a_refused_triage_run_is_explained_in_words", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/incidents/{id}", () => detail());
  route("POST /api/v1/incidents/{id}/triage", () =>
    failure(503, "budget_spent", "the monthly model budget is spent"),
  );
  open();

  await user.click(await screen.findByRole("button", { name: "Run triage again" }));
  expect(await screen.findByText("Could not run triage")).toBeVisible();
  expect(
    screen.getByText("The monthly model budget is spent. It resets next month."),
  ).toBeVisible();
});

test("a_reused_report_is_a_quiet_note", async () => {
  route("GET /api/v1/incidents/{id}", () => ({ ...detail(), triage: { kind: "cached" } }));
  open();

  const panel = within(await screen.findByRole("region", { name: "Triage" }));
  expect(
    panel.getByText("Triage reused the report of an earlier incident with the same evidence."),
  ).toBeVisible();
  expect(panel.queryByRole("alert")).toBeNull();
});

test("timeline_reads_in_words_and_keeps_an_unknown_kind", async () => {
  route("GET /api/v1/incidents/{id}", () => ({
    ...detail(),
    timeline: [
      { at: "2026-10-31T12:00:00Z", kind: "incident.created", summary: "Disk /srv crossed 90%" },
      { at: "2026-10-31T12:00:30Z", kind: "status.changed", summary: "pending → open" },
      { at: "2026-10-31T12:01:00Z", kind: "triage.report", summary: "Backups filled /srv" },
      { at: "2026-10-31T12:01:01Z", kind: "verdict", summary: "ACTION NEEDED" },
      { at: "2026-10-31T12:02:00Z", kind: "triage.followup", summary: "Was it the backup job?" },
      { at: "2026-10-31T12:03:00Z", kind: "action.approved", summary: "restart_container" },
      { at: "2026-10-31T12:04:00Z", kind: "from.a_newer_hub", summary: "Something new" },
    ],
  }));
  open();

  const timeline = within(await screen.findByRole("region", { name: "Timeline" }));
  expect(
    timeline.getByText("Everything that happened to this incident, oldest first."),
  ).toBeVisible();
  const lines = timeline.getAllByRole("listitem").map((li) => li.querySelector("p")?.firstChild);
  expect(lines.map((node) => node?.textContent)).toEqual([
    "Opened",
    "Confirmed",
    "Triage reported",
    "Triage said Action needed",
    "You asked a follow-up",
    "Action: Approved",
    "from.a_newer_hub",
  ]);
  expect(timeline.getByText("Disk /srv crossed 90%"), "the hub's line is the detail").toBeVisible();
  expect(timeline.getByText("Something new")).toBeVisible();
  expect(timeline.queryByText("incident.created"), "no raw kind for a known one").toBeNull();
  expect(
    timeline.queryByText("Was it the backup job?"),
    "the chat shows the question, the timeline only that one was asked",
  ).toBeNull();
});

test("a_long_timeline_shows_its_newest_entries_until_asked_for_all", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/incidents/{id}", () => ({
    ...detail(),
    timeline: Array.from({ length: 12 }, (_, n) => ({
      at: `2026-10-31T12:${String(n).padStart(2, "0")}:00Z`,
      kind: "incident.grouped",
      summary: `grouped check.${n}`,
    })),
  }));
  open();

  const timeline = within(await screen.findByRole("region", { name: "Timeline" }));
  expect(timeline.getAllByRole("listitem")).toHaveLength(8);
  expect(timeline.getByText("grouped check.11"), "the newest stay").toBeVisible();
  expect(timeline.queryByText("grouped check.0")).toBeNull();

  await user.click(timeline.getByRole("button", { name: "Show all 12" }));
  expect(timeline.getAllByRole("listitem")).toHaveLength(12);
  expect(timeline.getByText("grouped check.0")).toBeVisible();
});

async function openDismiss(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByRole("button", { name: "Dismiss" }));
  return screen.getByRole("dialog", { name: "Dismiss as a known condition" });
}

test("dismiss_dialog_sends_the_reason_and_the_period", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/incidents/{id}", () => detail());
  route("POST /api/v1/incidents/{id}/dismiss", () => undefined);
  open();

  const dialog = await openDismiss(user);
  expect(
    within(dialog).getByText(
      /If the check keeps failing while it is quiet, it reopens without an alert or triage\./,
    ),
    "saying what a dismissal does to the alerts",
  ).toBeVisible();

  await user.type(
    within(dialog).getByLabelText(/^Why is this not a real problem\?/),
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
  expect(await screen.findByText("Incident dismissed as a known condition")).toBeVisible();
  expect(
    screen.queryByRole("dialog", { name: "Dismiss as a known condition" }),
    "the dialog closes once the hub takes the dismissal",
  ).toBeNull();
});

test("dismiss_dialog_spells_out_each_choice_and_needs_a_reason", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/incidents/{id}", () => detail());
  route("POST /api/v1/incidents/{id}/dismiss", () => undefined);
  open();

  const dialog = within(await openDismiss(user));
  const submit = dialog.getByRole("button", { name: "Dismiss" });
  expect(submit, "an empty reason cannot be sent").toHaveAttribute("aria-disabled", "true");
  expect(submit).toHaveAccessibleDescription("Say why first.");

  const quiet = dialog.getByLabelText("Keep the check quiet for");
  expect(
    dialog.getByText(
      "Creates a known condition: matching incidents are kept in the history and never notify, for 1 day.",
    ),
  ).toBeVisible();
  await user.selectOptions(quiet, "30 days");
  expect(dialog.getByText(/never notify, for 30 days\.$/)).toBeVisible();
  await user.selectOptions(quiet, "Always");
  expect(dialog.getByText(/never notify, until you remove it\.$/)).toBeVisible();
  await user.selectOptions(quiet, "No");
  expect(
    dialog.getByText(
      "Closes this incident now and creates no known condition. The check alerts again the next time it fires.",
    ),
  ).toBeVisible();

  await user.type(
    dialog.getByLabelText(/^Why is this not a real problem\?/),
    "The nightly backup fills it, and it clears on its own.",
  );
  expect(submit).not.toHaveAttribute("aria-disabled");
  await user.click(submit);
  const dismiss = calls.find((call) => call.route === "POST /api/v1/incidents/{id}/dismiss");
  expect(dismiss?.init.body).toEqual({
    reason: "The nightly backup fills it, and it clears on its own.",
    ignore_for_minutes: null,
  });
});

test("dismiss_dialog_counts_characters_near_the_limit_and_keeps_a_failure_inside", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/incidents/{id}", () => detail());
  route("POST /api/v1/incidents/{id}/dismiss", () =>
    failure(409, "conflict", "the incident already resolved"),
  );
  open();

  const dialog = within(await openDismiss(user));
  const reason = dialog.getByLabelText(/^Why is this not a real problem\?/);
  expect(reason).toHaveAttribute("maxlength", "500");
  await user.click(reason);
  await user.paste("a".repeat(420));
  expect(dialog.getByText("420 of 500 characters")).toBeVisible();

  await user.click(dialog.getByRole("button", { name: "Dismiss" }));
  expect(await dialog.findByText("Could not dismiss the incident")).toBeVisible();
  expect(dialog.getByText("The incident already resolved.")).toBeVisible();
});

test("dismiss_is_offered_only_after_the_incident_is_confirmed", async () => {
  route("GET /api/v1/incidents/{id}", () => ({
    ...detail(),
    incident: incident({ status: "pending" }),
  }));
  open();

  expect(await screen.findByText("Backups filled /srv")).toBeVisible();
  expect(screen.getByRole("button", { name: "Acknowledge" })).toBeVisible();
  expect(
    screen.queryByRole("button", { name: "Dismiss" }),
    "dismissing a pending incident is refused, so the page offers no refusal",
  ).toBeNull();
});

test("an_action_that_finishes_while_the_page_is_open_is_announced", async () => {
  const d = detail();
  let status = "dispatched";
  route("GET /api/v1/incidents/{id}", () => ({ ...d, actions: [action({ status })] }));
  const { client } = open();

  expect(await screen.findByText("Running on nas. Waiting for the result…")).toBeVisible();
  status = "succeeded";
  void client.invalidateQueries({ queryKey: ["incidents"] });
  expect(await screen.findByText("restart_container succeeded on nas")).toBeVisible();
});
