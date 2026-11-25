import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { nas } from "../../test/fixtures";
import { helperView, unpinnedView, verifiedReport } from "../../test/helperFixtures";
import { renderAt } from "../../test/render";
import { expectP0Everywhere, renderWithProfile } from "../../test/visibility";
import { CapabilitiesPage } from "./CapabilitiesPage";
import { HelperTab } from "./HelperTab";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const HELPER = "GET /api/v1/hosts/{name}/helper";
const RECHECK = "POST /api/v1/hosts/{name}/helper/recheck";
const pi = { ...nas, id: "host-pi", name: "pi" };

beforeEach(() => {
  localStorage.clear();
  resetApi();
  route("GET /api/v1/hosts", () => [nas, pi]);
});

function helpers(byHost: Record<string, unknown>) {
  route(HELPER, (init) => {
    const name = init.params?.path?.name ?? "";
    const answer = byHost[name];
    return answer instanceof Object && "host" in answer ? { ...answer, host: name } : answer;
  });
}

function show() {
  return renderAt("/capabilities?tab=helper", "/capabilities", <HelperTab />);
}

function cardOf(host: string) {
  return screen.getByRole("group", { name: `Helper on ${host}` });
}

test("helper_panel_shows_each_host_state_and_reason", async () => {
  helpers({
    nas: helperView(),
    pi: unpinnedView({
      attestation: {
        state: "unverified",
        reason: "key_not_pinned",
        helper_reason: "state_corrupt",
        unsigned_hint: "uninitialized",
        checked_at: "2026-10-31T11:00:00Z",
      },
    }),
  });
  show();
  const verified = within(await screen.findByRole("group", { name: "Helper on nas" }));
  expect(verified.getByText("Key pinned")).toBeVisible();
  expect(verified.getByText("Verified")).toBeVisible();
  expect(verified.getByText("192.0.2.0/24")).toBeVisible();
  expect(verified.getByText("sshd.service")).toBeVisible();
  expect(verified.getByText("plex")).toBeVisible();
  expect(verified.getByText(/Generation 7/)).toBeVisible();
  expect(verified.getByText(/edit helper.toml on the host/)).toBeVisible();

  const open = within(await screen.findByRole("group", { name: "Helper on pi" }));
  expect(open.getByText("Key not pinned")).toBeVisible();
  expect(open.getByText("Unverified")).toBeVisible();
  expect(open.getByText("You have not pinned the helper's key yet.")).toBeVisible();
  expect(open.getByText(/Without a verified report/)).toBeVisible();
  expect(open.getByText(/state is damaged/)).toBeVisible();
  const commands = open.getAllByLabelText("Command to run on the host as root");
  expect(commands.map((node) => node.textContent)).toEqual([
    "domovoi helper recover",
    "domovoi helper init",
  ]);
  // The agent's hint is shown, labelled as relayed, and does not change the verdict.
  expect(open.getAllByText("Relayed by the agent, not verified.").length).toBeGreaterThan(0);
  expect(open.getByText(/has not been set up/)).toBeVisible();
  expect(open.queryByText("Verified")).toBeNull();
});

test("helper_panel_never_claims_verified_without_a_verified_report", async () => {
  helpers({
    nas: helperView({ verified: null }),
    pi: helperView({
      host: "pi",
      attestation: { state: "stale", reason: "attestation_stale" },
      verified: null,
    }),
  });
  show();
  const lying = within(await screen.findByRole("group", { name: "Helper on nas" }));
  expect(lying.getByText("Unverified")).toBeVisible();
  expect(lying.queryByText("Verified")).toBeNull();
  expect(lying.queryByText("192.0.2.0/24")).toBeNull();
  const stale = within(await screen.findByRole("group", { name: "Helper on pi" }));
  expect(stale.getByText("Attestation stale")).toBeVisible();
  expect(stale.queryByText("Verified")).toBeNull();
  expect(screen.queryByText(/^(Covered|Protected)$/)).toBeNull();
});

test("a_verified_verdict_over_a_stale_report_is_not_verified", async () => {
  helpers({ nas: helperView({ verified: verifiedReport({ fresh: false }) }), pi: unpinnedView() });
  show();
  const card = within(await screen.findByRole("group", { name: "Helper on nas" }));
  expect(card.queryByText("Verified")).toBeNull();
});

test("a_verified_report_without_a_hub_address_says_how_to_cover_it", async () => {
  helpers({ nas: helperView({ verified: verifiedReport({ hub_addrs: [] }) }), pi: unpinnedView() });
  show();
  expect(await screen.findByText(/lists no address for this hub/)).toBeVisible();
});

test("a_failed_helper_load_is_an_error_not_an_empty_card", async () => {
  helpers({ nas: failure(500, "internal", "boom"), pi: unpinnedView() });
  show();
  expect(await screen.findByText(/Could not load the helper on nas/)).toBeVisible();
  expect(await screen.findByRole("group", { name: "Helper on pi" })).toBeVisible();
});

test("the_selected_host_limits_the_panel_to_that_host", async () => {
  helpers({ nas: helperView(), pi: unpinnedView() });
  renderAt("/capabilities?tab=helper&host=pi", "/capabilities", <HelperTab />);
  expect(await screen.findByRole("group", { name: "Helper on pi" })).toBeVisible();
  expect(screen.queryByRole("group", { name: "Helper on nas" })).toBeNull();
});

test("recheck_asks_the_helper_and_reads_the_answer_again", async () => {
  const user = userEvent.setup();
  helpers({ nas: helperView(), pi: unpinnedView() });
  route(RECHECK, () => ({}));
  show();
  await screen.findByRole("group", { name: "Helper on nas" });
  await user.click(within(cardOf("nas")).getByRole("button", { name: "Re-check" }));
  expect(await screen.findByText(/Asked the helper for a fresh report/)).toBeVisible();
  expect(calls.filter((call) => call.route === RECHECK)).toHaveLength(1);
  expect(calls.filter((call) => call.route === HELPER).length).toBeGreaterThan(2);
});

test("recheck_too_soon_reads_in_words_without_a_raw_code", async () => {
  const user = userEvent.setup();
  helpers({ nas: helperView(), pi: unpinnedView() });
  route(RECHECK, () => failure(429, "recheck_too_soon", "asked 3 s ago"));
  show();
  await screen.findByRole("group", { name: "Helper on nas" });
  await user.click(within(cardOf("nas")).getByRole("button", { name: "Re-check" }));
  expect(await screen.findByText(/Wait ten seconds, then re-check/)).toBeVisible();
  expect(document.body.textContent).not.toContain("recheck_too_soon");
});

test("a_host_with_no_helper_key_cannot_be_rechecked_and_says_why", async () => {
  helpers({
    nas: helperView({ key_state: "no_helper", received_key: null, pin: null, verified: null }),
    pi: unpinnedView(),
  });
  show();
  await screen.findByRole("group", { name: "Helper on nas" });
  const button = within(cardOf("nas")).getByRole("button", { name: "Re-check" });
  expect(button).toHaveAttribute("aria-disabled", "true");
});

test("p0 helper_attestation_is_visible_in_every_profile", async () => {
  helpers({ nas: helperView(), pi: unpinnedView() });
  await expectP0Everywhere({
    expectedIds: ["capabilities.helper.attestation"],
    render: async (profile) => {
      const view = renderWithProfile(
        profile,
        "/capabilities?tab=helper",
        "/capabilities",
        <CapabilitiesPage />,
      );
      await screen.findByRole("group", { name: "Helper on pi" });
      return view;
    },
  });
});
