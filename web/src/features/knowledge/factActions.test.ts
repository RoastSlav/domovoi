import { factActions } from "./factActions";

const base = { status: "current", source: "inferred", predicate: "role", content_purged: false };

describe("factActions", () => {
  it("fact_actions_follow_the_status_source_and_predicate", () => {
    const live = ["current", "stale", "disputed"];
    for (const status of live) {
      expect(factActions({ ...base, status })).toEqual({
        accept: true,
        verify: true,
        retract: true,
        confirmElsewhere: false,
      });
    }
    for (const status of ["superseded", "retracted", "from_a_newer_hub"]) {
      expect(factActions({ ...base, status })).toEqual({
        accept: false,
        verify: false,
        retract: false,
        confirmElsewhere: false,
      });
    }
    // The owner's own fact has nothing to accept.
    expect(factActions({ ...base, source: "user" }).accept).toBe(false);
    expect(factActions({ ...base, source: "user" }).verify).toBe(true);
    // A purged fact cannot be accepted.
    expect(factActions({ ...base, content_purged: true }).accept).toBe(false);
    // A management path is never verified here; the owner's own one is confirmed elsewhere.
    const path = { ...base, predicate: "management_path" };
    expect(factActions(path).verify).toBe(false);
    expect(factActions(path).confirmElsewhere).toBe(false);
    expect(factActions({ ...path, source: "user" })).toEqual({
      accept: false,
      verify: false,
      retract: true,
      confirmElsewhere: true,
    });
    expect(factActions({ ...path, source: "user", status: "retracted" }).confirmElsewhere).toBe(
      false,
    );
  });
});
