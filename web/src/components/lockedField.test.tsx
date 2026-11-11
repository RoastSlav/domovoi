import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Field, Select, Switch, Tag, TextInput } from ".";

test("a_locked_field_makes_its_input_read_only_and_describes_the_reason", () => {
  render(
    <Field label="Name" path="hub.name" hint="What the hub is called." locked="Set in hub.toml.">
      <TextInput defaultValue="home" />
    </Field>,
  );
  const input = screen.getByLabelText("Name");
  expect(input).toHaveAttribute("readonly");
  expect(input).toHaveAccessibleDescription("Set in hub.toml. What the hub is called.");
});

test("a_locked_select_cannot_be_changed", () => {
  render(
    <Field label="Provider" locked="Set in hub.toml.">
      <Select defaultValue="a">
        <option value="a">a</option>
      </Select>
    </Field>,
  );
  expect(screen.getByLabelText("Provider")).toBeDisabled();
});

test("an_unlocked_field_stays_editable", () => {
  render(
    <Field label="Name">
      <TextInput />
    </Field>,
  );
  expect(screen.getByLabelText("Name")).not.toHaveAttribute("readonly");
});

test("badges_sit_beside_the_label_without_becoming_part_of_it", () => {
  render(
    <Field label="Name" badges={<Tag>default</Tag>}>
      <TextInput />
    </Field>,
  );
  expect(screen.getByLabelText("Name")).toBeVisible();
  expect(screen.getByText("default")).toBeVisible();
});

test("a_tag_with_a_hint_explains_itself_on_keyboard_focus", async () => {
  const user = userEvent.setup();
  render(<Tag hint="Written in hub.toml.">from hub.toml</Tag>);
  await user.tab();
  expect(screen.getByText("from hub.toml")).toHaveFocus();
  expect(await screen.findByRole("tooltip")).toHaveTextContent("Written in hub.toml.");
});

test("a_tag_without_a_hint_is_not_a_tab_stop", async () => {
  const user = userEvent.setup();
  render(<Tag>host</Tag>);
  await user.tab();
  expect(screen.getByText("host")).not.toHaveFocus();
});

test("a_switch_takes_an_id_so_a_link_can_focus_it", () => {
  render(<Switch id="field-x" label="Run it" checked={false} onChange={() => undefined} />);
  expect(screen.getByRole("switch", { name: /Run it/ })).toHaveAttribute("id", "field-x");
});
