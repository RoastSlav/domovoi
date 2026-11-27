import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { nas } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { renderWithProfile } from "../../test/visibility";
import { InstallWizardPage } from "./InstallWizardPage";
import { BASE, DETAIL, HOST_HASH, JOB_HASH, STATUS, attic, bare, job, plan } from "./jobFixtures";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const PATH = "/capabilities/install";
const URL = `${PATH}?package=nginx-tools&version=1.2.0&host=nas`;
const PASSWORD = "hunter2-secret";
const created = () => calls.filter((c) => c.route === "POST /api/v1/packages/jobs");

function serve(options: { hosts?: unknown[]; plan?: () => unknown; detail?: unknown } = {}) {
  route("GET /api/v1/hosts", () => options.hosts ?? [nas, attic, bare]);
  route("GET /api/v1/packages/status", () => STATUS);
  route("GET /api/v1/packages", () => [DETAIL]);
  route("GET /api/v1/packages/{name}/{version}", () => options.detail ?? DETAIL);
  route("POST /api/v1/packages/jobs/plan", options.plan ?? (() => plan()));
  route("POST /api/v1/packages/jobs", () => job());
}

beforeEach(() => {
  resetApi();
  serve();
});

type User = ReturnType<typeof userEvent.setup>;

async function toReview(user: User, url = URL) {
  const view = renderAt(url, PATH, <InstallWizardPage />);
  await user.click(await screen.findByRole("button", { name: "Next" }));
  await screen.findByText(/^Plan [0-9a-f]{8}$/);
  return view;
}

async function openDialog(user: User) {
  await user.click(screen.getByRole("button", { name: "Review and confirm" }));
  return screen.findByRole("dialog");
}

async function authorize(user: User, dialog: HTMLElement, name = "nginx-tools") {
  await user.type(within(dialog).getByLabelText("Your password"), PASSWORD);
  await user.type(within(dialog).getByLabelText(/Type .* to confirm/), name);
}

function confirmButton(dialog: HTMLElement, label = "Install") {
  return within(dialog).getByRole("button", { name: label });
}

function reasonOf(button: HTMLElement): string {
  expect(button).toHaveAttribute("aria-disabled", "true");
  return button.getAttribute("aria-describedby")
    ? (button.ownerDocument.getElementById(button.getAttribute("aria-describedby") ?? "")
        ?.textContent ?? "")
    : "";
}

test("wizard_walks_package_hosts_review_confirm_to_the_job_page", async () => {
  const user = userEvent.setup();
  renderAt(URL, PATH, <InstallWizardPage />);
  expect(await screen.findByRole("checkbox", { name: /nas/ })).toBeChecked();
  await user.click(screen.getByRole("button", { name: "Next" }));

  expect(await screen.findByText("Plan a1b2c3d4")).toBeVisible();
  expect(screen.getByText("nas")).toBeVisible();
  expect(screen.getByText(/Its skills stay off until a person enables them/)).toBeVisible();
  const dialog = await openDialog(user);
  expect(within(dialog).getByText(/Install nginx-tools 1.2.0 on 1 host\?/)).toBeVisible();
  await authorize(user, dialog);
  await user.click(confirmButton(dialog));

  await waitFor(() => expect(created()).toHaveLength(1));
  expect(created()[0]?.init.body).toEqual({
    kind: "install",
    name: "nginx-tools",
    version: "1.2.0",
    host_ids: [nas.id],
    plan_hash: JOB_HASH,
    password: PASSWORD,
    confirm_text: "nginx-tools",
  });
  await waitFor(() =>
    expect(screen.getByTestId("location")).toHaveTextContent("/capabilities/jobs/job-1"),
  );
});

test("wizard_never_posts_before_confirmation", async () => {
  const user = userEvent.setup();
  renderAt(PATH, PATH, <InstallWizardPage />);
  await user.selectOptions(await screen.findByLabelText("Package"), "nginx-tools@1.2.0");
  await user.click(screen.getByRole("button", { name: "Next" }));
  await user.click(await screen.findByRole("checkbox", { name: /nas/ }));
  await user.click(screen.getByRole("button", { name: "Next" }));
  await screen.findByText("Plan a1b2c3d4");
  const posts = calls.map((c) => c.route).filter((r) => r.startsWith("POST"));
  expect(posts).toEqual(["POST /api/v1/packages/jobs/plan"]);
});

test("wizard_offers_no_automation", async () => {
  const user = userEvent.setup();
  const banned =
    /automatic|auto-?install|auto-?update|schedule|recurr|every host|all hosts|remember|policy|unattended/i;
  const names = () =>
    screen
      .queryAllByRole("button")
      .concat(screen.queryAllByRole("checkbox"), screen.queryAllByRole("switch"))
      .map((el) => el.textContent ?? "");
  renderAt(PATH, PATH, <InstallWizardPage />);
  await screen.findByLabelText("Package");
  expect(names().filter((n) => banned.test(n))).toEqual([]);
  await user.selectOptions(screen.getByLabelText("Package"), "nginx-tools@1.2.0");
  await user.click(screen.getByRole("button", { name: "Next" }));
  await screen.findByRole("checkbox", { name: /nas/ });
  expect(screen.queryAllByRole("switch")).toHaveLength(0);
  expect(names().filter((n) => banned.test(n))).toEqual([]);
  await user.click(screen.getByRole("checkbox", { name: /nas/ }));
  await user.click(screen.getByRole("button", { name: "Next" }));
  await screen.findByText("Plan a1b2c3d4");
  expect(screen.queryAllByRole("switch")).toHaveLength(0);
  expect(names().filter((n) => banned.test(n))).toEqual([]);
  const dialog = await openDialog(user);
  expect(within(dialog).queryAllByRole("switch")).toHaveLength(0);
  expect(within(dialog).queryAllByRole("checkbox")).toHaveLength(0);
});

test("choose_blocks_a_package_whose_file_is_missing", async () => {
  const user = userEvent.setup();
  serve({ detail: { ...DETAIL, file_status: "missing" } });
  renderAt(URL, PATH, <InstallWizardPage />);
  await user.click(await screen.findByRole("button", { name: "Back" }));
  expect(await screen.findByText("File missing")).toBeVisible();
  const next = screen.getByRole("button", { name: "Next" });
  expect(next).toHaveAccessibleDescription(
    "The package file is missing from the store. Import it again.",
  );
});

test("hosts_step_offers_only_hosts_with_agents_and_says_what_offline_means", async () => {
  const stuck = {
    ...nas,
    id: "host-old",
    name: "old",
    connected: false,
    protocol_state: "incompatible",
  };
  serve({ hosts: [nas, attic, bare, stuck] });
  renderAt(URL, PATH, <InstallWizardPage />);
  const noAgent = await screen.findByRole("checkbox", { name: /bare/ });
  expect(noAgent).toHaveAttribute("aria-disabled", "true");
  expect(screen.getByText("No agent on this host.")).toBeVisible();
  expect(screen.getByText(/waits up to 10 minutes for it/)).toBeVisible();
  expect(screen.getByText("Needs an agent upgrade first. It will be skipped.")).toBeVisible();
});

test("hosts_step_limits_the_count", async () => {
  const user = userEvent.setup();
  const many = Array.from({ length: 17 }, (_, i) => ({ ...nas, id: `h${i}`, name: `host-${i}` }));
  serve({ hosts: many });
  renderAt(`${PATH}?package=nginx-tools&version=1.2.0`, PATH, <InstallWizardPage />);
  await screen.findAllByRole("checkbox");
  expect(screen.getByRole("button", { name: "Next" })).toHaveAccessibleDescription(
    "Choose at least one host.",
  );
  for (const box of screen.getAllByRole("checkbox")) {
    await user.click(box);
  }
  expect(screen.getByText("17 of up to 16")).toBeVisible();
  expect(screen.getByRole("button", { name: "Next" })).toHaveAccessibleDescription(
    "Choose at most 16 hosts.",
  );
});

test("review_shows_availability_installed_version_and_warnings_per_host", async () => {
  const user = userEvent.setup();
  serve({
    plan: () =>
      plan({
        targets: [
          { ...BASE, installed: { ...DETAIL_HOST, version: "1.1.0" } },
          {
            ...BASE,
            host_id: attic.id,
            host_name: "attic",
            availability: "host_incompatible",
            availability_text: "The agent is too old.",
            warnings: ["attic runs an old agent"],
          },
        ],
      }),
  });
  renderAt(URL, PATH, <InstallWizardPage />);
  await user.click(await screen.findByRole("checkbox", { name: /attic/ }));
  await user.click(screen.getByRole("button", { name: "Next" }));
  expect(await screen.findByText("Installed now: 1.1.0 (Stored (skills off))")).toBeVisible();
  expect(screen.getByText("Available")).toBeVisible();
  expect(screen.getByText("Incompatible")).toBeVisible();
  expect(screen.getByText("The agent is too old.")).toBeVisible();
  expect(screen.getByText("attic runs an old agent")).toBeVisible();
});

const DETAIL_HOST = {
  archive_sha256: "ab".repeat(32),
  installed_at: "2026-10-31T12:00:00Z",
  key_id: "0123456789abcdef",
  key_state: "trusted",
  name: "nginx-tools",
  observed_at: "2026-10-31T12:00:00Z",
  observed_by: "agent",
  privilege_boundary: "in_effect",
  skills: [],
  status: "active",
  version: "1.2.0",
};

test("review_blocks_confirm_when_no_host_can_run_it", async () => {
  const user = userEvent.setup();
  serve({ plan: () => plan({ targets: [{ ...BASE, availability: "not_installed" }] }) });
  await toReview(user);
  expect(screen.getByRole("button", { name: "Review and confirm" })).toHaveAccessibleDescription(
    "None of these hosts can run this now. Go back to choose others.",
  );
});

test("review_shows_the_full_fingerprint_only_as_collapsed_detail_in_standard", async () => {
  const user = userEvent.setup();
  renderWithProfile("standard", URL, PATH, <InstallWizardPage />);
  await user.click(await screen.findByRole("button", { name: "Next" }));
  expect(await screen.findByText("Plan a1b2c3d4")).toBeVisible();
  expect(screen.getByRole("button", { name: /Package fingerprint/ })).toBeVisible();
  expect(screen.queryByLabelText("Package digest")).toBeNull();
  expect(screen.getByText(/Its skills stay off until a person enables them/)).toBeVisible();
});

test("authorize_needs_password_and_typed_package_name", async () => {
  const user = userEvent.setup();
  await toReview(user);
  const dialog = await openDialog(user);
  expect(confirmButton(dialog)).toHaveAccessibleDescription("Enter your password first.");
  await user.type(within(dialog).getByLabelText("Your password"), PASSWORD);
  expect(confirmButton(dialog)).toHaveAccessibleDescription("Type nginx-tools to confirm.");
  await user.type(within(dialog).getByLabelText(/Type .* to confirm/), "nginx-tools");
  expect(confirmButton(dialog)).not.toHaveAttribute("aria-disabled");
  await user.click(confirmButton(dialog));
  await waitFor(() => expect(created()).toHaveLength(1));
  expect(created()[0]?.init.body).toMatchObject({
    password: PASSWORD,
    confirm_text: "nginx-tools",
    plan_hash: JOB_HASH,
  });
});

test("authorize_sends_the_hash_captured_when_the_dialog_opened", async () => {
  const user = userEvent.setup();
  await toReview(user);
  const dialog = await openDialog(user);
  // The hub would now answer another plan, but nothing refetched: the UI sends what it showed.
  route("POST /api/v1/packages/jobs/plan", () => plan({ job_plan_hash: "f".repeat(64) }));
  await authorize(user, dialog);
  await user.click(confirmButton(dialog));
  await waitFor(() => expect(created()).toHaveLength(1));
  expect((created()[0]?.init.body as { plan_hash: string }).plan_hash).toBe(JOB_HASH);
});

test("plan_change_under_an_open_dialog_blocks_confirm", async () => {
  const user = userEvent.setup();
  const view = await toReview(user);
  const dialog = await openDialog(user);
  await authorize(user, dialog);
  route("POST /api/v1/packages/jobs/plan", () => plan({ job_plan_hash: "f".repeat(64) }));
  await view.client.refetchQueries({ queryKey: ["packages", "plan"] });
  await waitFor(() =>
    expect(confirmButton(dialog)).toHaveAccessibleDescription(
      "The plan changed while you were looking. Close this and review it again.",
    ),
  );
  await user.click(confirmButton(dialog));
  expect(created()).toHaveLength(0);
});

test("password_is_not_kept_after_submit", async () => {
  const user = userEvent.setup();
  const view = await toReview(user);
  route("POST /api/v1/packages/jobs", () => failure(403, "forbidden", "wrong password"));
  const dialog = await openDialog(user);
  await authorize(user, dialog);
  await user.click(confirmButton(dialog));
  await within(dialog).findByText(/Could not start the install/);
  expect(within(dialog).getByLabelText("Your password")).toHaveValue("");
  await waitFor(() => expect(view.client.getMutationCache().getAll()).toHaveLength(0));
  expect(
    JSON.stringify(
      view.client
        .getMutationCache()
        .getAll()
        .map((m) => m.state),
    ),
  ).not.toContain(PASSWORD);

  route("POST /api/v1/packages/jobs", () => job());
  await authorize(user, dialog);
  await user.click(confirmButton(dialog));
  await waitFor(() => expect(view.client.getMutationCache().getAll()).toHaveLength(0));
  expect(
    JSON.stringify(
      view.client
        .getQueryCache()
        .getAll()
        .map((q) => q.queryKey),
    ),
  ).not.toContain(PASSWORD);
});

const FAILURES: [string, () => unknown, RegExp][] = [
  [
    "a wrong password or name (403)",
    () => failure(403, "forbidden", "password is wrong"),
    /Could not start the install/,
  ],
  [
    "too many attempts (429)",
    () => failure(429, "too_many_requests", "slow down", { "Retry-After": "30" }),
    /Try again in \d+ s/,
  ],
  ["a busy host (409)", () => failure(409, "host_busy", "nas is busy"), /nas is busy/i],
  [
    "packages turned off (409)",
    () => failure(409, "packages_disabled", "off"),
    /turned off on this hub/,
  ],
];

test.each(FAILURES)(
  "confirm_failures_stay_in_the_dialog_in_words: %s",
  async (_name, answer, words) => {
    const user = userEvent.setup();
    await toReview(user);
    route("POST /api/v1/packages/jobs", answer);
    const dialog = await openDialog(user);
    await authorize(user, dialog);
    await user.click(confirmButton(dialog));
    expect(await within(dialog).findByText(words)).toBeVisible();
    expect(screen.getByRole("dialog")).toBe(dialog);
    expect(within(dialog).getByLabelText("Your password")).toHaveValue("");
  },
);

test("confirm_failure_plan_changed_closes_the_dialog_and_reviews_again", async () => {
  const user = userEvent.setup();
  await toReview(user);
  route("POST /api/v1/packages/jobs", () => failure(409, "plan_changed", "changed"));
  const dialog = await openDialog(user);
  await authorize(user, dialog);
  await user.click(confirmButton(dialog));
  expect(await screen.findByText("The plan changed. Review it again.")).toBeVisible();
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(calls.filter((c) => c.route === "POST /api/v1/packages/jobs/plan").length).toBeGreaterThan(
    1,
  );
});

test("uninstall_flow_has_no_artifact_and_says_what_is_removed", async () => {
  const user = userEvent.setup();
  serve({
    plan: () => plan({ kind: "uninstall", archive_sha256: null, key_id: null, signer_label: null }),
  });
  await toReview(user, `${URL}&kind=uninstall`);
  expect(screen.queryByText("Digest")).toBeNull();
  expect(screen.queryByText("Signed by")).toBeNull();
  const dialog = await openDialog(user);
  expect(within(dialog).getByText(/is removed from nas/)).toBeVisible();
  const remove = confirmButton(dialog, "Remove");
  expect(remove.className).toMatch(/danger/);
  await authorize(user, dialog);
  await user.click(remove);
  await waitFor(() => expect(created()).toHaveLength(1));
  expect(created()[0]?.init.body).toMatchObject({ kind: "uninstall", plan_hash: JOB_HASH });
  expect(HOST_HASH).not.toBe(JOB_HASH);
});

test("unknown_values_render_neutral_and_never_as_success_in_the_plan", async () => {
  const user = userEvent.setup();
  serve({ plan: () => plan({ targets: [{ ...BASE, availability: "from_the_future" }] }) });
  await toReview(user);
  expect(screen.getByText("from_the_future")).toBeVisible();
  expect(screen.getByRole("button", { name: "Review and confirm" })).toHaveAttribute(
    "aria-disabled",
    "true",
  );
  void reasonOf;
});

test("copy_never_claims_enabled_or_available_early", async () => {
  const user = userEvent.setup();
  await toReview(user);
  await openDialog(user);
  const text = document.body.textContent ?? "";
  for (const phrase of [
    "ready to use",
    "now running",
    "runs automatically",
    "automatic",
    "auto-install",
    "update all",
    "install on all",
    "everything is allowed",
    "no limits",
    "trusted by your hosts",
  ]) {
    expect(text.toLowerCase(), phrase).not.toContain(phrase);
  }
});
