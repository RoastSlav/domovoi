import { act, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { renderAt } from "../../test/render";
import { LoginPage } from "./LoginPage";
import { SetupPage } from "./SetupPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const SESSION = { user: { id: "u1", username: "admin" }, csrf_token: "csrf" };
const PASSPHRASE = "a long passphrase";

beforeEach(resetApi);

async function fillSetup(user: ReturnType<typeof userEvent.setup>, password = PASSPHRASE) {
  await user.type(screen.getByLabelText("Password"), password);
  await user.type(screen.getByLabelText("Repeat password"), password);
}

test("setup_page_shows_the_password_rules_as_a_live_checklist", async () => {
  const user = userEvent.setup();
  renderAt("/setup?token=tok-123", "/setup", <SetupPage />);
  const rules = screen.getByRole("list", { name: "Password rules" });
  expect(within(rules).getByText(/At least 12 characters/)).toHaveTextContent("(not met yet)");
  expect(within(rules).getByText(/The two passwords match/)).toHaveTextContent("(not met yet)");

  await user.type(screen.getByLabelText("Password"), PASSPHRASE);
  expect(within(rules).getByText(/At least 12 characters/)).toHaveTextContent("(met)");
  expect(within(rules).getByText(/The two passwords match/)).toHaveTextContent("(not met yet)");

  await user.type(screen.getByLabelText("Repeat password"), PASSPHRASE);
  expect(within(rules).getByText(/The two passwords match/)).toHaveTextContent("(met)");
});

test("setup_page_blocks_a_short_or_mismatched_password_and_says_why", async () => {
  const user = userEvent.setup();
  route("POST /api/v1/auth/setup", () => SESSION);
  renderAt("/setup?token=tok-123", "/setup", <SetupPage />);
  expect(screen.queryByLabelText("Setup token"), "the link carries the token").toBeNull();
  const create = screen.getByRole("button", { name: "Create account" });

  await fillSetup(user, "short");
  expect(create).toHaveAccessibleDescription("Use at least 12 characters for the password.");
  await user.click(create);
  expect(calls).toHaveLength(0);

  await user.clear(screen.getByLabelText("Password"));
  await user.type(screen.getByLabelText("Password"), PASSPHRASE);
  expect(create).toHaveAccessibleDescription("Type the same password in both fields.");

  await user.clear(screen.getByLabelText("Repeat password"));
  await user.type(screen.getByLabelText("Repeat password"), PASSPHRASE);
  await user.click(create);
  expect(await screen.findByTestId("location")).toHaveTextContent("/welcome");
  expect(calls).toEqual([
    {
      route: "POST /api/v1/auth/setup",
      init: { body: { token: "tok-123", username: "admin", password: PASSPHRASE } },
    },
  ]);
});

test("setup_page_explains_a_spent_link", async () => {
  const user = userEvent.setup();
  route("POST /api/v1/auth/setup", () => failure(403, "forbidden", "bad token"));
  renderAt("/setup", "/setup", <SetupPage />);
  await user.type(screen.getByLabelText("Setup token"), "old");
  await fillSetup(user);
  await user.click(screen.getByRole("button", { name: "Create account" }));
  const alert = await screen.findByRole("alert");
  expect(alert).toHaveTextContent("Could not create the account");
  expect(alert).toHaveTextContent("wrong, used or expired");
});

test("setup_page_explains_a_public_url_mismatch", async () => {
  const user = userEvent.setup();
  route("POST /api/v1/auth/setup", () =>
    failure(403, "forbidden", "cross-origin requests are not allowed"),
  );
  renderAt("/setup?token=tok-123", "/setup", <SetupPage />);
  await fillSetup(user);
  await user.click(screen.getByRole("button", { name: "Create account" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Set DOMOVOI_WEB__PUBLIC_URL to this page's origin",
  );
});

test("login_page_says_the_credentials_were_wrong_and_clears_the_password", async () => {
  const user = userEvent.setup();
  route("POST /api/v1/auth/login", () =>
    failure(401, "bad_credentials", "wrong username or password"),
  );
  renderAt("/login", "/login", <LoginPage />);

  await user.type(screen.getByLabelText("Username"), "nobody");
  await user.type(screen.getByLabelText("Password"), "whatever it is");
  await user.click(screen.getByRole("button", { name: "Log in" }));
  const alert = await screen.findByRole("alert");
  expect(alert).toHaveTextContent("Could not log in");
  expect(alert).toHaveTextContent("Wrong username or password.");
  expect(screen.getByLabelText("Password")).toHaveValue("");

  route("POST /api/v1/auth/login", () => SESSION);
  await user.type(screen.getByLabelText("Password"), "correct horse battery");
  await user.click(screen.getByRole("button", { name: "Log in" }));
  expect(await screen.findByTestId("location")).toHaveTextContent("/incidents");
});

test("login_page_counts_down_a_rate_limit_and_asks_for_credentials_first", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  try {
    const user = userEvent.setup({ advanceTimers: (ms) => void vi.advanceTimersByTime(ms) });
    route("POST /api/v1/auth/login", () =>
      failure(429, "too_many_requests", "wait", { "Retry-After": "120" }),
    );
    renderAt("/login", "/login", <LoginPage />);
    expect(screen.getByRole("button", { name: "Log in" })).toHaveAccessibleDescription(
      "Enter your username and password first.",
    );

    await user.type(screen.getByLabelText("Username"), "admin");
    await user.type(screen.getByLabelText("Password"), "again");
    await user.click(screen.getByRole("button", { name: "Log in" }));
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Could not log in");
    expect(alert).toHaveTextContent(/Too many attempts\. Try again in (119|120) s\./);

    act(() => void vi.advanceTimersByTime(30_000));
    expect(screen.getByRole("alert")).toHaveTextContent(/Try again in (89|90|91) s\./);
  } finally {
    vi.useRealTimers();
  }
});
