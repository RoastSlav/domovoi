import { screen, within } from "@testing-library/react";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { nas } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { CapabilitiesPage } from "./CapabilitiesPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const CAPABILITIES = "GET /api/v1/hosts/{name}/capabilities";

/** One inventory row, as the hub sends it. */
function row(contract: string, state: string) {
  return {
    contract,
    version: 1,
    title: contract,
    category: "container.observe",
    effect: "read",
    target_kind: "container",
    execution: "agent",
    state,
    reason: state === "available" ? null : "ceiling_absent",
    describe: state === "available" ? "available on this host" : "this host's agent is offline",
    catalogue: { state: "complete", gap: null, hash: "abcd" },
    root_agent: true,
    runner_installed: false,
    from_hello_skills: false,
  };
}

beforeEach(() => {
  resetApi();
  route("GET /api/v1/hosts", () => [nas]);
});

test("shows a loading state while the inventory is pending", async () => {
  route(CAPABILITIES, () => new Promise(() => {}));
  renderAt("/capabilities?host=nas", "/capabilities", <CapabilitiesPage />);
  expect(await screen.findAllByRole("status")).not.toHaveLength(0);
});

test("shows an empty state with a real sentence when the host has no rows", async () => {
  route(CAPABILITIES, () => ({ host: "nas", entries: [], unknown_contracts: [] }));
  renderAt("/capabilities?host=nas", "/capabilities", <CapabilitiesPage />);
  expect(await screen.findByText(/No capabilities/)).toBeInTheDocument();
  expect(screen.getByText(/enrolled and its agent is connected/)).toBeInTheDocument();
});

test("shows the error text and retries", async () => {
  let attempts = 0;
  route(CAPABILITIES, () => {
    attempts += 1;
    return attempts === 1
      ? failure(500, "internal", "The hub could not answer.")
      : { host: "nas", entries: [row("container.logs", "available")], unknown_contracts: [] };
  });
  renderAt("/capabilities?host=nas", "/capabilities", <CapabilitiesPage />);
  expect(await screen.findByText(/Could not load capabilities/)).toBeInTheDocument();
  screen.getByRole("button", { name: /retry/i }).click();
  expect(await screen.findByText("container.logs")).toBeInTheDocument();
});

test("an offline host says so instead of looking empty", async () => {
  route(CAPABILITIES, () => ({
    host: "nas",
    entries: [row("container.logs", "host_absent"), row("container.inspect", "not_installed")],
    unknown_contracts: [],
  }));
  renderAt("/capabilities?host=nas", "/capabilities", <CapabilitiesPage />);
  const table = await screen.findByRole("table", { name: /Capabilities on this host/ });
  expect(within(table).getByText("Host offline")).toBeInTheDocument();
  expect(within(table).getByText("Not installed")).toBeInTheDocument();
  expect(screen.queryByText(/No capabilities/)).not.toBeInTheDocument();
});

test("a partial catalogue is reported as unverified", async () => {
  route(CAPABILITIES, () => ({
    host: "nas",
    entries: [
      {
        ...row("container.logs", "unavailable"),
        catalogue: { state: "partial", gap: "page_failed", hash: null },
      },
    ],
    unknown_contracts: [],
  }));
  renderAt("/capabilities?host=nas", "/capabilities", <CapabilitiesPage />);
  expect(await screen.findByText(/has not been verified/)).toBeInTheDocument();
  expect(screen.queryByText("Available")).not.toBeInTheDocument();
});

test("an unknown availability value renders through the fallback", async () => {
  route(CAPABILITIES, () => ({
    host: "nas",
    entries: [row("container.logs", "quarantined")],
    unknown_contracts: [],
  }));
  renderAt("/capabilities?host=nas", "/capabilities", <CapabilitiesPage />);
  const table = await screen.findByRole("table", { name: /Capabilities on this host/ });
  expect(within(table).getByText("quarantined")).toBeInTheDocument();
  const pill = within(table).getByText("quarantined");
  expect(pill.getAttribute("aria-description")).toMatch(/not known to this version of the UI/);
});

test("unknown contracts are listed as diagnostics only", async () => {
  route(CAPABILITIES, () => ({
    host: "nas",
    entries: [row("container.logs", "available")],
    unknown_contracts: ["container.destroy"],
  }));
  renderAt("/capabilities?host=nas", "/capabilities", <CapabilitiesPage />);
  expect(await screen.findByText("container.destroy")).toBeInTheDocument();
  expect(calls.some((call) => call.route === CAPABILITIES)).toBe(true);
});
