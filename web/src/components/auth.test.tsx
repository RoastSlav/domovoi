import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { AuthCard, Field, PasswordInput, Stepper } from ".";

function Form() {
  return (
    <Field label="Password">
      <PasswordInput defaultValue="hunter2 hunter2" />
    </Field>
  );
}

test("password_input_toggle_shows_and_hides_what_was_typed", async () => {
  const user = userEvent.setup();
  render(<Form />);
  const input = screen.getByLabelText("Password");
  expect(input).toHaveAttribute("type", "password");

  await user.click(screen.getByRole("button", { name: "Show password" }));
  expect(input).toHaveAttribute("type", "text");
  expect(screen.queryByRole("button", { name: "Show password" })).toBeNull();

  await user.click(screen.getByRole("button", { name: "Hide password" }));
  expect(input).toHaveAttribute("type", "password");
});

test("password_input_says_when_caps_lock_is_on_and_stops_when_it_is_off", () => {
  render(<Form />);
  const input = screen.getByLabelText("Password");
  expect(screen.queryByText("Caps Lock is on")).toBeNull();

  fireEvent.keyDown(input, { key: "A", modifierCapsLock: true });
  expect(screen.getByRole("status")).toHaveTextContent("Caps Lock is on");

  fireEvent.keyUp(input, { key: "CapsLock", modifierCapsLock: false });
  expect(screen.queryByText("Caps Lock is on")).toBeNull();

  fireEvent.keyDown(input, { key: "A", modifierCapsLock: true });
  fireEvent.blur(input);
  expect(screen.queryByText("Caps Lock is on")).toBeNull();
});

test("stepper_marks_the_current_step_and_says_each_state_in_words", () => {
  render(
    <Stepper
      label="Setup steps"
      steps={[
        { id: "a", label: "Hub", state: "done" },
        { id: "b", label: "Discord", state: "skipped" },
        { id: "c", label: "Model", state: "current" },
        { id: "d", label: "First host", state: "upcoming" },
      ]}
    />,
  );
  const list = screen.getByRole("list", { name: "Setup steps" });
  const items = within(list).getAllByRole("listitem");
  expect(items).toHaveLength(4);
  expect(items[2]).toHaveAttribute("aria-current", "step");
  expect(items[0]).not.toHaveAttribute("aria-current");
  expect(items[0]).toHaveTextContent("Hub (done)");
  expect(items[1]).toHaveTextContent("Discord (skipped)");
  expect(items[2]).toHaveTextContent("Model (current step)");
  expect(items[3]).toHaveTextContent("First host (not started)");
});

test("auth_card_has_one_heading_and_names_the_page", () => {
  render(
    <AuthCard title="Log in" footer="Need help?">
      <p>Fields</p>
    </AuthCard>,
  );
  expect(screen.getByRole("heading", { level: 1, name: "Log in" })).toBeVisible();
  expect(document.title).toBe("Log in · Domovoi");
  expect(screen.getByText("Need help?")).toBeVisible();
});
