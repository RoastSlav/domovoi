import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Badge } from "./Badge";
import { ConfirmDialog } from "./ConfirmDialog";
import { Markdown } from "./Markdown";
import { relative } from "./RelativeTime";

test("confirm_dialog_traps_focus_and_closes_on_escape", async () => {
  const user = userEvent.setup();
  const onCancel = vi.fn();
  render(
    <>
      <button type="button">outside</button>
      <ConfirmDialog title="Sure?" confirmLabel="Yes" onConfirm={vi.fn()} onCancel={onCancel}>
        <input aria-label="note" />
      </ConfirmDialog>
    </>,
  );
  expect(screen.getByRole("dialog", { name: "Sure?" })).toHaveAttribute("aria-modal", "true");
  expect(screen.getByLabelText("note")).toHaveFocus();

  await user.tab();
  expect(screen.getByRole("button", { name: "Cancel" })).toHaveFocus();
  await user.tab();
  expect(screen.getByRole("button", { name: "Yes" })).toHaveFocus();
  await user.tab();
  expect(screen.getByLabelText("note"), "wraps to the first").toHaveFocus();
  await user.tab({ shift: true });
  expect(screen.getByRole("button", { name: "Yes" }), "and back to the last").toHaveFocus();

  await user.keyboard("{Escape}");
  expect(onCancel).toHaveBeenCalledOnce();
});

test("markdown_never_renders_raw_html", () => {
  const { container } = render(
    <Markdown
      text={'**bold** <script>alert(1)</script><img src=x onerror="x"> [x](javascript:alert(1))'}
    />,
  );
  expect(screen.getByText("bold").tagName).toBe("STRONG");
  expect(container.querySelector("script")).toBeNull();
  expect(container.querySelector("img")).toBeNull();
  expect(screen.getByText("x").getAttribute("href") ?? "").not.toContain("javascript");
});

test("badges_carry_text_not_only_colour", () => {
  render(
    <>
      <Badge value="ACTION_NEEDED" />
      <Badge value="critical" />
    </>,
  );
  expect(screen.getByText("Action needed")).toHaveAttribute("data-tone", "action-needed");
  expect(screen.getByText("Critical")).toHaveAttribute("data-tone", "critical");
});

test("relative_time_reads_naturally", () => {
  const now = new Date("2026-10-31T12:00:00Z");
  expect(relative(new Date("2026-10-31T11:55:00Z"), now)).toBe("5 minutes ago");
  expect(relative(new Date("2026-10-31T11:59:40Z"), now)).toBe("just now");
  expect(relative(new Date("2026-10-30T12:00:00Z"), now)).toBe("yesterday");
});
