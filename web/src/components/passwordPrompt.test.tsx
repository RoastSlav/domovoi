import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { PasswordPrompt } from "./PasswordPrompt";

test("password_prompt_shows_extra_content_and_a_custom_title_and_keeps_its_old_behaviour", async () => {
  const onConfirm = vi.fn();
  const { rerender } = render(
    <PasswordPrompt reason="This widens trust." onConfirm={onConfirm} onCancel={vi.fn()} />,
  );
  expect(screen.getByText("Confirm with your password")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Save" })).toHaveAttribute("aria-disabled", "true");
  await userEvent.type(screen.getByLabelText("Password"), "secret{Enter}");
  expect(onConfirm).toHaveBeenCalledWith("secret");

  rerender(
    <PasswordPrompt
      reason="This widens trust."
      title="Publish version 4"
      error="That password is not right."
      onConfirm={onConfirm}
      onCancel={vi.fn()}
    >
      <p>Two rules change.</p>
    </PasswordPrompt>,
  );
  expect(screen.getByText("Publish version 4")).toBeInTheDocument();
  expect(screen.queryByText("Confirm with your password")).not.toBeInTheDocument();
  expect(screen.getByText("Two rules change.")).toBeInTheDocument();
  expect(screen.getByText("That password is not right.")).toBeInTheDocument();
});
