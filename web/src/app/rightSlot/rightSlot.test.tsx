import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { InfoTip } from "../../components";
import { renderSlot } from "../../test/rightSlot";
import { RightSlotSurface, useRightSlot } from "./index";

afterEach(() => {
  vi.unstubAllGlobals();
});

/** A page with a surface per id and a button that opens each. */
function Page({
  ids = ["a"],
  onClose,
  focus,
}: {
  ids?: string[];
  onClose?: (() => void) | undefined;
  focus?: boolean | undefined;
}) {
  const slot = useRightSlot();
  const [mounted, setMounted] = useState(true);
  return (
    <div>
      {ids.map((id) => (
        <button key={id} type="button" onClick={() => slot.open(id, { focus })}>
          Open {id}
        </button>
      ))}
      <button type="button" onClick={() => setMounted(false)}>
        Remove owner
      </button>
      {mounted &&
        ids.map((id) => (
          <RightSlotSurface
            key={id}
            spec={{ id, label: `Panel ${id}`, icon: "info" }}
            onClose={onClose}
          >
            <p>Content of {id}</p>
            <InfoTip>A hint inside {id}</InfoTip>
          </RightSlotSurface>
        ))}
    </div>
  );
}

const panel = (id: string) => screen.getByRole("complementary", { name: `Panel ${id}` });

test("opening_a_surface_renders_it_beside_the_page_and_moves_focus_in", async () => {
  const user = userEvent.setup();
  renderSlot(<Page />);
  expect(screen.queryByText("Content of a")).toBeNull();

  await user.click(screen.getByRole("button", { name: "Open a" }));

  expect(within(panel("a")).getByText("Content of a")).toBeVisible();
  expect(screen.getByRole("heading", { name: "Panel a" })).toHaveFocus();
});

test("opening_with_focus_false_leaves_focus_alone", async () => {
  const user = userEvent.setup();
  renderSlot(<Page focus={false} />);
  const opener = screen.getByRole("button", { name: "Open a" });

  await user.click(opener);

  expect(await screen.findByText("Content of a")).toBeVisible();
  expect(opener).toHaveFocus();
});

test("closing_returns_focus_to_the_opener_and_calls_onclose", async () => {
  const user = userEvent.setup();
  const onClose = vi.fn();
  renderSlot(<Page onClose={onClose} />);
  const opener = screen.getByRole("button", { name: "Open a" });
  await user.click(opener);

  await user.click(screen.getByRole("button", { name: "Close Panel a" }));

  expect(onClose).toHaveBeenCalledTimes(1);
  expect(screen.queryByText("Content of a")).toBeNull();
  expect(opener).toHaveFocus();
});

test("unmounting_an_open_surface_removes_it_without_calling_onclose", async () => {
  const user = userEvent.setup();
  const onClose = vi.fn();
  renderSlot(<Page onClose={onClose} />);
  await user.click(screen.getByRole("button", { name: "Open a" }));

  await user.click(screen.getByRole("button", { name: "Remove owner" }));

  expect(screen.queryByText("Content of a")).toBeNull();
  expect(onClose).not.toHaveBeenCalled();
});

test("escape_closes_the_surface_with_focus_after_a_popover_has_taken_its_own_escape", async () => {
  const user = userEvent.setup();
  renderSlot(<Page />);
  await user.click(screen.getByRole("button", { name: "Open a" }));
  await user.click(within(panel("a")).getByRole("button", { name: "More information" }));
  expect(screen.getByRole("dialog", { name: "More information" })).toBeVisible();

  await user.keyboard("{Escape}");
  expect(screen.queryByRole("dialog", { name: "More information" })).toBeNull();
  expect(screen.getByText("Content of a")).toBeVisible();

  await user.keyboard("{Escape}");
  expect(screen.queryByText("Content of a")).toBeNull();
});

test("escape_with_focus_on_the_page_does_nothing", async () => {
  const user = userEvent.setup();
  renderSlot(<Page />);
  await user.click(screen.getByRole("button", { name: "Open a" }));
  screen.getByRole("button", { name: "Open a" }).focus();

  await user.keyboard("{Escape}");

  expect(screen.getByText("Content of a")).toBeVisible();
});

test("a_second_surface_that_does_not_fit_collapses_the_first_to_a_tab", async () => {
  const user = userEvent.setup();
  renderSlot(<Page ids={["a", "b"]} />, { width: 1100, wide: true });
  await user.click(screen.getByRole("button", { name: "Open a" }));
  await user.click(screen.getByRole("button", { name: "Open b" }));

  expect(screen.getByText("Content of b")).toBeVisible();
  expect(screen.queryByText("Content of a")).toBeNull();
  const tabs = screen.getByRole("group", { name: "Side panels" });
  expect(within(tabs).getAllByRole("radio")).toHaveLength(2);

  await user.click(within(tabs).getByRole("radio", { name: "Panel a" }));
  expect(screen.getByText("Content of a")).toBeVisible();
  expect(screen.queryByText("Content of b")).toBeNull();
});

test("a_narrow_shell_shows_a_modal_sheet_and_makes_the_page_inert", async () => {
  const user = userEvent.setup();
  renderSlot(<Page />, { width: 700, wide: true });
  await user.click(screen.getByRole("button", { name: "Open a" }));

  const sheet = screen.getByRole("dialog", { name: "Panel a" });
  expect(sheet).toHaveAttribute("aria-modal", "true");
  expect(document.querySelector("main")).toHaveAttribute("inert");

  await user.click(within(sheet).getByRole("button", { name: "Close Panel a" }));
  expect(document.querySelector("main")).not.toHaveAttribute("inert");
});

test("the_page_keeps_its_content_when_the_slot_is_open_side_by_side", async () => {
  const user = userEvent.setup();
  renderSlot(<Page />, { width: 1300, wide: true });
  await user.click(screen.getByRole("button", { name: "Open a" }));

  const body = screen.getByTestId("body");
  expect(Array.from(body.children).map((child) => child.tagName)).toEqual(["MAIN", "ASIDE"]);
  expect(document.querySelector("main")).not.toHaveAttribute("inert");
});

function FocusButtons() {
  const slot = useRightSlot();
  return (
    <>
      <button type="button" onClick={() => slot.focus("a")}>
        Focus a
      </button>
      <button type="button" onClick={() => slot.focus("missing")}>
        Focus missing
      </button>
    </>
  );
}

test("slot_focus_moves_focus_into_an_open_surface", async () => {
  const user = userEvent.setup();
  renderSlot(
    <>
      <Page focus={false} />
      <FocusButtons />
    </>,
  );
  await user.click(screen.getByRole("button", { name: "Open a" }));
  await screen.findByText("Content of a");

  await user.click(screen.getByRole("button", { name: "Focus a" }));

  expect(screen.getByRole("heading", { name: "Panel a" })).toHaveFocus();
});

test("slot_focus_is_a_no_op_for_a_closed_or_unregistered_surface", async () => {
  const user = userEvent.setup();
  renderSlot(
    <>
      <Page />
      <FocusButtons />
    </>,
  );
  const focusButton = screen.getByRole("button", { name: "Focus a" });

  await user.click(focusButton);
  expect(focusButton).toHaveFocus();
  await user.click(screen.getByRole("button", { name: "Focus missing" }));
  expect(screen.getByRole("button", { name: "Focus missing" })).toHaveFocus();
});
