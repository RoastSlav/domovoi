import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router";
import { ConfirmDialog } from "./ConfirmDialog";
import { Markdown } from "./Markdown";
import { PageHeader } from "./PageHeader";
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

test("markdown_never_renders_raw_html", async () => {
  const { container } = render(
    <Markdown
      text={'**bold** <script>alert(1)</script><img src=x onerror="x"> [x](javascript:alert(1))'}
    />,
  );
  // The renderer is loaded on demand; the text shows as written until it arrives.
  expect((await screen.findByText("bold")).tagName).toBe("STRONG");
  expect(container.querySelector("script")).toBeNull();
  expect(container.querySelector("img")).toBeNull();
  expect(screen.getByText("x").getAttribute("href") ?? "").not.toContain("javascript");
});

test("page_header_shows_its_title_intro_and_a_link_to_the_docs", () => {
  render(
    <MemoryRouter>
      <PageHeader
        title="Incidents"
        docs="concepts/incidents.html"
        intro="Problems Domovoi found on your hosts."
      />
    </MemoryRouter>,
  );
  expect(screen.getByRole("heading", { level: 1, name: "Incidents" })).toBeVisible();
  expect(screen.getByText(/^Problems Domovoi found on your hosts\./)).toBeVisible();
  const link = screen.getByRole("link", {
    name: "Learn more about incidents (opens in a new tab)",
  });
  expect(link).toHaveAttribute(
    "href",
    "https://RoastSlav.github.io/domovoi/concepts/incidents.html",
  );
  expect(link).toHaveAttribute("target", "_blank");
  expect(link).toHaveAttribute("rel", "noreferrer");
});

test("page_header_with_no_docs_path_shows_no_link", () => {
  render(<PageHeader title="Incidents" intro="Problems Domovoi found on your hosts." />);
  expect(screen.getByText("Problems Domovoi found on your hosts.")).toBeVisible();
  expect(screen.queryByRole("link")).toBeNull();
});

test("relative_time_reads_naturally", () => {
  const now = new Date("2026-10-31T12:00:00Z");
  expect(relative(new Date("2026-10-31T11:55:00Z"), now)).toBe("5 minutes ago");
  expect(relative(new Date("2026-10-31T11:59:40Z"), now)).toBe("just now");
  expect(relative(new Date("2026-10-30T12:00:00Z"), now)).toBe("yesterday");
});
