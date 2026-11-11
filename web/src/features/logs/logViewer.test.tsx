import { fireEvent, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { LogLine } from "../../api/types";
import { renderAt } from "../../test/render";
import { LogViewer } from "./LogViewer";
import { PAGE_LINES, levelKind, lineMatches, splitMatches } from "./logLines";

/** `count` numbered info lines, oldest first. */
function manyLines(count: number): LogLine[] {
  return Array.from({ length: count }, (_, index) => ({
    at: "2026-11-01T12:00:00Z",
    level: "info",
    target: "hub.work",
    message: `step ${index + 1} done`,
  }));
}

const WARNING_LINE: LogLine = {
  at: "2026-11-01T12:00:01Z",
  level: "warn",
  target: "hub.checks",
  message: "disk is 91% full",
};

const THREE: LogLine[] = [
  { at: "2026-11-01T12:00:00Z", level: "info", target: "hub.runtime", message: "started" },
  WARNING_LINE,
  { at: "2026-11-01T12:00:02Z", level: "error", target: "hub.chat", message: "gateway rejected" },
];

function show(data: LogLine[] | undefined, refreshMs: number | null, onLiveChange?: () => void) {
  const query = { data, error: null, isPending: data === undefined, refetch: vi.fn() };
  renderAt(
    "/",
    "/",
    <LogViewer
      query={query}
      level="info"
      label="Test log lines"
      refreshMs={refreshMs}
      {...(onLiveChange ? { onLiveChange } : {})}
    />,
  );
  return screen.queryByRole("log", { name: "Test log lines" });
}

test("level_kinds_fold_common_spellings_and_keep_unknown_levels_plain", () => {
  expect(levelKind("ERROR")).toBe("error");
  expect(levelKind("fatal")).toBe("error");
  expect(levelKind("warning")).toBe("warn");
  expect(levelKind("trace")).toBe("debug");
  expect(levelKind("notice")).toBe("info");
});

test("a_search_matches_any_part_of_a_line_and_marks_the_matches", () => {
  expect(lineMatches(WARNING_LINE, "DISK")).toBe(true);
  expect(lineMatches(WARNING_LINE, "hub.checks")).toBe(true);
  expect(lineMatches(WARNING_LINE, "gateway")).toBe(false);
  expect(splitMatches("disk is 91% full", "9")).toEqual([
    { match: false, text: "disk is " },
    { match: true, text: "9" },
    { match: false, text: "1% full" },
  ]);
  expect(splitMatches("a.b", ".")).toEqual([
    { match: false, text: "a" },
    { match: true, text: "." },
    { match: false, text: "b" },
  ]);
});

test("the_auto_refresh_line_is_hidden_when_refresh_ms_is_null", () => {
  show(THREE, null);
  expect(screen.queryByText(/Auto-refresh every/)).toBeNull();
});

test("the_auto_refresh_line_names_the_interval_when_refresh_ms_is_set", () => {
  show(THREE, 5_000);
  expect(screen.getByText("Auto-refresh every 5 s")).toBeVisible();
});

test("the_live_switch_reports_what_the_reader_chose", async () => {
  const onLiveChange = vi.fn();
  show(THREE, 5_000, onLiveChange);
  const live = screen.getByRole("switch", { name: "Live" });
  expect(live).toBeChecked();
  await userEvent.setup().click(live);
  expect(onLiveChange).toHaveBeenCalledWith(false);
});

test("searching_keeps_only_the_matching_lines_and_marks_the_match", async () => {
  const user = userEvent.setup();
  show(THREE, null);
  expect(screen.getByText(/gateway rejected/)).toBeVisible();

  await user.type(screen.getByRole("searchbox", { name: "Search lines" }), "disk");
  expect(screen.queryByText(/gateway rejected/)).toBeNull();
  expect(screen.queryByText(/started/)).toBeNull();
  const marks = screen.getAllByText("disk", { selector: "mark" });
  expect(marks).toHaveLength(1);

  await user.clear(screen.getByRole("searchbox", { name: "Search lines" }));
  expect(screen.getByText(/gateway rejected/)).toBeVisible();
});

test("a_search_with_no_match_says_so_instead_of_showing_an_empty_box", async () => {
  const user = userEvent.setup();
  show(THREE, null);
  await user.type(screen.getByRole("searchbox", { name: "Search lines" }), "zzz");
  expect(screen.getByText("No log lines")).toBeVisible();
  expect(screen.getByText(/matches the search/)).toBeVisible();
  expect(screen.queryByRole("log", { name: "Test log lines" })).toBeNull();
});

test("a_long_log_draws_only_the_newest_lines_until_show_older_is_pressed", async () => {
  const user = userEvent.setup();
  const log = show(manyLines(10_000), null);
  expect(log?.querySelectorAll("[data-level]")).toHaveLength(PAGE_LINES);
  expect(screen.getByText(/step 10000 done/)).toBeVisible();
  expect(screen.queryByText(/step 9500 done/)).toBeNull();
  expect(screen.getByText(`${PAGE_LINES} of 10000 lines`)).toBeVisible();

  await user.click(screen.getByRole("button", { name: "Show older" }));
  expect(log?.querySelectorAll("[data-level]")).toHaveLength(PAGE_LINES * 2);
  expect(screen.getByText(/step 9500 done/)).toBeVisible();
});

test("jump_to_latest_appears_only_after_scrolling_up_from_the_bottom", () => {
  const log = show(manyLines(50), null);
  expect(screen.queryByRole("button", { name: "Jump to latest" })).toBeNull();
  if (!log) {
    throw new Error("the log area did not render");
  }
  Object.defineProperty(log, "scrollHeight", { configurable: true, value: 2000 });
  Object.defineProperty(log, "clientHeight", { configurable: true, value: 300 });
  log.scrollTop = 0;
  fireEvent.scroll(log);
  const jump = screen.getByRole("button", { name: "Jump to latest" });

  log.scrollTop = 1700;
  fireEvent.scroll(log);
  expect(jump).not.toBeInTheDocument();
});

test("the_wrap_switch_folds_long_lines", async () => {
  const user = userEvent.setup();
  const log = show(THREE, null);
  expect(log).not.toHaveAttribute("data-wrap");
  await user.click(screen.getByRole("switch", { name: "Wrap lines" }));
  expect(log).toHaveAttribute("data-wrap", "true");
});

test("copy_visible_lines_puts_the_drawn_lines_on_the_clipboard", async () => {
  const user = userEvent.setup();
  show(THREE, null);
  await user.click(screen.getByRole("button", { name: "Copy visible lines" }));
  const copied = await navigator.clipboard.readText();
  expect(copied.split("\n")).toHaveLength(3);
  expect(copied).toMatch(/ERROR hub\.chat: gateway rejected/);
  expect(await screen.findByText("Copied")).toBeVisible();
});
