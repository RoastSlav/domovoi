import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { TONES } from "../../design/tone";
import { readPalette, toneVars, usePalette } from "./palette";

/** Stubs `getComputedStyle` so every token reads as `<theme>:<name>`. */
function stubTheme(theme: () => string): void {
  vi.spyOn(window, "getComputedStyle").mockImplementation(
    () => ({ getPropertyValue: (name: string) => ` ${theme()}:${name} ` }) as CSSStyleDeclaration,
  );
}

afterEach(() => {
  vi.restoreAllMocks();
  document.documentElement.removeAttribute("data-theme");
});

describe("the graph palette", () => {
  it("palette_reads_every_token_and_follows_the_theme", async () => {
    let theme = "light";
    stubTheme(() => theme);
    const light = readPalette(document.documentElement);
    expect(light.surface).toBe("light:--color-surface");
    expect(light.accent).toBe("light:--color-accent");
    expect(light.focus).toBe("light:--color-focus");
    for (const tone of TONES) {
      expect(light.tones[tone]).toEqual({
        bg: `light:--tone-${tone}-bg`,
        fg: `light:--tone-${tone}-fg`,
        border: `light:--tone-${tone}-border`,
        solid: `light:--tone-${tone}-solid`,
      });
    }
    const { result } = renderHook(() => usePalette());
    expect(result.current.text).toBe("light:--color-text");
    theme = "dark";
    await act(async () => {
      document.documentElement.setAttribute("data-theme", "dark");
      await Promise.resolve();
    });
    expect(result.current.text).toBe("dark:--color-text");
  });

  it("tone_vars_names_the_five_custom_properties_of_a_tone", () => {
    expect(toneVars("danger")).toEqual({
      bg: "--tone-danger-bg",
      fg: "--tone-danger-fg",
      border: "--tone-danger-border",
      solid: "--tone-danger-solid",
      onSolid: "--tone-danger-on-solid",
    });
  });
});
