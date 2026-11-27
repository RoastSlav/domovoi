import { screen } from "@testing-library/react";
import { expect, test } from "vitest";
import type { RetiredFact } from "../../api/types";
import { renderAt } from "../../test/render";
import { NoLongerReported } from "./NoLongerReported";

const gone: RetiredFact = {
  fact: {
    key: "disk./old.free_gib",
    label: "Free on /old",
    value: "12",
    unit: "GiB",
    observed_at: "2026-11-27T10:00:00Z",
  },
  retired_at: "2026-11-27T11:00:00Z",
};

test("facts_a_check_stopped_reporting_are_grouped_and_never_shown_as_current", () => {
  renderAt("/hosts/nas", "/hosts/:name", <NoLongerReported facts={[gone]} />);
  expect(screen.getByText("No longer reported (1)")).toBeVisible();
});

test("nothing_is_shown_when_every_fact_is_still_reported", () => {
  renderAt("/hosts/nas", "/hosts/:name", <NoLongerReported facts={[]} />);
  expect(screen.queryByText(/No longer reported/)).toBeNull();
});
