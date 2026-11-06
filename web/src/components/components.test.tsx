import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Badge } from "./Badge";
import { ConfirmDialog } from "./ConfirmDialog";
import { Markdown } from "./Markdown";
import { PageIntro } from "./PageIntro";
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

test("badges_explain_their_verdicts_in_tooltips", () => {
  render(
    <>
      <Badge value="ACTION_NEEDED" />
      <Badge value="MONITOR" />
      <Badge value="EXPECTED" />
      <Badge value="FIXED" />
      <Badge value="resolved" />
    </>,
  );
  expect(screen.getByText("Action needed")).toHaveAttribute(
    "title",
    "Something is broken or will break soon; act now.",
  );
  expect(screen.getByText("Monitor")).toHaveAttribute(
    "title",
    "Off but not urgent; keep an eye on it.",
  );
  expect(screen.getByText("Expected")).toHaveAttribute(
    "title",
    "Explained by something intended, like an update or a known condition.",
  );
  expect(screen.getByText("Fixed")).toHaveAttribute("title", "Already recovered.");
  expect(screen.getByText("Resolved")).toHaveAttribute("title", "The problem cleared.");
});

test("page_intro_shows_its_text_and_a_link_to_the_docs", () => {
  render(
    <PageIntro docs="concepts/incidents.html">Problems Domovoi found on your hosts.</PageIntro>,
  );
  expect(screen.getByText(/^Problems Domovoi found on your hosts\./)).toBeVisible();
  const link = screen.getByRole("link", { name: "Learn more" });
  expect(link).toHaveAttribute(
    "href",
    "https://RoastSlav.github.io/domovoi/concepts/incidents.html",
  );
  expect(link).toHaveAttribute("target", "_blank");
  expect(link).toHaveAttribute("rel", "noreferrer");
});

test("page_intro_with_no_docs_path_shows_no_link", () => {
  render(<PageIntro>Problems Domovoi found on your hosts.</PageIntro>);
  expect(screen.getByText("Problems Domovoi found on your hosts.")).toBeVisible();
  expect(screen.queryByRole("link")).toBeNull();
});

test("relative_time_reads_naturally", () => {
  const now = new Date("2026-10-31T12:00:00Z");
  expect(relative(new Date("2026-10-31T11:55:00Z"), now)).toBe("5 minutes ago");
  expect(relative(new Date("2026-10-31T11:59:40Z"), now)).toBe("just now");
  expect(relative(new Date("2026-10-30T12:00:00Z"), now)).toBe("yesterday");
});
