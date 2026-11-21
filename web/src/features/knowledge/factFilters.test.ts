import { STATUS_CHIPS, readFactView, statusParam, writeFactView } from "./factFilters";

describe("fact filters", () => {
  it("fact_filters_round_trip_through_the_address", () => {
    expect(readFactView(new URLSearchParams())).toEqual({
      status: "live",
      source: "",
      predicate: "",
      q: "",
    });
    const none = writeFactView(new URLSearchParams("tab=proposals&host=nas"), {
      status: "live",
      source: "",
      predicate: "",
      q: "",
    });
    expect(none.toString()).toBe("tab=proposals&host=nas");

    const retracted = writeFactView(new URLSearchParams("host=nas"), { status: "retracted" });
    expect(retracted.get("status")).toBe("retracted");
    expect(retracted.get("host")).toBe("nas");
    expect(readFactView(retracted).status).toBe("retracted");

    const full = writeFactView(new URLSearchParams(), {
      source: "inferred",
      predicate: "role",
      q: "plex",
    });
    expect(readFactView(full)).toEqual({
      status: "live",
      source: "inferred",
      predicate: "role",
      q: "plex",
    });
    // Going back to the default removes the parameter.
    expect(writeFactView(full, { source: "" }).has("source")).toBe(false);

    const unknown = new URLSearchParams("status=bogus&source=nobody&predicate=nothing&q=");
    expect(readFactView(unknown)).toEqual({ status: "live", source: "", predicate: "", q: "" });
  });

  it("chips are labelled and live sends no status", () => {
    expect(STATUS_CHIPS.map((c) => c.label)).toEqual([
      "Live",
      "Current",
      "Stale",
      "Disputed",
      "Replaced",
      "Retracted",
    ]);
    expect(statusParam("live")).toBeUndefined();
    expect(statusParam("superseded")).toBe("superseded");
    expect(statusParam("retracted")).toBe("retracted");
  });
});
