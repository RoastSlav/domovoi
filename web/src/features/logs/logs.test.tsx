import { act, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { LOGS_REFRESH_MS } from "../../api/hooks/system";
import type { LogLine } from "../../api/types";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { T0, nas } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { LogsPage } from "./LogsPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

/** How many times the mocked endpoint answered, so a refetch can answer with more lines. */
let answered = 0;

/** The hub's own lines, oldest first, as the endpoint answers them. */
function served(): LogLine[] {
  return [
    { at: T0, level: "info", target: "domovoi_hub.runtime", message: "hub started hosts=1" },
    {
      at: T0,
      level: "warn",
      target: "domovoi_hub.checks",
      message: "disk.usage on /srv is 91% full",
    },
    {
      at: T0,
      level: "error",
      target: "domovoi_hub.chat",
      message: "gateway rejected key=[REDACTED]",
    },
  ];
}

/** One more line, as it is only there once the page asked again. */
function later(): LogLine {
  return {
    at: "2026-10-31T12:00:20Z",
    level: "info",
    target: "domovoi_hub.triage",
    message: "triage reported inc-1",
  };
}

function logQueries() {
  return calls
    .filter((call) => call.route === "GET /api/v1/system/logs")
    .map((call) => call.init.params?.query);
}

beforeEach(() => {
  resetApi();
  answered = 0;
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/system/logs", () =>
    (answered += 1) === 1 ? served() : [...served(), later()],
  );
});

afterEach(() => {
  vi.useRealTimers();
});

test("logs_page_lists_the_hub_lines_oldest_first_with_their_level", async () => {
  renderAt("/logs", "/logs", <LogsPage />);
  await screen.findByText(/hub started hosts=1/);
  const log = screen.getByRole("log", { name: "Hub log lines" });
  const rows = [...log.querySelectorAll("[data-level]")];
  expect(rows, "one element per line").toHaveLength(3);
  expect(rows[0]).toHaveTextContent(/INFO domovoi_hub\.runtime: hub started hosts=1/);
  expect(rows[1]).toHaveTextContent(/WARN domovoi_hub\.checks: disk\.usage on \/srv is 91% full/);
  expect(rows[2]).toHaveTextContent(/ERROR domovoi_hub\.chat: gateway rejected key=\[REDACTED\]/);
  expect(rows.map((row) => row.getAttribute("data-level"))).toEqual(["info", "warn", "error"]);
  expect(screen.getByRole("button", { name: "Refresh" })).toBeVisible();
});

test("logs_page_keeps_its_shape_while_the_first_lines_load", async () => {
  route("GET /api/v1/system/logs", () => new Promise(() => undefined));
  renderAt("/logs", "/logs", <LogsPage />);
  expect(await screen.findByText("Loading…")).toBeInTheDocument();
  expect(screen.getByRole("searchbox", { name: "Search lines" })).toBeVisible();
  expect(screen.queryByRole("log", { name: "Hub log lines" })).toBeNull();
  expect(screen.queryByText(/No log lines/)).toBeNull();
});

test("log_selects_move_the_level_and_line_count_into_the_query", async () => {
  const user = userEvent.setup();
  renderAt("/logs?level=warn", "/logs", <LogsPage />);
  expect(logQueries()).toEqual([{ level: "warn", lines: 200 }]);

  await user.selectOptions(screen.getByLabelText("Level and above"), "error");
  expect(screen.getByTestId("location")).toHaveTextContent("/logs?level=error");
  expect(await screen.findByText(/gateway rejected/)).toBeVisible();

  await user.selectOptions(screen.getByLabelText("Lines"), "1000");
  expect(screen.getByTestId("location")).toHaveTextContent("/logs?level=error&lines=1000");
  expect(logQueries().slice(1)).toEqual([
    { level: "error", lines: 200 },
    { level: "error", lines: 1000 },
  ]);
});

test("logs_refresh_button_asks_for_the_lines_again", async () => {
  const user = userEvent.setup();
  renderAt("/logs", "/logs", <LogsPage />);
  await screen.findByText(/gateway rejected/);
  expect(screen.queryByText(/triage reported/)).toBeNull();

  await user.click(screen.getByRole("button", { name: "Refresh" }));
  expect(logQueries()).toHaveLength(2);
  expect(await screen.findByText(/triage reported inc-1/)).toBeVisible();
});

test("logs_page_shows_the_hub_s_words_when_nothing_was_captured", async () => {
  route("GET /api/v1/system/logs", () => failure(503, "unavailable", "logs are not captured"));
  renderAt("/logs", "/logs", <LogsPage />);
  const alert = await screen.findByRole("alert");
  expect(alert).toHaveTextContent("Could not load logs");
  expect(alert).toHaveTextContent("Logs are not captured.");
  expect(screen.getByRole("button", { name: "Retry" })).toBeVisible();
  expect(screen.queryByText(/No log lines/)).toBeNull();
});

test("logs_page_says_why_the_hub_logged_nothing_to_show", async () => {
  route("GET /api/v1/system/logs", () => []);
  renderAt("/logs?level=debug", "/logs", <LogsPage />);
  expect(await screen.findByText(/Nothing was logged at debug or above/)).toBeVisible();
  expect(screen.getByText("No log lines")).toBeVisible();
});

test("switching_the_log_source_to_a_host_reads_that_host_s_logs_endpoint", async () => {
  route("GET /api/v1/hosts/{name}/logs", () => [
    {
      at: T0,
      level: "warn",
      target: "agent.checks",
      message: "disk.usage on /srv is 97% full",
    },
  ]);
  const user = userEvent.setup();
  renderAt("/logs", "/logs", <LogsPage />);
  await screen.findByText(/hub started hosts=1/);

  await user.click(screen.getByRole("combobox", { name: "Source" }));
  await user.click(await screen.findByRole("option", { name: /nas/ }));
  expect(screen.getByTestId("location")).toHaveTextContent("/logs?source=nas");
  expect(await screen.findByText(/disk\.usage on \/srv is 97% full/)).toBeVisible();
  const hostCalls = calls.filter((entry) => entry.route === "GET /api/v1/hosts/{name}/logs");
  expect(hostCalls.length, "the host's lines come from its own endpoint").toBeGreaterThan(0);
  expect(hostCalls[0]?.init.params).toEqual({
    path: { name: "nas" },
    query: { level: "info", lines: 200 },
  });
});

test("the_source_picker_shows_whether_each_host_is_connected", async () => {
  const user = userEvent.setup();
  renderAt("/logs", "/logs", <LogsPage />);
  await screen.findByText(/hub started hosts=1/);
  await user.click(screen.getByRole("combobox", { name: "Source" }));
  const option = await screen.findByRole("option", { name: /nas/ });
  expect(within(option).getByText(/connected|offline/i)).toBeVisible();
  expect(screen.getByRole("option", { name: "Hub" })).toBeVisible();
});

test("logs_page_asks_to_update_an_agent_too_old_to_send_its_lines", async () => {
  route("GET /api/v1/hosts/{name}/logs", () =>
    failure(409, "conflict", "rejected by agent: the agent is too old to send logs; update it"),
  );
  renderAt("/logs?source=nas", "/logs", <LogsPage />);
  expect(await screen.findByText(/the agent is too old to send logs/)).toBeVisible();
  expect(screen.getByText(/update it/)).toBeVisible();
});

test("turning_live_off_stops_the_page_asking_again_on_its_own", async () => {
  vi.useFakeTimers({ now: new Date("2026-11-01T12:00:00Z"), shouldAdvanceTime: true });
  const user = userEvent.setup({ advanceTimers: (ms) => vi.advanceTimersByTime(ms) });
  renderAt("/logs", "/logs", <LogsPage />);
  await screen.findByText(/hub started hosts=1/);
  expect(screen.getByText("Auto-refresh every 5 s")).toBeVisible();

  await act(() => vi.advanceTimersByTimeAsync(LOGS_REFRESH_MS + 1_000));
  expect(logQueries().length, "live asks again by itself").toBeGreaterThan(1);

  await user.click(screen.getByRole("switch", { name: "Live" }));
  expect(screen.getByTestId("location")).toHaveTextContent("refresh=off");
  expect(screen.queryByText(/Auto-refresh every/)).toBeNull();
  const asked = logQueries().length;
  await act(() => vi.advanceTimersByTimeAsync(LOGS_REFRESH_MS * 3));
  expect(logQueries(), "off leaves the asking to the button").toHaveLength(asked);

  await user.click(screen.getByRole("button", { name: "Refresh" }));
  expect(logQueries()).toHaveLength(asked + 1);

  await user.click(screen.getByRole("switch", { name: "Live" }));
  await act(() => vi.advanceTimersByTimeAsync(LOGS_REFRESH_MS + 1_000));
  expect(logQueries().length, "on asks again").toBeGreaterThan(asked + 1);
});

/** Hides or shows the browser tab the way the browser does, and tells the page. */
function setVisibility(state: "visible" | "hidden") {
  Object.defineProperty(document, "visibilityState", { configurable: true, value: state });
  document.dispatchEvent(new Event("visibilitychange"));
}

test("live_logs_stop_refreshing_while_the_tab_is_hidden", async () => {
  vi.useFakeTimers({ now: new Date("2026-11-01T12:00:00Z"), shouldAdvanceTime: true });
  try {
    renderAt("/logs", "/logs", <LogsPage />);
    await screen.findByText(/hub started hosts=1/);

    await act(() => vi.advanceTimersByTimeAsync(LOGS_REFRESH_MS * 2 + 1_000));
    const visible = logQueries().length;
    expect(visible, "live asks again while the tab is visible").toBeGreaterThan(1);

    act(() => setVisibility("hidden"));
    await act(() => vi.advanceTimersByTimeAsync(LOGS_REFRESH_MS * 4));
    expect(logQueries(), "a hidden tab asks for nothing").toHaveLength(visible);

    act(() => setVisibility("visible"));
    await act(() => vi.advanceTimersByTimeAsync(LOGS_REFRESH_MS * 2 + 1_000));
    expect(logQueries().length, "showing the tab resumes the asking").toBeGreaterThan(visible);
  } finally {
    setVisibility("visible");
  }
});

test("logs_start_on_the_selected_host_until_a_source_is_picked", async () => {
  route("GET /api/v1/hosts/{name}/logs", () => [
    { at: T0, level: "warn", target: "agent.checks", message: "disk.usage on /srv is 97% full" },
  ]);
  const user = userEvent.setup();
  renderAt("/logs?host=nas", "/logs", <LogsPage />);

  expect(await screen.findByText(/disk\.usage on \/srv is 97% full/)).toBeVisible();
  expect(screen.getByRole("combobox", { name: "Source" })).toHaveTextContent("nas");
  expect(logQueries(), "the hub's own lines were not asked for").toEqual([]);

  await user.click(screen.getByRole("combobox", { name: "Source" }));
  await user.click(await screen.findByRole("option", { name: "Hub" }));
  expect(await screen.findByText(/hub started hosts=1/)).toBeVisible();
});
