import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { renderAt } from "../../test/render";
import { LoginPage } from "./LoginPage";
import { SetupPage } from "./SetupPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const SESSION = { user: { id: "u1", username: "admin" }, csrf_token: "csrf" };

beforeEach(resetApi);

test("setup_page_validates_password_length_and_submits", async () => {
  const user = userEvent.setup();
  route("POST /api/v1/auth/setup", () => SESSION);
  renderAt("/setup?token=tok-123", "/setup", <SetupPage />);
  expect(screen.queryByLabelText("Setup token"), "the link carries the token").toBeNull();

  await user.type(screen.getByLabelText("Password"), "short");
  await user.type(screen.getByLabelText("Repeat password"), "short");
  await user.click(screen.getByRole("button", { name: "Create account" }));
  expect(screen.getByRole("alert")).toHaveTextContent("at least 12 characters");
  expect(calls).toHaveLength(0);

  await user.clear(screen.getByLabelText("Password"));
  await user.type(screen.getByLabelText("Password"), "a long passphrase");
  await user.click(screen.getByRole("button", { name: "Create account" }));
  expect(screen.getByRole("alert")).toHaveTextContent("do not match");

  await user.clear(screen.getByLabelText("Repeat password"));
  await user.type(screen.getByLabelText("Repeat password"), "a long passphrase");
  await user.click(screen.getByRole("button", { name: "Create account" }));
  expect(await screen.findByTestId("location")).toHaveTextContent("/welcome");
  expect(calls).toEqual([
    {
      route: "POST /api/v1/auth/setup",
      init: { body: { token: "tok-123", username: "admin", password: "a long passphrase" } },
    },
  ]);
});

test("setup_page_explains_a_spent_link", async () => {
  const user = userEvent.setup();
  route("POST /api/v1/auth/setup", () => failure(403, "forbidden", "bad token"));
  renderAt("/setup", "/setup", <SetupPage />);
  await user.type(screen.getByLabelText("Setup token"), "old");
  await user.type(screen.getByLabelText("Password"), "a long passphrase");
  await user.type(screen.getByLabelText("Repeat password"), "a long passphrase");
  await user.click(screen.getByRole("button", { name: "Create account" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("wrong, used or expired");
});

test("login_page_shows_generic_error", async () => {
  const user = userEvent.setup();
  route("POST /api/v1/auth/login", () =>
    failure(401, "bad_credentials", "wrong username or password"),
  );
  renderAt("/login", "/login", <LoginPage />);

  await user.type(screen.getByLabelText("Username"), "nobody");
  await user.type(screen.getByLabelText("Password"), "whatever it is");
  await user.click(screen.getByRole("button", { name: "Log in" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Wrong username or password.");
  expect(screen.getByLabelText("Password")).toHaveValue("");

  route("POST /api/v1/auth/login", () =>
    failure(429, "too_many_requests", "wait", { "Retry-After": "120" }),
  );
  await user.type(screen.getByLabelText("Password"), "again");
  await user.click(screen.getByRole("button", { name: "Log in" }));
  expect(await screen.findByText("Too many attempts. Try again in 2 minutes.")).toBeVisible();

  route("POST /api/v1/auth/login", () => SESSION);
  await user.type(screen.getByLabelText("Password"), "correct horse battery");
  await user.click(screen.getByRole("button", { name: "Log in" }));
  expect(await screen.findByTestId("location")).toHaveTextContent("/incidents");
});
