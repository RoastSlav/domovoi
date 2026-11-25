import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { setUnauthorizedHandler } from "../../api/client";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { nas } from "../../test/fixtures";
import { FINGERPRINT, GROUPS, helperView, unpinnedView } from "../../test/helperFixtures";
import { renderAt } from "../../test/render";
import { expectP0Everywhere, renderWithProfile } from "../../test/visibility";
import { HelperKeyPage } from "./HelperKeyPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const HELPER = "GET /api/v1/hosts/{name}/helper";
const PIN = "PUT /api/v1/hosts/{name}/helper/key";
const UNPIN = "DELETE /api/v1/hosts/{name}/helper/key";

beforeEach(() => {
  localStorage.clear();
  resetApi();
  route("GET /api/v1/hosts", () => [nas]);
});

function show(view: unknown) {
  route(HELPER, () => view);
  return renderAt(
    "/capabilities/helper-key?host=nas",
    "/capabilities/helper-key",
    <HelperKeyPage />,
  );
}

async function fill(user: ReturnType<typeof userEvent.setup>, typed: string, password: string) {
  await user.click(await screen.findByRole("textbox", { name: /Fingerprint printed/ }));
  await user.paste(typed);
  if (password) {
    await user.click(screen.getByLabelText("Your password"));
    await user.paste(password);
  }
}

const pinButton = () => screen.getByRole("button", { name: "Pin this key" });

test("helper_key_page_pin_needs_full_fingerprint_and_password", async () => {
  const user = userEvent.setup();
  show(unpinnedView());
  route(PIN, () => helperView());
  expect(await screen.findByText(GROUPS)).toBeVisible();
  expect(screen.getByText("Unverified")).toBeVisible();
  expect(screen.getByLabelText("Command to run on the host as root")).toHaveTextContent(
    "domovoi helper fingerprint",
  );

  await fill(user, FINGERPRINT.slice(0, 40), "pw");
  expect(pinButton()).toHaveAttribute("aria-disabled", "true");
  await user.click(screen.getByRole("textbox", { name: /Fingerprint printed/ }));
  await user.paste(FINGERPRINT.slice(40));
  await user.clear(screen.getByLabelText("Your password"));
  expect(pinButton()).toHaveAttribute("aria-disabled", "true");
  await user.click(screen.getByLabelText("Your password"));
  await user.paste("pw");
  expect(pinButton()).not.toHaveAttribute("aria-disabled");

  await user.click(pinButton());
  await waitFor(() => expect(calls.filter((call) => call.route === PIN)).toHaveLength(1));
  expect(calls.find((call) => call.route === PIN)?.init.body).toEqual({
    fingerprint: FINGERPRINT,
    password: "pw",
  });
  expect(await screen.findByText(/Pinned the helper key on nas/)).toBeVisible();
  expect(await screen.findByRole("button", { name: "Unpin" })).toBeVisible();
});

test("a_wrong_password_on_pin_stays_in_the_form_and_does_not_log_out", async () => {
  const user = userEvent.setup();
  const unauthorized = vi.fn();
  setUnauthorizedHandler(unauthorized);
  show(unpinnedView());
  route(PIN, () => failure(401, "wrong_password", "wrong password"));
  await fill(user, GROUPS, "nope");
  await user.click(pinButton());
  expect(await screen.findByText("That password is not right.")).toBeVisible();
  expect(pinButton()).toBeVisible();
  expect(unauthorized).not.toHaveBeenCalled();
  expect(document.body.textContent).not.toContain("wrong_password");
});

test("a_fingerprint_that_does_not_match_is_said_in_words", async () => {
  const user = userEvent.setup();
  show(unpinnedView());
  route(PIN, () => failure(422, "fingerprint_mismatch", "no"));
  await fill(user, "f".repeat(64), "pw");
  await user.click(pinButton());
  expect(await screen.findByText(/not the fingerprint of the key the hub holds/)).toBeVisible();
  expect(document.body.textContent).not.toContain("fingerprint_mismatch");
});

test("a_pinned_key_shows_who_pinned_it_and_unpins_with_the_password", async () => {
  const user = userEvent.setup();
  show(helperView());
  route(UNPIN, () => unpinnedView());
  expect(await screen.findByText("admin")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Unpin" }));
  const dialog = await screen.findByRole("dialog", { name: "Remove the pin" });
  await user.type(within(dialog).getByLabelText("Password"), "pw");
  await user.click(within(dialog).getByRole("button", { name: "Unpin" }));
  await waitFor(() => expect(calls.filter((call) => call.route === UNPIN)).toHaveLength(1));
  expect(calls.find((call) => call.route === UNPIN)?.init.body).toEqual({ password: "pw" });
  expect(await screen.findByRole("button", { name: "Pin this key" })).toBeVisible();
});

test("a_host_with_no_helper_key_has_nothing_to_pin", async () => {
  show(helperView({ key_state: "no_helper", received_key: null, pin: null, verified: null }));
  expect(await screen.findByText("No key received yet")).toBeVisible();
  expect(screen.queryByRole("button", { name: /Pin/ })).toBeNull();
});

test("the_key_page_without_a_host_asks_for_one_and_requests_nothing", async () => {
  renderAt("/capabilities/helper-key", "/capabilities/helper-key", <HelperKeyPage />);
  expect(await screen.findByText("Choose a host")).toBeVisible();
  expect(screen.getByRole("link", { name: /Back to the helper tab/ })).toHaveAttribute(
    "href",
    "/capabilities?tab=helper",
  );
  expect(calls.some((call) => call.route === HELPER)).toBe(false);
});

test("helper_key_changed_banner_is_always_visible", async () => {
  route(HELPER, () => helperView({ key_state: "changed", verified: null }));
  await expectP0Everywhere({
    expectedIds: ["capabilities.helper-key.enroll"],
    render: async (profile) => {
      const view = renderWithProfile(
        profile,
        "/capabilities/helper-key?host=nas",
        "/capabilities/helper-key",
        <HelperKeyPage />,
      );
      const banner = await screen.findByRole("alert");
      expect(banner).toHaveTextContent("Helper key changed: re-enrol");
      expect(banner.closest('[data-visibility="P0"]')).not.toBeNull();
      expect(screen.getByRole("button", { name: "Pin the new key" })).toBeVisible();
      return view;
    },
  });
});
