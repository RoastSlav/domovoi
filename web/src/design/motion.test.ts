import { afterEach, describe, expect, it, vi } from "vitest";
import { MAX_TRANSITION_MS, motionMs, parseDuration } from "./motion";

afterEach(() => vi.restoreAllMocks());

function stubTokens(values: () => Record<string, string>): void {
  vi.spyOn(window, "getComputedStyle").mockImplementation(
    () => ({ getPropertyValue: (name: string) => values()[name] ?? "" }) as CSSStyleDeclaration,
  );
}

describe("motion tokens", () => {
  it("parse_duration_reads_ms_and_seconds", () => {
    expect(parseDuration("240ms")).toBe(240);
    expect(parseDuration("0.24s")).toBe(240);
    expect(parseDuration("0ms")).toBe(0);
    expect(parseDuration(" 240ms ")).toBe(240);
    for (const bad of ["", "auto", "-5ms", "NaN", "240"]) {
      expect(parseDuration(bad)).toBe(0);
    }
    expect(parseDuration("900ms")).toBe(MAX_TRANSITION_MS);
    expect(MAX_TRANSITION_MS).toBe(600);
  });

  it("motion_ms_reads_the_token_now_and_is_zero_when_missing", () => {
    let layout = "320ms";
    stubTokens(() => ({ "--motion-layout": layout }));
    expect(motionMs("layout")).toBe(320);
    layout = "0ms";
    expect(motionMs("layout")).toBe(0);
    expect(motionMs("graph")).toBe(0);
  });
});
