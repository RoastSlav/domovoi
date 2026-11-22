import { formatClock } from "./clock";

const AT = Date.UTC(2026, 0, 1, 15, 4, 5);

describe("the replay clock", () => {
  it("replay_clock_formats_in_the_viewers_locale", () => {
    expect(formatClock(AT, "en-US", "UTC")).toBe(
      new Intl.DateTimeFormat("en-US", { timeStyle: "medium", timeZone: "UTC" }).format(AT),
    );
    expect(formatClock(AT, "en-US", "UTC")).toMatch(/3:04:05\sPM/);
    expect(formatClock(AT, "de-DE", "UTC")).toBe("15:04:05");
    expect(formatClock(AT)).toBe(
      new Intl.DateTimeFormat(undefined, { timeStyle: "medium" }).format(AT),
    );
  });
});
