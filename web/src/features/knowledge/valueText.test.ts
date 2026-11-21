import type { SubjectNames } from "./subjects";
import { valueText } from "./valueText";

const names: SubjectNames = {
  hosts: new Map([["host-nas", "nas"]]),
  entities: new Map([["abc", "Home VPN"]]),
};

describe("valueText", () => {
  it("value_text_renders_each_predicate_and_never_throws", () => {
    expect(valueText("management_path", { cidr: "192.168.1.0/24" }, names)).toBe("192.168.1.0/24");
    expect(valueText("trusted_cidr", { cidr: "10.0.0.0/8" }, names)).toBe("10.0.0.0/8");
    expect(
      valueText(
        "maintenance_window",
        { starts_at: "2026-11-20T02:00:00Z", ends_at: "2026-11-20T04:00:00Z", note: "updates" },
        names,
      ),
    ).toBe("2026-11-20 02:00 to 2026-11-20 04:00 UTC (updates)");
    expect(
      valueText(
        "maintenance_window",
        { starts_at: "2026-11-20T02:00:00Z", ends_at: "2026-11-20T04:00:00Z" },
        names,
      ),
    ).toBe("2026-11-20 02:00 to 2026-11-20 04:00 UTC");
    expect(
      valueText(
        "host_platform",
        {
          hostname: "nas",
          os: "Debian 12",
          kernel: "6.1.0",
          arch: "x86_64",
          cpu_count: 4,
          mem_total_bytes: Math.round(15.6 * 1024 ** 3),
        },
        names,
      ),
    ).toBe("nas · Debian 12 · 6.1.0 · x86_64 · 4 CPUs · 15.6 GiB");
    expect(valueText("never_restart", true, names)).toBe("Never restart");
    expect(valueText("depends_on", "container:host-nas/plex", names)).toBe("plex on nas");
    expect(valueText("role", "media server", names)).toBe("media server");
    expect(valueText("role", null, names)).toBe("");
    expect(valueText("role", undefined, names)).toBe("");
  });

  it("an unknown shape is compact JSON cut at 400 characters", () => {
    expect(valueText("role", { a: 1 }, names)).toBe('{"a":1}');
    expect(valueText("management_path", { nope: true }, names)).toBe('{"nope":true}');
    expect(valueText("host_platform", { hostname: "nas" }, names)).toBe('{"hostname":"nas"}');
    const long = valueText("role", { text: "x".repeat(1000) }, names);
    expect(long).toHaveLength(401);
    expect(long.endsWith("…")).toBe(true);
    const cyclic: Record<string, unknown> = {};
    cyclic.self = cyclic;
    expect(() => valueText("role", cyclic, names)).not.toThrow();
    expect(() => valueText("role", 10n, names)).not.toThrow();
  });
});
