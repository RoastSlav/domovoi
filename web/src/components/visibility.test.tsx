import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactNode } from "react";
import { PROFILE_IDS, type ProfileId } from "../design/visibility";
import { ProfileProvider } from "../test/visibility";
import {
  DetailsScope,
  Disclosure,
  Visible,
  VisibilityPage,
  useVisibility,
  visibilityAttrs,
} from ".";

function Probe({ category }: { category: "P0" | "P1" | "P2" | "P3" }) {
  const v = useVisibility(category);
  return <output data-testid={category}>{v.presentation}</output>;
}

const at = (profile: ProfileId, children: ReactNode, pages?: Record<string, ProfileId>) =>
  render(
    <ProfileProvider profile={profile} pages={pages ?? {}}>
      {children}
    </ProfileProvider>,
  );

test("use_visibility_without_a_provider_answers_detailed", () => {
  render(
    <>
      <Probe category="P0" />
      <Probe category="P3" />
    </>,
  );
  expect(screen.getByTestId("P0")).toHaveTextContent("shown");
  expect(screen.getByTestId("P3")).toHaveTextContent("shown");
});

test("use_visibility_follows_the_provider_and_a_page_override", () => {
  at(
    "essential",
    <>
      <VisibilityPage page="operation">
        <output data-testid="in">
          <Probe category="P3" />
        </output>
      </VisibilityPage>
      <VisibilityPage page="other">
        <output data-testid="out">
          <Probe category="P3" />
        </output>
      </VisibilityPage>
    </>,
    { operation: "detailed" },
  );
  expect(screen.getByTestId("in")).toHaveTextContent("shown");
  expect(screen.getByTestId("out")).toHaveTextContent("hidden");
});

test("visible_shows_its_children_in_a_marked_wrapper_when_shown", () => {
  at(
    "detailed",
    <Visible category="P2" id="demo.block" label="Block">
      <p>inner</p>
    </Visible>,
  );
  const wrapper = screen.getByText("inner").parentElement as HTMLElement;
  expect(wrapper).toHaveAttribute("data-visibility", "P2");
  expect(wrapper).toHaveAttribute("data-visibility-id", "demo.block");
  expect(wrapper).toHaveAttribute("data-presentation", "shown");
  expect(wrapper.className).toMatch(/contents/);
});

test("visible_collapsed_shows_a_toggle_with_label_and_count_and_does_not_mount_children", async () => {
  const user = userEvent.setup();
  const mounted = vi.fn();
  function Child() {
    mounted();
    return <p>technical</p>;
  }
  at(
    "standard",
    <Visible category="P2" id="demo.block" label="Intent" count={4}>
      <Child />
    </Visible>,
  );
  const toggle = screen.getByRole("button", { name: /Intent/ });
  expect(toggle).toHaveAttribute("aria-expanded", "false");
  expect(toggle).toHaveTextContent("4");
  expect(mounted).not.toHaveBeenCalled();
  await user.click(toggle);
  expect(toggle).toHaveAttribute("aria-expanded", "true");
  expect(screen.getByText("technical")).toBeInTheDocument();
});

test("visible_reveal_opens_a_collapsed_block_at_mount_and_when_it_turns_true", () => {
  const view = (reveal: boolean) => (
    <ProfileProvider profile="standard">
      <Visible category="P2" id="demo.block" label="Run" reveal={reveal}>
        <p>inside</p>
      </Visible>
    </ProfileProvider>
  );
  const first = render(view(true));
  expect(screen.getByText("inside")).toBeInTheDocument();
  first.unmount();
  const second = render(view(false));
  expect(screen.queryByText("inside")).toBeNull();
  second.rerender(view(true));
  expect(screen.getByText("inside")).toBeInTheDocument();
});

test("visible_hidden_inside_a_details_scope_renders_nothing_and_adds_to_the_plus_n_button", () => {
  at(
    "essential",
    <DetailsScope>
      <Visible category="P3" id="a.one" label="One" count={2}>
        <p>first fact</p>
      </Visible>
      <Visible category="P3" id="a.two" label="Two">
        <p>second fact</p>
      </Visible>
    </DetailsScope>,
  );
  expect(screen.getByRole("button", { name: "+3 details" })).toBeInTheDocument();
  expect(screen.queryByText("first fact")).toBeNull();
  expect(screen.queryByText("second fact")).toBeNull();
});

test("plus_details_reveals_every_hidden_block_and_hide_puts_them_back", async () => {
  const user = userEvent.setup();
  at(
    "essential",
    <DetailsScope>
      <Visible category="P3" id="a.one" label="One" count={2}>
        <p>first fact</p>
      </Visible>
      <Visible category="P3" id="a.two" label="Two">
        <p>second fact</p>
      </Visible>
    </DetailsScope>,
  );
  await user.click(screen.getByRole("button", { name: "+3 details" }));
  expect(screen.getByText("first fact")).toBeInTheDocument();
  expect(screen.getByText("second fact")).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Hide 3 details" }));
  expect(screen.queryByText("first fact")).toBeNull();
});

test("plus_one_detail_is_singular_and_a_scope_without_hidden_blocks_has_no_button", () => {
  const one = at(
    "essential",
    <DetailsScope>
      <Visible category="P3" id="a.one" label="One">
        <p>fact</p>
      </Visible>
    </DetailsScope>,
  );
  expect(screen.getByRole("button", { name: "+1 detail" })).toBeInTheDocument();
  one.unmount();
  at(
    "detailed",
    <DetailsScope>
      <Visible category="P3" id="a.one" label="One">
        <p>fact</p>
      </Visible>
    </DetailsScope>,
  );
  expect(screen.queryByRole("button")).toBeNull();
});

test("visible_hidden_without_a_details_scope_is_collapsed_not_dropped", () => {
  at(
    "essential",
    <Visible category="P3" id="a.one" label="Loose facts">
      <p>fact</p>
    </Visible>,
  );
  expect(screen.getByRole("button", { name: /Loose facts/ })).toHaveAttribute(
    "aria-expanded",
    "false",
  );
});

test("p0_and_p1_visible_ignore_every_profile", () => {
  for (const profile of PROFILE_IDS) {
    const view = at(
      profile,
      <>
        <Visible category="P0" id="a.safe">
          <p>safe</p>
        </Visible>
        <Visible category="P1" id="a.main">
          <p>main</p>
        </Visible>
      </>,
    );
    expect(screen.getByText("safe").parentElement).toHaveAttribute("data-presentation", "shown");
    expect(screen.getByText("main")).toBeInTheDocument();
    view.unmount();
  }
});

test("disclosure_li_variant_is_a_list_item_and_axe_clean_in_a_list", () => {
  render(
    <ol>
      <Disclosure as="li" label="2 technical steps" summary="Model turn ×2">
        <ol>
          <li>row</li>
        </ol>
      </Disclosure>
    </ol>,
  );
  expect(screen.getByRole("listitem")).toBeInTheDocument();
});

test("disclosure_button_has_an_accessible_name_and_controls_its_region", async () => {
  const user = userEvent.setup();
  render(
    <Disclosure label="Connections" count={2}>
      <p>links</p>
    </Disclosure>,
  );
  const button = screen.getByRole("button", { name: /Connections/ });
  await user.click(button);
  const region = screen.getByRole("group", { name: "Connections" });
  expect(button).toHaveAttribute("aria-controls", region.id);
});

test("visibility_attrs_returns_the_two_data_attributes", () => {
  expect(visibilityAttrs("P0", "demo.row")).toEqual({
    "data-visibility": "P0",
    "data-visibility-id": "demo.row",
  });
});
