import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { renderSlot } from "../../test/rightSlot";
import { RightSlotSurface, useRightSlot } from "./index";

afterEach(() => {
  vi.unstubAllGlobals();
});

function Page() {
  const slot = useRightSlot();
  return (
    <div>
      <button type="button" onClick={() => slot.open("a")}>
        Open a
      </button>
      <RightSlotSurface spec={{ id: "a", label: "Panel a", icon: "info" }}>
        <button type="button">Inside</button>
      </RightSlotSurface>
    </div>
  );
}

test("a_sheet_lifts_only_the_inert_it_set_and_leaves_an_already_inert_element_alone", async () => {
  const user = userEvent.setup();
  renderSlot(<Page />, { width: 700, wide: true });
  const main = document.querySelector("main");
  main?.setAttribute("inert", "");
  await user.click(screen.getByRole("button", { name: "Open a" }));
  await user.click(
    within(screen.getByRole("dialog", { name: "Panel a" })).getByRole("button", {
      name: "Close Panel a",
    }),
  );
  expect(main).toHaveAttribute("inert");
});
