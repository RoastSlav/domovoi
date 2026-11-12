import { defaultOf, describe, tidyDescription } from "./form";

const schema = {
  properties: {
    notify: { $ref: "#/$defs/Notify" },
    incidents: { $ref: "#/$defs/Incidents" },
  },
  $defs: {
    Notify: {
      properties: {
        quiet_start: {
          description: 'When quiet hours begin, such as `"00:00"`.\nUses the hub\'s time zone.',
          default: "",
        },
        digest: { $ref: "#/$defs/Digest" },
      },
    },
    Digest: {
      properties: { time: { type: "string", default: "09:00" }, enabled: { default: true } },
    },
    Incidents: {
      properties: {
        confirm_runs: { type: "integer", default: 2, description: "Runs before an incident." },
        reopen: { anyOf: [{ type: "string", default: "6h" }, { type: "null" }], default: null },
        tags: { default: ["a"] },
      },
    },
  },
};

test("describe_strips_inline_code_backticks_and_joins_lines", () => {
  expect(describe(schema, "notify.quiet_start")).toBe(
    `When quiet hours begin, such as "00:00". Uses the hub's time zone.`,
  );
});

test("tidy_description_keeps_link_text_and_drops_code_marks", () => {
  expect(tidyDescription("See [`Foo`] and [the guide](https://x.test/y) for `bar`.")).toBe(
    "See Foo and the guide for bar.",
  );
});

test("default_of_reads_a_string_or_number_default_through_refs", () => {
  expect(defaultOf(schema, "incidents.confirm_runs")).toBe("2");
  expect(defaultOf(schema, "notify.digest.time")).toBe("09:00");
});

test("default_of_skips_switches_lists_empty_strings_and_unknown_paths", () => {
  expect(defaultOf(schema, "notify.digest.enabled")).toBeUndefined();
  expect(defaultOf(schema, "incidents.tags")).toBeUndefined();
  expect(defaultOf(schema, "notify.quiet_start")).toBeUndefined();
  expect(defaultOf(schema, "nope.nothing")).toBeUndefined();
  expect(defaultOf(undefined, "incidents.confirm_runs")).toBeUndefined();
});
