import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { Scrubber } from ".";
import css from "./scrubber.module.css?raw";
import { expectNoA11yViolations } from "../test/axe";

function Harness({
  start = 2,
  max = 20,
  onValue,
}: {
  max?: number;
  onValue?: (v: number) => void;
  start?: number;
}) {
  const [value, setValue] = useState(start);
  return (
    <Scrubber
      label="Replay position"
      min={0}
      max={max}
      value={value}
      valueText={`Moment ${value + 1} of ${max + 1}`}
      onChange={(next) => {
        setValue(next);
        onValue?.(next);
      }}
    />
  );
}

test("scrubber_exposes_label_value_and_value_text", () => {
  render(<Harness />);
  const slider = screen.getByRole("slider", { name: "Replay position" });
  expect(slider).toHaveAttribute("type", "range");
  expect(slider).toHaveAttribute("min", "0");
  expect(slider).toHaveAttribute("max", "20");
  expect(slider).toHaveValue("2");
  expect(slider).toHaveAttribute("aria-valuetext", "Moment 3 of 21");
});

test("scrubber_calls_onChange_with_a_number", () => {
  const seen: unknown[] = [];
  render(<Harness onValue={(v) => seen.push(v)} />);
  fireEvent.change(screen.getByRole("slider"), { target: { value: "7" } });
  expect(seen).toEqual([7]);
});

test("scrubber_moves_with_arrow_keys_home_and_end", async () => {
  const user = userEvent.setup();
  render(<Harness />);
  const slider = screen.getByRole("slider");
  slider.focus();
  await user.keyboard("{ArrowRight}");
  expect(slider).toHaveValue("3");
  await user.keyboard("{ArrowLeft}{ArrowLeft}");
  expect(slider).toHaveValue("1");
  await user.keyboard("{PageUp}");
  expect(slider, "page is a tenth of the range").toHaveValue("3");
  await user.keyboard("{PageDown}{PageDown}");
  expect(slider).toHaveValue("0");
  await user.keyboard("{End}");
  expect(slider).toHaveValue("20");
  await user.keyboard("{ArrowRight}");
  expect(slider, "stays inside the range").toHaveValue("20");
  await user.keyboard("{Home}");
  expect(slider).toHaveValue("0");
});

test("scrubber_disabled_shows_its_reason", () => {
  render(
    <Scrubber
      label="Replay position"
      min={0}
      max={0}
      value={0}
      valueText="Live"
      onChange={() => undefined}
      disabled
      disabledReason="Nothing to replay yet."
    />,
  );
  const slider = screen.getByRole("slider", { name: "Replay position" });
  expect(slider).toBeDisabled();
  expect(screen.getByText("Nothing to replay yet.")).toBeVisible();
  expect(slider).toHaveAccessibleDescription("Nothing to replay yet.");
});

test("scrubber_styles_use_tokens_only", () => {
  expect(css).not.toMatch(/#[0-9a-f]{3,8}\b|rgb\(|hsl\(/i);
  expect(css).toContain("var(--color-accent)");
  expect(css).toContain("var(--shadow-focus)");
});

test("scrubber_has_no_axe_violations", async () => {
  const { container } = render(<Harness />);
  await expectNoA11yViolations(container);
});
