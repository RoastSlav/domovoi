import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { JOB_POLL_MS } from "../../api/hooks/packageJobs";
import { INSTALL_JOB_STATUSES, INSTALL_TARGET_STATUSES } from "../../design/packageVocab";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { nas } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { renderWithProfile } from "../../test/visibility";
import { InstallJobPage } from "./InstallJobPage";
import { HOST_HASH, job, plan, target } from "./jobFixtures";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const PATH = "/capabilities/jobs/:id";
const URL = "/capabilities/jobs/job-1";
const reads = () => calls.filter((c) => c.route === "GET /api/v1/packages/jobs/{id}").length;
const SOON = () => new Date(Date.now() + 125_000).toISOString();

function serve(answer: () => unknown) {
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/packages/jobs/{id}", answer);
}

function show(url = URL) {
  return renderAt(url, PATH, <InstallJobPage />);
}

beforeEach(() => {
  resetApi();
  serve(() => job());
});

afterEach(() => {
  vi.useRealTimers();
});

test("job_page_polls_while_running_and_stops_when_finished", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  show();
  await screen.findByText("Running on nas. Waiting for the result…");
  const first = reads();
  await act(() => vi.advanceTimersByTimeAsync(JOB_POLL_MS));
  expect(reads()).toBe(first + 1);

  serve(() =>
    job([target({ status: "succeeded", finished_at: "2026-10-31T12:01:00Z" })], {
      status: "succeeded",
    }),
  );
  await act(() => vi.advanceTimersByTimeAsync(JOB_POLL_MS));
  await waitFor(() => expect(screen.queryByText(/Waiting for the result/)).toBeNull());
  const done = reads();
  await act(() => vi.advanceTimersByTimeAsync(JOB_POLL_MS * 4));
  expect(reads()).toBe(done);
});

test("job_page_does_not_poll_an_unknown_status", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  serve(() => job([target({ status: "from_the_future" })], { status: "from_the_future" }));
  show();
  await screen.findAllByText("from_the_future");
  const first = reads();
  await act(() => vi.advanceTimersByTimeAsync(JOB_POLL_MS * 3));
  expect(reads()).toBe(first);
});

test.each(Object.entries(INSTALL_JOB_STATUSES))(
  "job_statuses_use_the_vocabularies: job %s",
  async (status, meta) => {
    serve(() => job([target({ status: "succeeded" })], { status }));
    show();
    expect((await screen.findAllByText(meta.label)).length).toBeGreaterThan(0);
  },
);

test("job_statuses_use_the_vocabularies_for_every_target_status", async () => {
  const all = Object.keys(INSTALL_TARGET_STATUSES).map((status, i) =>
    target({ id: `t${i}`, host_id: `h${i}`, host_name: `host-${i}`, status, expires_at: SOON() }),
  );
  serve(() => job(all, { status: "partial" }));
  show();
  for (const meta of Object.values(INSTALL_TARGET_STATUSES)) {
    expect((await screen.findAllByText(meta.label)).length).toBeGreaterThan(0);
  }
});

test("job_targets_show_the_latest_attempt_and_fold_earlier_ones", async () => {
  const user = userEvent.setup();
  serve(() =>
    job([
      target({
        id: "a1",
        attempt: 1,
        status: "failed",
        failure: { code: "transfer_failed", message: "" },
      }),
      target({ id: "a2", attempt: 2, status: "dispatched" }),
    ]),
  );
  renderWithProfile("standard", URL, PATH, <InstallJobPage />);
  expect(await screen.findByText("Running on nas. Waiting for the result…")).toBeVisible();
  expect(
    screen.getAllByRole("listitem").filter((li) => li.textContent?.includes("nas")),
  ).not.toHaveLength(0);
  expect(screen.queryByText(/Attempt 1/)).toBeNull();
  await user.click(screen.getByRole("button", { name: /Earlier attempts/ }));
  expect(screen.getByText(/Attempt 1/)).toBeVisible();
});

test("queued_target_says_it_is_waiting_for_the_host_and_when_it_is_dropped", async () => {
  serve(() => job([target({ status: "queued", expires_at: SOON() })]));
  show();
  expect(await screen.findByText(/Waiting for nas\. It is dropped in/)).toHaveTextContent(
    /dropped in \d+:\d\d if the host is not ready/,
  );
});

test("failed_target_shows_a_sentence_never_a_raw_code", async () => {
  serve(() =>
    job(
      [
        target({
          status: "failed",
          failure: { code: "bad_signature", message: "bad_signature: key abc failed" },
        }),
      ],
      {
        status: "failed",
      },
    ),
  );
  show();
  expect(await screen.findByText("The host could not verify the package signature.")).toBeVisible();
  expect(screen.getByText("key abc failed")).toBeVisible();
  expect(document.body.textContent).not.toContain("bad_signature");
});

test("target_failure_text_is_plain_and_has_no_raw_code", async () => {
  serve(() =>
    job([
      target({
        status: "failed",
        failure: { code: "agent_refused", message: "<b>bold</b> **x**" },
      }),
    ]),
  );
  show();
  expect(await screen.findByText("<b>bold</b> **x**")).toBeVisible();
  expect(document.querySelector("b")).toBeNull();
  expect(document.querySelector("strong")).toBeNull();
});

test("outcome_unknown_says_check_the_host_and_offers_retry_only_as_a_new_approval", async () => {
  const user = userEvent.setup();
  serve(() =>
    job(
      [target({ status: "outcome_unknown", failure: { code: "result_mismatch", message: "" } })],
      {
        status: "partial",
      },
    ),
  );
  route("POST /api/v1/hosts/{name}/packages/refresh", () => ({
    host_id: nas.id,
    entries: [{ name: "nginx-tools", version: "1.2.0" }],
  }));
  show();
  expect(await screen.findByText("Result unknown")).toBeVisible();
  expect(screen.getByText(/Check the host before trying again/)).toBeVisible();
  expect(screen.getByRole("button", { name: "Retry" })).toBeVisible();
  expect(document.body.textContent?.toLowerCase()).not.toContain("failed");
  await user.click(screen.getByRole("button", { name: "Check host" }));
  expect(await screen.findByText("nas reports nginx-tools 1.2.0 installed")).toBeVisible();
  expect(calls.some((c) => c.route === "POST /api/v1/hosts/{name}/packages/refresh")).toBe(true);
});

test("check_host_says_when_the_host_does_not_list_the_package", async () => {
  const user = userEvent.setup();
  serve(() => job([target({ status: "outcome_unknown" })]));
  route("POST /api/v1/hosts/{name}/packages/refresh", () => ({ host_id: nas.id, entries: [] }));
  show();
  await user.click(await screen.findByRole("button", { name: "Check host" }));
  expect(await screen.findByText("nas does not list nginx-tools 1.2.0")).toBeVisible();
});

test("succeeded_without_confirmation_says_to_check_the_host", async () => {
  serve(() =>
    job([target({ status: "succeeded", outputs_confirmed: false })], { status: "succeeded" }),
  );
  show();
  expect(
    await screen.findByText(
      "Confirmed by the host's record only. Check the host to see what is installed.",
    ),
  ).toBeVisible();
  expect(screen.getByRole("button", { name: "Check host" })).toBeVisible();
});

test("retry_asks_for_a_fresh_approval", async () => {
  const user = userEvent.setup();
  serve(() =>
    job([target({ status: "failed", failure: { code: "transfer_failed", message: "" } })], {
      status: "failed",
    }),
  );
  route("POST /api/v1/packages/jobs/plan", () => plan());
  route("POST /api/v1/packages/jobs/{id}/targets/{host_id}/retry", () => job());
  show();
  await user.click(await screen.findByRole("button", { name: "Retry" }));
  const dialog = await screen.findByRole("dialog");
  const planned = await within(dialog).findByText("Plan 0f0e0d0c");
  expect(planned).toBeVisible();
  expect(calls.find((c) => c.route === "POST /api/v1/packages/jobs/plan")?.init.body).toEqual({
    kind: "install",
    name: "nginx-tools",
    version: "1.2.0",
    host_ids: [nas.id],
  });
  const retry = within(dialog).getByRole("button", { name: "Retry" });
  expect(retry).toHaveAccessibleDescription("Enter your password first.");
  await user.type(within(dialog).getByLabelText("Your password"), "hunter2");
  await user.type(within(dialog).getByLabelText(/Type .* to confirm/), "nginx-tools");
  const before = reads();
  await user.click(retry);
  await waitFor(() =>
    expect(calls.find((c) => c.route.endsWith("/retry"))?.init.body).toEqual({
      plan_hash: HOST_HASH,
      password: "hunter2",
      confirm_text: "nginx-tools",
    }),
  );
  expect(await screen.findByText("Started again on nas")).toBeVisible();
  await waitFor(() => expect(reads()).toBeGreaterThan(before));
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("cancel_is_offered_only_for_queued_targets_and_needs_no_password", async () => {
  const user = userEvent.setup();
  serve(() => job([target({ status: "queued", expires_at: SOON() })]));
  route("POST /api/v1/packages/jobs/{id}/targets/{host_id}/cancel", () =>
    job([target({ status: "cancelled" })]),
  );
  show();
  await user.click(await screen.findByRole("button", { name: "Cancel" }));
  expect(await screen.findByText("Cancelled the install on nas")).toBeVisible();
  const sent = calls.find((c) => c.route.endsWith("/cancel"));
  expect(sent?.init.body).toBeUndefined();
});

test("cancel_conflict_reads_inline_in_the_row", async () => {
  const user = userEvent.setup();
  serve(() => job([target({ status: "queued", expires_at: SOON() })]));
  route("POST /api/v1/packages/jobs/{id}/targets/{host_id}/cancel", () =>
    failure(409, "conflict", "it already started"),
  );
  show();
  await user.click(await screen.findByRole("button", { name: "Cancel" }));
  expect(await screen.findByText("Could not cancel the install")).toBeVisible();
});

test.each(["dispatched", "transferring", "succeeded", "failed", "not_run", "outcome_unknown"])(
  "cancel_is_not_offered_when_the_host_is_%s",
  async (status) => {
    serve(() => job([target({ status })]));
    show();
    await screen.findByText("nas");
    expect(screen.queryByRole("button", { name: "Cancel" })).toBeNull();
  },
);

test("job_page_404_says_no_such_install_with_a_link_back", async () => {
  serve(() => failure(404, "no_such_job", "gone"));
  show();
  expect(await screen.findByText("No such install")).toBeVisible();
  expect(screen.getAllByRole("link", { name: "Back to packages" })[0]).toHaveAttribute(
    "href",
    "/capabilities?tab=packages",
  );
});

test("unknown_values_render_neutral_and_never_as_success", async () => {
  serve(() => job([target({ status: "from_the_future" })], { status: "also_new" }));
  show();
  expect(await screen.findAllByText("from_the_future")).not.toHaveLength(0);
  expect(screen.getByText("also_new")).toBeVisible();
  expect(screen.queryByText("Succeeded")).toBeNull();
  expect(screen.queryByRole("button", { name: /Retry|Cancel|Check host/ })).toBeNull();
});

test("copy_never_claims_enabled_or_available_early_on_the_job_page", async () => {
  serve(() =>
    job([target({ status: "succeeded", outputs_confirmed: false })], { status: "succeeded" }),
  );
  show();
  await screen.findByText(/Confirmed by the host/);
  const text = (document.body.textContent ?? "").toLowerCase();
  for (const phrase of [
    "ready to use",
    "now running",
    "runs automatically",
    "automatic",
    "auto-install",
    "update all",
    "install on all",
    "trusted by your hosts",
  ]) {
    expect(text).not.toContain(phrase);
  }
});
