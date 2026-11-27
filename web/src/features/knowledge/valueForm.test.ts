import { buildValue, isNetwork, type ValueDraft } from "./valueForm";

const draft = (over: Partial<ValueDraft>): ValueDraft => ({
  predicate: "role",
  text: "",
  cidr: "",
  criticality: "",
  startsAt: "",
  endsAt: "",
  note: "",
  dependsOn: "",
  ...over,
});

describe("buildValue", () => {
  it("build_value_validates_each_predicate_and_sends_the_cidr_as_typed", () => {
    expect(buildValue(draft({ text: "  media server " }))).toEqual({
      ok: true,
      value: "media server",
    });
    expect(buildValue(draft({ text: "  " }))).toEqual({
      ok: false,
      problem: "Write 1 to 80 characters.",
    });
    expect(buildValue(draft({ text: "x".repeat(81) })).ok).toBe(false);
    expect(buildValue(draft({ text: "x".repeat(80) })).ok).toBe(true);
    expect(buildValue(draft({ predicate: "owner_note", text: "x".repeat(1000) })).ok).toBe(true);
    expect(buildValue(draft({ predicate: "owner_note", text: "x".repeat(1001) }))).toEqual({
      ok: false,
      problem: "Write 1 to 1000 characters.",
    });
    expect(buildValue(draft({ predicate: "backup_location", text: "x".repeat(201) })).ok).toBe(
      false,
    );
    expect(buildValue(draft({ predicate: "backup_location", text: "s3://bucket" }))).toEqual({
      ok: true,
      value: "s3://bucket",
    });
    expect(buildValue(draft({ predicate: "criticality", criticality: "critical" }))).toEqual({
      ok: true,
      value: "critical",
    });
    expect(buildValue(draft({ predicate: "criticality", criticality: "high" })).ok).toBe(false);
    expect(buildValue(draft({ predicate: "never_restart" }))).toEqual({ ok: true, value: true });
    expect(buildValue(draft({ predicate: "depends_on", dependsOn: "entity:abc" }))).toEqual({
      ok: true,
      value: "entity:abc",
    });
    expect(buildValue(draft({ predicate: "depends_on" })).ok).toBe(false);
    // The cidr goes as typed: the hub canonicalises it.
    expect(buildValue(draft({ predicate: "management_path", cidr: " 192.168.1.7/24 " }))).toEqual({
      ok: true,
      value: { cidr: "192.168.1.7/24" },
    });
    expect(buildValue(draft({ predicate: "trusted_cidr", cidr: "" })).ok).toBe(false);
    expect(buildValue(draft({ predicate: "host_platform" })).ok).toBe(false);
    expect(buildValue(draft({ predicate: "from_a_newer_hub", text: "x" })).ok).toBe(false);
  });

  it("says that a value is not a network before anything is sent", () => {
    for (const cidr of ["not a network", "10.0.0.0/33", "010.0.0.1", "10.0.0/8", "fd00::/129"]) {
      expect(buildValue(draft({ predicate: "management_path", cidr })), cidr).toEqual({
        ok: false,
        problem: "That is not a network. Write one such as 192.168.1.0/24 or fd00::/64.",
      });
    }
    // What the hub reads: bare addresses, compressed and mapped IPv6.
    for (const cidr of [
      "192.168.1.20",
      "10.0.0.0/8",
      "fd00::/64",
      "fd00::20",
      "2001:DB8:0:0:0:0:0:0/32",
      "::ffff:192.168.1.20",
      "1:0:0:2:0:0:0:3",
    ]) {
      expect(isNetwork(cidr), cidr).toBe(true);
    }
    for (const cidr of ["::ffff:10.0.0.7/64", "1::2::3", "1:2:3:4:5:6:7:8::", "10.0.0.0/8/1"]) {
      expect(isNetwork(cidr), cidr).toBe(false);
    }
  });

  it("a maintenance window is read as UTC and must end after it starts", () => {
    const window = {
      predicate: "maintenance_window",
      startsAt: "2026-11-20T02:00",
      endsAt: "2026-11-20T04:00",
    };
    expect(buildValue(draft(window))).toEqual({
      ok: true,
      value: { starts_at: "2026-11-20T02:00:00Z", ends_at: "2026-11-20T04:00:00Z" },
    });
    expect(buildValue(draft({ ...window, note: " updates " }))).toEqual({
      ok: true,
      value: {
        starts_at: "2026-11-20T02:00:00Z",
        ends_at: "2026-11-20T04:00:00Z",
        note: "updates",
      },
    });
    expect(buildValue(draft({ ...window, endsAt: "2026-11-20T02:00" }))).toEqual({
      ok: false,
      problem: "The end must be after the start.",
    });
    expect(buildValue(draft({ ...window, startsAt: "" })).ok).toBe(false);
    expect(buildValue(draft({ ...window, note: "x".repeat(201) })).ok).toBe(false);
  });

  it("refuses_a_network_that_is_every_address", () => {
    for (const cidr of ["0.0.0.0/0", "::/0"]) {
      expect(buildValue(draft({ predicate: "management_path", cidr })).ok).toBe(false);
    }
  });
});
