import { screen, within } from "@testing-library/react";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { nas } from "../../test/fixtures";
import { expectP0Everywhere, renderWithProfile } from "../../test/visibility";
import { renderAt } from "../../test/render";
import { CapabilitiesPage } from "./CapabilitiesPage";
import { ceilingNotReported } from "./limitsText";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const CEILING = "GET /api/v1/hosts/{name}/ceiling";
const CAPABILITIES = "GET /api/v1/hosts/{name}/capabilities";

const REPORTED = {
  host: "nas",
  connected: true,
  reporting: "reported",
  unattended_enabled: true,
  hash: "9f86d081884c7d659a2f0000",
  unreadable_reason: null,
  entries: [
    {
      contract: "firewall.block_source",
      targets: { kind: "any" },
      max_per_hour: 20,
      max_active: 50,
      max_ttl_secs: 3600,
      args: { min_prefix_len_v4: 24 },
    },
    {
      contract: "container.restart",
      targets: { kind: "names", names: ["plex", "jellyfin"] },
      max_per_hour: 3,
      max_active: null,
      max_ttl_secs: null,
      args: {},
    },
  ],
};

function silent(reporting: string, extra: object = {}) {
  return {
    host: "nas",
    connected: true,
    reporting,
    unattended_enabled: null,
    hash: null,
    entries: [],
    unreadable_reason: null,
    ...extra,
  };
}

const INVENTORY = {
  host: "nas",
  entries: [
    {
      contract: "container.logs",
      version: 1,
      title: "container.logs",
      category: "container.observe",
      effect: "read",
      target_kind: "container",
      execution: "agent",
      state: "available",
      reason: null,
      describe: "available on this host",
      catalogue: { state: "partial", gap: "page_failed", hash: null },
      root_agent: true,
      runner_installed: false,
      from_hello_skills: false,
    },
  ],
  unknown_contracts: ["container.destroy"],
};

beforeEach(() => {
  resetApi();
  route("GET /api/v1/hosts", () => [nas]);
  route(CAPABILITIES, () => INVENTORY);
});

function limits(answer: unknown, url = "/capabilities?tab=limits&host=nas") {
  route(CEILING, () => answer);
  return renderAt(url, "/capabilities", <CapabilitiesPage />);
}

test("inventory is the default tab", async () => {
  renderAt("/capabilities?host=nas", "/capabilities", <CapabilitiesPage />);
  const tab = await screen.findByRole("tab", { name: "Inventory" });
  expect(tab).toHaveAttribute("aria-selected", "true");
  expect(await screen.findByText("container.logs")).toBeInTheDocument();
});

test("tab choice lives in the URL and an unknown tab falls back to inventory", async () => {
  renderAt("/capabilities?host=nas&tab=bogus", "/capabilities", <CapabilitiesPage />);
  expect(await screen.findByRole("tab", { name: "Inventory", selected: true })).toBeVisible();
  route(CEILING, () => REPORTED);
  screen.getByRole("tab", { name: "Host limits" }).click();
  expect(await screen.findByRole("tab", { name: "Host limits", selected: true })).toBeVisible();
  expect(screen.getByTestId("location").textContent).toContain("tab=limits");
  screen.getByRole("tab", { name: "Inventory" }).click();
  expect(await screen.findByRole("tab", { name: "Inventory", selected: true })).toBeVisible();
  expect(screen.getByTestId("location").textContent).not.toContain("tab=");
});

test("inventory rows carry their visibility categories", async () => {
  renderWithProfile("standard", "/capabilities?host=nas", "/capabilities", <CapabilitiesPage />);
  await screen.findByText("container.logs");
  const state = document.querySelector('[data-visibility-id="capabilities.row.state"]');
  expect(state?.getAttribute("data-visibility")).toBe("P0");
  const technical = document.querySelector('[data-visibility-id="capabilities.row.technical"]');
  expect(technical?.getAttribute("data-presentation")).toBe("collapsed");
});

test("limits tab shows the entries the host reported", async () => {
  limits(REPORTED);
  const table = await screen.findByRole("table", { name: /Unattended limits reported by nas/ });
  expect(within(table).getByText("Any target")).toBeInTheDocument();
  expect(within(table).getByText("jellyfin")).toBeInTheDocument();
  expect(within(table).getByText("1 h")).toBeInTheDocument();
  expect(within(table).getByText("min_prefix_len_v4: 24")).toBeInTheDocument();
  for (const header of ["Per hour", "Active at once", "Longest hold", "Argument bounds"]) {
    expect(within(table).getByText(header)).toBeInTheDocument();
  }
});

test("limits tab is read only", async () => {
  limits(REPORTED);
  await screen.findByRole("table", { name: /Unattended limits/ });
  const panel = screen.getByRole("tabpanel");
  expect(within(panel).queryByRole("textbox")).toBeNull();
  expect(within(panel).queryByRole("checkbox")).toBeNull();
  expect(within(panel).queryByRole("switch")).toBeNull();
  expect(within(panel).queryByRole("button", { name: /save|edit/i })).toBeNull();
  expect(calls.every((call) => call.route.startsWith("GET "))).toBe(true);
});

test("ceiling not reported says so and does not imply permission", async () => {
  limits(silent("not_reported"));
  expect(await screen.findByText(ceilingNotReported("nas"))).toBeInTheDocument();
  expect(screen.queryByText(/everything is allowed|no limits/i)).toBeNull();
});

test("ceiling not reported for an offline host says the host is offline", async () => {
  limits(silent("not_reported", { connected: false }));
  expect(await screen.findByText(/nas is offline, so it has not reported/)).toBeInTheDocument();
});

test("ceiling unreadable is not permissive", async () => {
  limits(silent("unreadable", { unreadable_reason: "bad shape" }));
  expect(await screen.findByText(/could not read what nas reported/)).toBeInTheDocument();
  expect(screen.getByText("bad shape")).toBeInTheDocument();
  expect(
    screen.getByText(/treats unattended actions on this host as not allowed/),
  ).toBeInTheDocument();
  expect(screen.queryByRole("table", { name: /Unattended limits/ })).toBeNull();
});

test("ceiling disabled summary says nothing runs without approval", async () => {
  limits({ ...REPORTED, unattended_enabled: false, entries: [] });
  expect(await screen.findByText(/Nothing runs without your approval/)).toBeInTheDocument();
});

test("ceiling hash is collapsed metadata", async () => {
  route(CEILING, () => REPORTED);
  renderWithProfile(
    "standard",
    "/capabilities?tab=limits&host=nas",
    "/capabilities",
    <CapabilitiesPage />,
  );
  await screen.findByText(/These limits belong to the host/);
  expect(screen.queryByText(/Lets Domovoi notice/)).toBeNull();
  expect(
    await screen.findByRole("button", { name: /Configuration fingerprint/ }),
  ).toBeInTheDocument();
});

test("limits tab asks for a host under all hosts", async () => {
  route(CEILING, () => REPORTED);
  renderAt("/capabilities?tab=limits", "/capabilities", <CapabilitiesPage />);
  expect(await screen.findByText(/Choose a host to see its unattended limits/)).toBeInTheDocument();
  expect(calls.some((call) => call.route === CEILING)).toBe(false);
});

test("limits tab load error is not an empty state", async () => {
  let attempts = 0;
  route(CEILING, () => {
    attempts += 1;
    return attempts === 1 ? failure(500, "internal", "no") : REPORTED;
  });
  renderAt("/capabilities?tab=limits&host=nas", "/capabilities", <CapabilitiesPage />);
  expect(await screen.findByText(/Could not load the host limits/)).toBeInTheDocument();
  screen.getByRole("button", { name: /retry/i }).click();
  expect(await screen.findByRole("table", { name: /Unattended limits/ })).toBeInTheDocument();
});

test("p0 is visible in every profile on the capability tabs", async () => {
  route(CEILING, () => REPORTED);
  await expectP0Everywhere({
    expectedIds: [
      "capabilities.row.state",
      "capabilities.catalogue.note",
      "capabilities.row.authority",
    ],
    render: async (profile) => {
      const view = renderWithProfile(
        profile,
        "/capabilities?host=nas",
        "/capabilities",
        <CapabilitiesPage />,
      );
      await screen.findByText("container.logs");
      return view;
    },
  });
  await expectP0Everywhere({
    expectedIds: ["limits.note", "limits.status"],
    render: async (profile) => {
      const view = renderWithProfile(
        profile,
        "/capabilities?tab=limits&host=nas",
        "/capabilities",
        <CapabilitiesPage />,
      );
      await screen.findByText(/These limits belong to the host/);
      return view;
    },
  });
});
