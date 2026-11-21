import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Host, PolicyOverview, PolicySimulation } from "../../api/types";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { OVERVIEW_ACTIVE, OVERVIEW_FRESH, SIMULATE_BOTH } from "../../test/policyFixtures";
import { renderAt } from "../../test/render";
import { SimulatorPage, untilOf } from "./SimulatorPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const SIMULATE = "POST /api/v1/policy/simulate";
const HOST_ID = "0192a000-0000-7000-8000-000000000001";
const NAS = { id: HOST_ID, name: "nas", kind: "agent", connected: true } as unknown as Host;

function show(url = "/policy/simulator", overview: PolicyOverview = OVERVIEW_ACTIVE) {
  route("GET /api/v1/policy", () => overview);
  route("GET /api/v1/policy/versions", () => ({
    items: overview.active ? [overview.active] : [],
    next_cursor: null,
  }));
  route("GET /api/v1/hosts", () => [NAS]);
  route(SIMULATE, () => SIMULATE_BOTH);
  return renderAt(url, "/policy/simulator", <SimulatorPage />);
}

const posts = () => calls.filter((c) => c.route === SIMULATE);

async function run() {
  await userEvent.click(await screen.findByRole("button", { name: "Run simulation" }));
}

beforeEach(resetApi);

test("simulator_defaults_to_the_draft_both_selections_and_recorded_inputs_and_runs_on_click", async () => {
  show();
  expect(await screen.findByRole("radio", { name: /Draft, revision/ })).toBeChecked();
  expect(screen.getByRole("radio", { name: "Both" })).toBeChecked();
  expect(screen.getByRole("radio", { name: "Recorded" })).toBeChecked();
  expect(screen.getByRole("spinbutton", { name: /Most results/ })).toHaveValue(200);
  expect(posts()).toHaveLength(0);
  await run();
  await screen.findByText("draft revision 4");
  expect(posts()).toHaveLength(1);
  expect(posts()[0]?.init.body).toEqual({
    rules: { source: "draft" },
    selection: "both",
    inputs: "recorded",
    filter: { status: "evaluated" },
    execution_ids: [],
    limit: 200,
  });
});

test("simulator_labels_current_inputs_as_a_what_if_and_recorded_inputs_with_their_gaps", async () => {
  show("/policy/simulator?inputs=current");
  await run();
  expect(
    await screen.findByText(
      "Current inputs. This is a what-if, not a reproduction of what happened.",
    ),
  ).toBeInTheDocument();
  expect(posts()[0]?.init.body).toMatchObject({ inputs: "current" });
});

test("simulator_labels_recorded_inputs_with_their_gaps", async () => {
  show();
  await run();
  expect(
    await screen.findByText(/Recorded inputs\. Where an input was not available/),
  ).toBeInTheDocument();
});

test("simulator_shows_baseline_simulated_and_comparison_per_item_and_the_summary", async () => {
  show();
  await run();
  const table = await screen.findByRole("table", { name: "Simulated decisions" });
  const rows = within(table).getAllByRole("row");
  expect(rows).toHaveLength(SIMULATE_BOTH.items.length + 1);
  const first = within(rows[1] as HTMLElement);
  expect(first.getAllByText("Ask for approval").length).toBeGreaterThan(0);
  expect(first.getAllByText("Allow automatically").length).toBeGreaterThan(0);
  expect(first.getByText("Changed")).toBeInTheDocument();
  expect(first.getByText("Same")).toBeInTheDocument();
  expect(first.getByText("nas")).toBeInTheDocument();
  expect(first.getByRole("link", { name: "skill.restart_plex" })).toHaveAttribute(
    "href",
    `/policy/decisions/${SIMULATE_BOTH.items[0]?.execution_id}`,
  );
  expect(screen.getByText("Evaluated")).toBeInTheDocument();
  expect(screen.getByText("Would become Deny")).toBeInTheDocument();
});

test("simulator_warns_when_decisions_would_become_allow_automatically", async () => {
  show();
  await run();
  expect(
    await screen.findByText("1 decisions would become Allow automatically"),
  ).toBeInTheDocument();
});

test("simulator_does_not_warn_when_nothing_would_become_allow", async () => {
  const quiet: PolicySimulation = {
    ...SIMULATE_BOTH,
    summary: { ...SIMULATE_BOTH.summary, newly_allowed: 0 },
  };
  show();
  route(SIMULATE, () => quiet);
  await run();
  await screen.findByText("draft revision 4");
  expect(screen.queryByText(/would become Allow automatically$/)).not.toBeInTheDocument();
});

test("simulator_shows_notes_truncation_and_not_replayable_counts", async () => {
  show();
  route(SIMULATE, () => ({ ...SIMULATE_BOTH, truncated: true }));
  await run();
  for (const note of SIMULATE_BOTH.notes) {
    expect(await screen.findByText(note)).toBeInTheDocument();
  }
  expect(screen.getByText(/2 could not be replayed/)).toBeInTheDocument();
  expect(
    screen.getByText(
      `Showing the newest ${SIMULATE_BOTH.items.length} decisions. Narrow the filter to see others.`,
    ),
  ).toBeInTheDocument();
});

test("selecting_a_result_shows_recorded_and_simulated_explanations_side_by_side", async () => {
  show();
  await run();
  const links = await screen.findAllByRole("link", { name: "skill.restart_plex" });
  const link = links[0] as HTMLElement;
  const row = link.closest("tr") as HTMLElement;
  await userEvent.click(within(row).getByText("nas"));
  await waitFor(() => expect(screen.getAllByRole("group", { name: "Recorded" }).length).toBe(2));
  expect(screen.getAllByRole("group", { name: "Simulated" }).length).toBe(2);
  expect(screen.getByText("Allow overrides a broader Deny")).toBeInTheDocument();
  expect(screen.getByTestId("location")).toHaveTextContent(
    `item=${SIMULATE_BOTH.items[0]?.execution_id}`,
  );
});

test("selecting_a_result_that_cannot_be_replayed_says_so", async () => {
  const id = SIMULATE_BOTH.items[2]?.execution_id;
  show(`/policy/simulator?item=${id}`);
  await run();
  expect(
    (await screen.findAllByText(/could not be replayed, so there is no simulated/)).length,
  ).toBeGreaterThan(0);
});

test("simulator_can_be_started_for_one_step_from_the_decision_page", async () => {
  const id = SIMULATE_BOTH.items[0]?.execution_id ?? "";
  show(`/policy/simulator?execution=${id}`);
  expect(await screen.findByText(`Only step ${id.slice(0, 8)}`)).toBeInTheDocument();
  await run();
  await screen.findByText("draft revision 4");
  expect(posts()[0]?.init.body).toMatchObject({ execution_ids: [id] });
});

test("simulator_without_a_draft_or_version_says_nothing_to_simulate_and_disables_the_draft_option", async () => {
  show("/policy/simulator", OVERVIEW_FRESH);
  expect(await screen.findByText("Nothing to simulate yet")).toBeInTheDocument();
  expect(screen.getByRole("link", { name: "Start a draft" })).toHaveAttribute(
    "href",
    "/policy/draft",
  );
  expect(posts()).toHaveLength(0);
});

test("simulator_disables_the_draft_option_when_only_a_version_exists", async () => {
  show("/policy/simulator", { ...OVERVIEW_ACTIVE, draft: null });
  expect(await screen.findByRole("radio", { name: "Active version 3" })).toBeChecked();
  expect(screen.getByRole("radio", { name: /Draft/ })).toBeDisabled();
  expect(screen.queryByRole("combobox", { name: /Older version/ })).not.toBeInTheDocument();
});

test("simulator_failures_are_inline_in_the_hubs_words", async () => {
  show();
  route(SIMULATE, () => {
    return failure(404, "not_found", "no draft");
  });
  await run();
  expect(
    await screen.findByText("It no longer exists. It may have been removed."),
  ).toBeInTheDocument();
  route(SIMULATE, () => {
    return failure(400, "bad_request", "The date range is not valid.");
  });
  await run();
  expect(await screen.findByText("The date range is not valid.")).toBeInTheDocument();
  expect(
    screen.queryByText("1 decisions would become Allow automatically"),
  ).not.toBeInTheDocument();
});

test("a_decision_that_could_not_be_replayed_still_shows_what_was_recorded", async () => {
  const id = SIMULATE_BOTH.items[3]?.execution_id;
  show(`/policy/simulator?item=${id}`);
  await run();
  expect(
    (await screen.findAllByText(/so there is no simulated result to compare\. The recorded/))
      .length,
  ).toBeGreaterThan(0);
  expect(screen.getAllByRole("group", { name: "Recorded" }).length).toBeGreaterThan(0);
  expect(screen.queryByRole("group", { name: "Simulated" })).not.toBeInTheDocument();
});

test("results_are_marked_as_earlier_when_the_settings_change_after_a_run", async () => {
  show();
  await run();
  await screen.findByText("draft revision 4");
  const warning = /These results are from an earlier run\. Run the simulation again/;
  expect(screen.queryByText(warning)).not.toBeInTheDocument();
  await userEvent.selectOptions(screen.getByRole("combobox", { name: /^Status/ }), "");
  expect(screen.getByText(warning)).toBeInTheDocument();
  expect(screen.getByText("draft revision 4")).toBeInTheDocument();
  await userEvent.selectOptions(screen.getByRole("combobox", { name: /^Status/ }), "evaluated");
  expect(screen.queryByText(warning)).not.toBeInTheDocument();
});

test("a_slower_older_run_does_not_replace_a_newer_result", async () => {
  show();
  let release: (() => void) | undefined;
  const slow = new Promise<void>((resolve) => (release = resolve));
  let n = 0;
  route(SIMULATE, async () => {
    n += 1;
    if (n === 1) {
      await slow;
      return { ...SIMULATE_BOTH, label: "older run" };
    }
    return { ...SIMULATE_BOTH, label: "newer run" };
  });
  await run();
  fireEvent.submit(
    screen.getByRole("button", { name: "Run simulation" }).closest("form") as HTMLFormElement,
  );
  await screen.findByText("newer run");
  release?.();
  await waitFor(() => expect(posts()).toHaveLength(2));
  await new Promise((r) => setTimeout(r, 20));
  expect(screen.getByText("newer run")).toBeInTheDocument();
  expect(screen.queryByText("older run")).not.toBeInTheDocument();
});

test("the_filters_are_labelled_optional_and_counted_when_set", async () => {
  show();
  expect(await screen.findByRole("heading", { name: "What to test" })).toBeInTheDocument();
  expect(screen.getByRole("heading", { name: "Optional: narrow it down" })).toBeInTheDocument();
  expect(screen.getByText("Test these rules").parentElement).toHaveTextContent("(required)");
  for (const name of [/^Status/, /^Action \(optional\)/, /^From \(UTC\) \(optional\)/]) {
    expect(screen.getByLabelText(name)).toBeInTheDocument();
  }
  expect(screen.getByText("Most results (optional)")).toBeInTheDocument();
  expect(screen.queryByText(/filters? set/)).not.toBeInTheDocument();
  fireEvent.change(screen.getByLabelText("From (UTC) (optional)"), {
    target: { value: "2026-03-01" },
  });
  await userEvent.selectOptions(screen.getByRole("combobox", { name: /^Status/ }), "");
  expect(screen.getByText("2 filters set")).toBeInTheDocument();
  await run();
  expect(posts()[0]?.init.body).toMatchObject({ filter: { since: "2026-03-01T00:00:00Z" } });
});

test("few_hosts_are_choices_and_the_chosen_host_is_sent", async () => {
  show();
  await userEvent.click(await screen.findByRole("radio", { name: "nas" }));
  await run();
  expect(posts()[0]?.init.body).toMatchObject({ filter: { host: HOST_ID } });
});

test("the_against_choice_sends_the_same_selection_as_before", async () => {
  show("/policy/simulator?selection=shadow");
  expect(await screen.findByRole("radio", { name: "The shadow rules" })).toBeChecked();
  await userEvent.click(screen.getByRole("radio", { name: "The active policy" }));
  expect(screen.getByTestId("location")).toHaveTextContent("selection=enforced");
  await run();
  expect(posts()[0]?.init.body).toMatchObject({ selection: "enforced" });
});

test("the_form_has_no_overlapping_controls_structure", async () => {
  show();
  await screen.findByRole("radio", { name: "Both" });
  expect(screen.queryByRole("combobox", { name: "Against" })).not.toBeInTheDocument();
  expect(screen.getByRole("group", { name: /^Against/ })).toBeInTheDocument();
  expect(screen.getByRole("group", { name: "Using" })).toBeInTheDocument();
});

test("the_to_date_includes_the_whole_selected_day", () => {
  expect(untilOf("2026-03-14")).toBe("2026-03-15T00:00:00Z");
  expect(untilOf("2026-01-31")).toBe("2026-02-01T00:00:00Z");
  expect(untilOf("2026-12-31")).toBe("2027-01-01T00:00:00Z");
  expect(untilOf("2028-02-28")).toBe("2028-02-29T00:00:00Z");
  expect(untilOf("2028-02-29")).toBe("2028-03-01T00:00:00Z");
});
