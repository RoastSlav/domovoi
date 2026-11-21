import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SIMULATE_BOTH, VERSION_3 } from "../../test/policyFixtures";
import { renderAt } from "../../test/render";
import { PolicyNotes } from "./PolicyNotes";
import { RulesTable } from "./RulesTable";
import { SimulationSummary } from "./SimulationSummary";

function table(mode: "view" | "edit" = "view", handlers = {}) {
  renderAt(
    "/p",
    "/p",
    <RulesTable
      rules={VERSION_3.rules}
      hostNames={{}}
      zone="Europe/Berlin"
      mode={mode}
      {...handlers}
    />,
  );
}

test("locked_deny_reads_differently_from_a_plain_deny_in_the_rules_table", () => {
  table();
  expect(screen.getByText("Locked Deny")).toBeInTheDocument();
  expect(screen.getByText("Shadow")).toBeInTheDocument();
  expect(screen.getByText("Disabled")).toBeInTheDocument();
  expect(screen.getByText(/Order does not matter/)).toBeInTheDocument();
});

test("rules_table_edit_mode_offers_edit_duplicate_remove_in_a_row_menu", async () => {
  const onEdit = vi.fn();
  table("edit", { onEdit, onDuplicate: vi.fn(), onRemove: vi.fn() });
  const first = VERSION_3.rules.map((r) => r.id)[0] ?? "";
  await userEvent.click(screen.getByRole("button", { name: `Actions for ${first}` }));
  const menu = screen.getByRole("menu");
  expect(within(menu).getByRole("menuitem", { name: "Duplicate" })).toBeInTheDocument();
  expect(within(menu).getByRole("menuitem", { name: "Remove" })).toBeInTheDocument();
  await userEvent.click(within(menu).getByRole("menuitem", { name: "Edit" }));
  expect(onEdit).toHaveBeenCalledWith(first);
});

test("simulation_summary_shows_every_count", () => {
  const counts = {
    evaluated: 11,
    same: 7,
    changed: 4,
    not_replayable: 3,
    newly_allowed: 2,
    newly_denied: 1,
    newly_requires_approval: 5,
  };
  renderAt(
    "/p",
    "/p",
    <SimulationSummary summary={{ ...SIMULATE_BOTH.summary, ...counts }} label="Recorded" />,
  );
  for (const label of [
    "Evaluated",
    "Same",
    "Changed",
    "Cannot replay",
    "Would become Allow automatically",
    "Would become Deny",
    "Would ask for approval",
  ]) {
    expect(screen.getByText(label)).toBeInTheDocument();
  }
  for (const n of ["11", "7", "4", "3", "2", "1", "5"]) {
    expect(screen.getAllByText(n).length).toBeGreaterThan(0);
  }
  expect(screen.getByText("Recorded")).toBeInTheDocument();
});

test("policy_notes_show_the_hub_text_verbatim", () => {
  renderAt("/p", "/p", <PolicyNotes notes={["Shadow rules are recorded only.", "Second note."]} />);
  expect(screen.getByText("Shadow rules are recorded only.")).toBeInTheDocument();
  expect(screen.getByText("Second note.")).toBeInTheDocument();
});
