import { ApiFailure } from "../api/types";
import { HUB_CODES, describeError, errorText } from "./messages";

const STATUS: Record<string, number> = {
  unauthorized: 401,
  setup_required: 401,
  bad_credentials: 401,
  forbidden: 403,
  not_found: 404,
  bad_request: 400,
  invalid: 400,
  conflict: 409,
  too_many_requests: 429,
  unavailable: 503,
  triage_off: 503,
  budget_spent: 503,
  delivery_failed: 502,
  internal: 500,
};

const OTHER_FAILURES: [string, unknown][] = [
  ["network failure", new TypeError("Failed to fetch")],
  ["abort", new DOMException("The operation was aborted.", "AbortError")],
  ["timeout", new DOMException("The operation timed out.", "TimeoutError")],
  ["unknown error", new Error("boom")],
  ["not an error", "boom"],
  ["a proxy's error page", new ApiFailure(502, "unknown", "request failed")],
];

describe("describeError", () => {
  it.each(HUB_CODES)("says %s in words, with and without a context", (code) => {
    const failure = new ApiFailure(STATUS[code] ?? 500, code, "the server said this");
    const bare = describeError(failure);
    expect(bare.title.length).toBeGreaterThan(10);
    expect(bare.title, "no raw code").not.toContain(code);
    expect(bare.title).toMatch(/^[A-Z].*\.$/);

    const inContext = describeError(failure, "approve the action");
    expect(inContext.title).toBe("Could not approve the action");
    expect(inContext.detail?.length).toBeGreaterThan(10);
    expect(inContext.detail, "no raw code").not.toContain(code);
  });

  it.each(OTHER_FAILURES)("says a %s in words", (_, failure) => {
    const bare = describeError(failure);
    expect(bare.title).toMatch(/^[A-Z].*\.$/);
    expect(bare.title).not.toMatch(/boom|Failed to fetch|request failed|unknown/);
    expect(describeError(failure, "load hosts").title).toBe("Could not load hosts");
  });

  it("tells a network failure from a timeout", () => {
    expect(describeError(OTHER_FAILURES[0]?.[1]).title).toContain("did not answer");
    expect(describeError(OTHER_FAILURES[2]?.[1]).title).toContain("too long");
  });

  it("uses the hub's own message as the detail when it is specific", () => {
    const failure = new ApiFailure(409, "conflict", "the approval window closed 2 minutes ago");
    expect(describeError(failure, "approve the action")).toEqual({
      title: "Could not approve the action",
      detail: "The approval window closed 2 minutes ago.",
    });
    expect(describeError(failure)).toEqual({
      title: "That no longer fits the current state.",
      detail: "The approval window closed 2 minutes ago.",
    });
  });

  it("prefers its own words where the hub's message is generic", () => {
    const failure = new ApiFailure(404, "not_found", "not found");
    expect(describeError(failure, "load the note").detail).toBe(
      "It no longer exists. It may have been removed.",
    );
    expect(describeError(failure).detail).toBeUndefined();
  });

  it("keeps the case of a message that starts with an identifier", () => {
    const failure = new ApiFailure(400, "bad_request", "discord.channel_id must be a string");
    expect(describeError(failure, "run the test").detail).toBe(
      "discord.channel_id must be a string.",
    );
  });

  it("passes retryAfter through and says how long to wait", () => {
    const failure = new ApiFailure(429, "too_many_requests", "too many attempts", 28);
    const described = describeError(failure, "log in");
    expect(described.retryAfter).toBe(28);
    expect(described.detail).toBe("Too many attempts. Try again in 28 s.");
  });

  it("passes issues through", () => {
    const issues = [{ path: "web.port", message: "Port must be 1 to 65535.", severity: "error" }];
    const failure = new ApiFailure(400, "invalid", "the settings are not valid", undefined, issues);
    const described = describeError(failure, "save the settings");
    expect(described.issues).toEqual(issues);
    expect(described.detail).toBe("Some values are not valid. Fix them and try again.");
  });

  it("reads as one line where there is room for one", () => {
    expect(errorText(new TypeError("Failed to fetch"), "load hosts")).toBe(
      "Could not load hosts. The hub did not answer. Check the connection and try again.",
    );
    expect(errorText(new ApiFailure(404, "not_found", "not found"))).toBe(
      "It no longer exists. It may have been removed.",
    );
  });
});
