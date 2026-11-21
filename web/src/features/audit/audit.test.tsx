import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { auditRow, verification } from "../../test/fixtures";
import { expectP0Everywhere, renderWithProfile } from "../../test/visibility";
import { renderAt } from "../../test/render";
import { AuditPage } from "./AuditPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const CHAINED = [auditRow({ seq: 7 }), auditRow({ seq: 6, kind: "audit.checkpoint" })];
const LEGACY = [auditRow({ seq: 2, history: "legacy", hash: null })];

function serve(check = verification(), rows = CHAINED) {
  route("GET /api/v1/audit/verification", (init) =>
    (init.params as { query?: { scope?: string } } | undefined)?.query?.scope === "full"
      ? verification({
          scope: "full",
          legacy: { ...check.legacy, status: "verified" },
          chained: check.chained,
        })
      : check,
  );
  route("GET /api/v1/audit", (init) =>
    (init.params as { query?: { history?: string } } | undefined)?.query?.history === "legacy"
      ? { legacy_last_id: 5, next_cursor: null, items: LEGACY }
      : { legacy_last_id: 5, next_cursor: null, items: rows },
  );
}

const view = (url = "/audit") => renderAt(url, "/audit", <AuditPage />);

beforeEach(() => resetApi());

test("audit_page_shows_both_histories_separately_with_their_own_status", async () => {
  serve();
  view();
  expect(await screen.findByRole("heading", { name: "Chained history" })).toBeVisible();
  expect(screen.getByRole("heading", { name: "Legacy history" })).toBeVisible();
  expect(screen.getByText("Intact")).toBeVisible();
  expect(screen.getByText("Count matches")).toBeVisible();
  expect(screen.getByText(/digest was not compared/)).toBeVisible();
});

test("the_head_hash_is_copyable_and_comes_with_the_off_host_advice", async () => {
  serve();
  view();
  const block = await screen.findByLabelText("Head hash");
  expect(block).toHaveTextContent("9f".repeat(32));
  expect(screen.getByText(/Keep a copy somewhere off this host/)).toBeVisible();
  expect(screen.getByText("What this check can show")).toBeVisible();
});

test("the_whole_log_check_runs_only_on_request_and_updates_both_cards", async () => {
  const user = userEvent.setup();
  serve();
  view();
  await screen.findByText("Count matches");
  expect(JSON.stringify(calls)).not.toContain('"scope":"full"');
  await user.click(screen.getByRole("button", { name: "Check the whole log" }));
  expect(await screen.findByText("Verified")).toBeVisible();
  expect(screen.getByRole("button", { name: "Check again" })).toBeVisible();
});

test("a_failed_whole_log_check_keeps_the_recent_result_and_offers_retry", async () => {
  const user = userEvent.setup();
  serve();
  route("GET /api/v1/audit/verification", (init) =>
    (init.params as { query?: { scope?: string } } | undefined)?.query?.scope === "full"
      ? failure(500, "internal", "boom")
      : verification(),
  );
  view();
  await screen.findByText("Count matches");
  await user.click(screen.getByRole("button", { name: "Check the whole log" }));
  expect(await screen.findByText("Could not check the whole log")).toBeVisible();
  expect(screen.getByText("Count matches")).toBeVisible();
});

test("a_failed_verification_is_a_load_error_never_a_status", async () => {
  route("GET /api/v1/audit/verification", () => failure(500, "internal", "boom"));
  route("GET /api/v1/audit", () => ({ legacy_last_id: 0, next_cursor: null, items: [] }));
  view();
  expect(await screen.findByText("Could not check the audit log")).toBeVisible();
  expect(screen.queryByText("Intact")).toBeNull();
  expect(screen.queryByText("Verified")).toBeNull();
});

test("a_broken_chain_names_the_entry_and_the_reason_and_marks_the_row", async () => {
  const base = verification();
  serve(
    verification({
      chained: {
        ...base.chained,
        status: "broken",
        first_broken_seq: 7,
        break_reason: "hash_mismatch",
        break_text: "a row was edited",
        head_seq: null,
        head_hash: null,
      },
    }),
  );
  view();
  expect(await screen.findByText("Broken at entry #7")).toBeVisible();
  expect(screen.getByText(/An entry was edited\./)).toBeVisible();
  expect(await screen.findByText("First broken entry")).toBeVisible();
  expect(screen.getByLabelText("Head hash")).toBeVisible();
});

test("a_changed_legacy_history_says_it_cannot_say_which_entry", async () => {
  const base = verification();
  serve(
    verification({
      legacy: {
        ...base.legacy,
        status: "mismatch",
        count: null,
        expected_count: 5,
        actual_count: 4,
        digest_matches: false,
      },
    }),
  );
  view();
  expect(await screen.findByText(/Expected 5 entries and found 4/)).toBeVisible();
  expect(screen.getByText(/digest differs/)).toBeVisible();
});

test("a_fresh_install_says_there_are_no_legacy_entries", async () => {
  const base = verification();
  serve(verification({ anchor: { ...base.anchor, legacy_count: 0, legacy_last_id: 0 } }), []);
  view("/audit?tab=legacy");
  expect(
    await screen.findByText("This install has no entries from before the chain."),
  ).toBeVisible();
});

test("entries_open_a_dialog_with_the_hash_and_the_detail_as_text", async () => {
  const user = userEvent.setup();
  serve(verification(), [auditRow({ seq: 7, detail: "<b>x</b>", detail_truncated: true })]);
  view();
  await user.click(await screen.findByRole("button", { name: "Open entry 7" }));
  const dialog = await screen.findByRole("dialog", { name: "Audit entry #7" });
  expect(within(dialog).getByLabelText("Entry detail")).toHaveTextContent("<b>x</b>");
  expect(within(dialog).getByText("Shortened to 4 KiB.")).toBeVisible();
  expect(within(dialog).getByText(/9f9f9f/)).toBeVisible();
  await user.keyboard("{Escape}");
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("the_legacy_tab_lists_legacy_entries_and_says_none_for_the_hash", async () => {
  const user = userEvent.setup();
  serve();
  view();
  await user.click(await screen.findByRole("tab", { name: "Legacy history" }));
  await user.click(await screen.findByRole("button", { name: "Open entry 2" }));
  expect(
    await screen.findByText("None. This entry was written before the chain began."),
  ).toBeVisible();
});

test("a_failed_entries_load_is_a_load_error_not_an_empty_table", async () => {
  route("GET /api/v1/audit/verification", () => verification());
  route("GET /api/v1/audit", () => failure(500, "internal", "boom"));
  view();
  expect(await screen.findByText("Could not load audit entries")).toBeVisible();
});

test("p0_visible_in_every_profile_on_the_audit_page", async () => {
  serve();
  await expectP0Everywhere({
    expectedIds: ["audit.verification", "audit.limits", "audit.entries"],
    render: async (profile) => {
      const view = renderWithProfile(profile, "/audit", "/audit", <AuditPage />);
      await screen.findByText("Intact");
      await screen.findByRole("button", { name: "Open entry 7" });
      return view;
    },
  });
});

test("standard_collapses_the_anchor_details_only", async () => {
  serve();
  renderWithProfile("standard", "/audit", "/audit", <AuditPage />);
  await screen.findByText("Intact");
  expect(screen.getByRole("button", { name: /Anchor details/ })).toBeVisible();
  expect(screen.queryByText("Install ID")).toBeNull();
  expect(screen.getByText("What this check can show")).toBeVisible();
});
