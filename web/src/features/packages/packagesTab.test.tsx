import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { nas, T0 } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { PackagesTab } from "./PackagesTab";
import { hostPackage, job, pkg, status } from "./packagesFixtures";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const ARCHIVE = "Package archive";
const SIGNATURE = "Signature file";

function stub(options: { enabled?: boolean; owner?: boolean } = {}) {
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/packages/status", () =>
    status({
      enabled: options.enabled ?? true,
      owner_keys: options.owner === false ? [] : status().owner_keys,
    }),
  );
  route("GET /api/v1/packages", () => [pkg()]);
  route("GET /api/v1/packages/jobs", () => ({ items: [], next_cursor: null }));
}

function show(url = "/capabilities?tab=packages") {
  renderAt(url, "/capabilities", <PackagesTab />);
}

const archiveFile = new File(["zst"], "nginx-tools-1.2.0.tar.zst");
const sigFile = new File(["sig"], "nginx-tools-1.2.0.sig.toml");

/** Waits for the status to load, which lets the drop zones accept files. */
async function ready() {
  await screen.findByText("Revoked keys");
  return screen.getByLabelText(ARCHIVE);
}

async function chooseBoth(user: ReturnType<typeof userEvent.setup>) {
  await user.upload(await ready(), archiveFile);
  await user.upload(screen.getByLabelText(SIGNATURE), sigFile);
}

beforeEach(() => {
  resetApi();
});

test("packages_tab_explains_when_installation_is_off", async () => {
  stub({ enabled: false });
  show();

  expect(await screen.findByText("Package installation is off")).toBeVisible();
  expect(screen.getByRole("link", { name: "Open Packages settings" })).toHaveAttribute(
    "href",
    "/settings/packages",
  );
  const importButton = screen.getByRole("button", { name: "Import package" });
  expect(importButton).toHaveAttribute("aria-disabled", "true");
  expect(importButton).toHaveAccessibleDescription(/off on this hub/);
  const install = await screen.findByRole("button", { name: "Install…" });
  expect(install).toHaveAttribute("aria-disabled", "true");
  expect(await screen.findByText("nginx-tools")).toBeVisible();
});

test("packages_tab_warns_when_no_owner_key_is_set", async () => {
  stub({ owner: false });
  show();
  expect(await screen.findByText("No owner key is set")).toBeVisible();
});

test("owner_keys_are_read_only_text", async () => {
  stub();
  show();
  const keys = await screen.findByRole("list", { name: "Owner keys" });
  expect(keys).toHaveTextContent("Release key: 0123456789abcdef");
  expect(screen.getByRole("list", { name: "Revoked keys" })).toHaveTextContent("fedcba9876543210");
  expect(screen.getByText(/the hub cannot change them/)).toBeVisible();
  expect(screen.queryByRole("textbox")).toBeNull();
  expect(screen.queryByRole("button", { name: /key/i })).toBeNull();
});

test("import_sends_archive_and_signature_as_multipart", async () => {
  const user = userEvent.setup({ applyAccept: false });
  stub();
  route("POST /api/v1/packages/import", () => pkg());
  show();
  await chooseBoth(user);
  await user.click(screen.getByRole("button", { name: "Import package" }));

  await screen.findByText("Imported nginx-tools 1.2.0");
  const sent = calls.find((c) => c.route === "POST /api/v1/packages/import");
  const form = sent?.init.bodySerializer?.(sent.init.body) as FormData;
  expect([...form.keys()].sort()).toEqual(["archive", "signature"]);
  expect((form.get("archive") as File).name).toBe("nginx-tools-1.2.0.tar.zst");
  expect((form.get("signature") as File).name).toBe("nginx-tools-1.2.0.sig.toml");
  expect(screen.queryByLabelText(/password/i)).toBeNull();
});

test("import_needs_both_files", async () => {
  const user = userEvent.setup({ applyAccept: false });
  stub();
  show();
  await user.upload(await ready(), archiveFile);
  const button = screen.getByRole("button", { name: "Import package" });
  expect(button).toHaveAttribute("aria-disabled", "true");
  expect(button).toHaveAccessibleDescription("Choose the archive and its signature file.");
});

test("import_shows_the_signature_result_and_how_to_continue", async () => {
  const user = userEvent.setup({ applyAccept: false });
  stub();
  route("POST /api/v1/packages/import", () => pkg());
  show();
  await chooseBoth(user);
  await user.click(screen.getByRole("button", { name: "Import package" }));

  const result = (await screen.findByText("Imported nginx-tools 1.2.0")).closest("[role]");
  expect(screen.getAllByText("Release key").length).toBeGreaterThan(0);
  expect(screen.getAllByText("0123456789abcdef").length).toBeGreaterThan(0);
  expect(screen.getByText("aaaaaaaaaaaa")).toBeVisible();
  expect(screen.getByText(/Each host checks it again/)).toBeVisible();
  expect(screen.getByRole("link", { name: "Install it" })).toHaveAttribute(
    "href",
    "/capabilities/install?package=nginx-tools&version=1.2.0",
  );
  expect(result).not.toBeNull();
  expect(screen.queryByText("nginx-tools-1.2.0.tar.zst")).toBeNull();
});

test("import_reports_an_existing_package_without_alarm", async () => {
  const user = userEvent.setup({ applyAccept: false });
  stub();
  route("POST /api/v1/packages/import", () => pkg({ already_imported: true }));
  show();
  await chooseBoth(user);
  await user.click(screen.getByRole("button", { name: "Import package" }));
  expect(await screen.findByText("nginx-tools 1.2.0 is already in the store")).toBeVisible();
});

test.each([
  ["unknown_key", 422, "Hub text"],
  ["revoked_key", 422, "Hub text"],
  ["bad_signature", 422, "does not match the archive"],
  ["digest_mismatch", 422, "digest in its signature file"],
  ["signature_file_invalid", 422, "not in the expected format"],
  ["not_zstd", 422, "not a .tar.zst file"],
  ["too_large", 413, "larger than this hub accepts"],
  ["store_full", 409, "store is full"],
  ["package_conflict", 409, "Hub text"],
  ["packages_disabled", 409, "turned off on this hub"],
])("import_failures_read_in_words: %s", async (code, status_, words) => {
  const user = userEvent.setup({ applyAccept: false });
  stub();
  route("POST /api/v1/packages/import", () => failure(status_, code, "hub text"));
  show();
  await chooseBoth(user);
  await user.click(screen.getByRole("button", { name: "Import package" }));

  const alert = await screen.findByRole("alert");
  expect(alert).toHaveTextContent(words);
  expect(alert).not.toHaveTextContent(code);
  expect(screen.getByText("nginx-tools-1.2.0.tar.zst")).toBeVisible();
});

test("import_warns_when_the_two_files_do_not_match_by_name", async () => {
  const user = userEvent.setup({ applyAccept: false });
  stub();
  show();
  await user.upload(await ready(), archiveFile);
  await user.upload(
    screen.getByLabelText(SIGNATURE),
    new File(["sig"], "other-pkg-0.1.0.sig.toml"),
  );
  expect(screen.getByText("These two files may not belong together.")).toBeVisible();
  expect(screen.getByRole("button", { name: "Import package" })).not.toHaveAttribute(
    "aria-disabled",
    "true",
  );
});

test("store_list_has_loading_empty_error_states", async () => {
  stub();
  route("GET /api/v1/packages", () => [] as unknown[]);
  show();
  expect(await screen.findByText("No packages yet")).toBeVisible();
  expect(screen.getByText("Import an owner-signed package above.")).toBeVisible();
});

test("store_list_failure_is_a_load_error_not_an_empty_state", async () => {
  stub();
  route("GET /api/v1/packages", () => failure(500, "internal", "boom"));
  show();
  expect(await screen.findByText(/Could not load the package store/)).toBeVisible();
  expect(screen.queryByText("No packages yet")).toBeNull();
});

test("missing_package_file_is_flagged_in_the_details", async () => {
  const user = userEvent.setup();
  stub();
  route("GET /api/v1/packages/{name}/{version}", () => ({
    ...pkg(),
    signature_toml: 'key_id = "x"',
    hosts: [],
    file_status: "missing",
  }));
  show();
  await user.click(await screen.findByRole("button", { name: "Details" }));
  const dialog = await screen.findByRole("dialog", { name: "nginx-tools 1.2.0" });
  expect(await within(dialog).findByText("File missing")).toBeVisible();
});

test("details_dialog_lists_digest_signature_and_hosts", async () => {
  const user = userEvent.setup();
  stub();
  route("GET /api/v1/packages/{name}/{version}", () => ({
    ...pkg(),
    signature_toml: 'key_id = "sigtext"',
    hosts: [{ host_id: "host-nas", host_name: "nas", status: "active", observed_at: T0 }],
    file_status: "ready",
  }));
  show();
  await user.click(await screen.findByRole("button", { name: "Details" }));
  const dialog = await screen.findByRole("dialog", { name: "nginx-tools 1.2.0" });
  expect(await within(dialog).findByText("a".repeat(64))).toBeVisible();
  expect(within(dialog).getByText('key_id = "sigtext"')).toBeVisible();
  expect(within(dialog).getByText("nas")).toBeVisible();
  expect(within(dialog).getByText("Installed")).toBeVisible();
  expect(within(dialog).getByText(/as last reported/)).toBeVisible();
  expect(dialog).not.toHaveTextContent(/\/var\/|\/etc\/|\.part|data_dir/);
});

test("delete_package_is_confirmed_and_says_installed_copies_stay", async () => {
  const user = userEvent.setup();
  stub();
  route("GET /api/v1/packages/{name}/{version}", () => ({
    ...pkg(),
    signature_toml: "",
    hosts: [],
    file_status: "ready",
  }));
  let deletes = 0;
  route("DELETE /api/v1/packages/{name}/{version}", () => {
    deletes += 1;
    return failure(409, "package_in_use", "in use");
  });
  show();
  await user.click(await screen.findByRole("button", { name: "Details" }));
  await user.click(await screen.findByRole("button", { name: "Delete from the store" }));
  expect(deletes).toBe(0);
  const confirm = screen.getByRole("dialog", { name: "Delete nginx-tools 1.2.0 from the store?" });
  expect(within(confirm).getByText(/Hosts that already have it keep it/)).toBeVisible();
  await user.click(within(confirm).getByRole("button", { name: "Delete" }));
  expect(await within(confirm).findByRole("alert")).toHaveTextContent(
    /running install still needs/,
  );
  expect(deletes).toBe(1);
});

test("installed_list_is_display_only_and_says_when_it_was_last_checked", async () => {
  stub();
  route("GET /api/v1/hosts/{name}/packages", () => ({
    host_id: "host-nas",
    observed_at: T0,
    entries: [hostPackage()],
    install_allowed: true,
  }));
  show("/capabilities?tab=packages&host=nas");
  expect(await screen.findByText("Installed")).toBeVisible();
  expect(screen.getByText("Isolated")).toBeVisible();
  expect(screen.getByText(/Last checked/)).toBeVisible();
  expect(screen.getByText("Installed does not mean enabled.")).toBeVisible();
  expect(screen.getByRole("link", { name: "Remove…" })).toHaveAttribute(
    "href",
    "/capabilities/install?kind=uninstall&package=nginx-tools&version=1.2.0&host=nas",
  );
});

test("installed_list_asks_for_a_host_under_all_hosts", async () => {
  stub();
  show();
  expect(await screen.findByText("Choose a host to see what is installed on it.")).toBeVisible();
  expect(calls.some((c) => c.route === "GET /api/v1/hosts/{name}/packages")).toBe(false);
});

test("check_host_refreshes_and_reports_in_words", async () => {
  const user = userEvent.setup();
  stub();
  route("GET /api/v1/hosts/{name}/packages", () => ({
    host_id: "host-nas",
    observed_at: null,
    entries: [],
  }));
  route("POST /api/v1/hosts/{name}/packages/refresh", () => ({
    host_id: "host-nas",
    observed_at: T0,
    entries: [hostPackage()],
  }));
  show("/capabilities?tab=packages&host=nas");
  expect(await screen.findByText("Not checked yet")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Check host" }));
  expect(await screen.findByText("Installed")).toBeVisible();
  expect(screen.getByText(/Last checked/)).toBeVisible();
});

test("check_host_failure_reads_through_describe_error", async () => {
  const user = userEvent.setup();
  stub();
  route("GET /api/v1/hosts/{name}/packages", () => ({
    host_id: "host-nas",
    observed_at: T0,
    entries: [],
  }));
  route("POST /api/v1/hosts/{name}/packages/refresh", () =>
    failure(503, "unavailable", "agent not connected"),
  );
  show("/capabilities?tab=packages&host=nas");
  await user.click(await screen.findByRole("button", { name: "Check host" }));
  expect(await screen.findByText("Could not check the host")).toBeVisible();
});

test("installed_list_says_when_the_agent_has_not_turned_installation_on", async () => {
  stub();
  route("GET /api/v1/hosts/{name}/packages", () => ({
    host_id: "host-nas",
    observed_at: T0,
    entries: [hostPackage()],
    install_allowed: false,
  }));
  show("/capabilities?tab=packages&host=nas");
  expect(
    await screen.findByText(/nas's agent has not turned on package installation/),
  ).toBeVisible();
});

test("recent_installs_link_to_the_job_page", async () => {
  stub();
  route("GET /api/v1/packages/jobs", () => ({ items: [job()], next_cursor: null }));
  show();
  const link = await screen.findByRole("link", { name: /nginx-tools/ });
  await waitFor(() => expect(screen.getByText("Running")).toBeVisible());
  const recent = screen.getAllByRole("link").find((a) => a.getAttribute("href")?.includes("jobs"));
  expect(recent).toHaveAttribute("href", "/capabilities/jobs/job-1");
  expect(link).toBeDefined();
});

test("package_screens_show_no_hub_path", async () => {
  stub();
  show();
  await screen.findByText("nginx-tools");
  expect(document.body).not.toHaveTextContent(/\/var\/|\.part|data_dir/);
});
