import { useEffect, useState } from "react";
import { TONES, type Tone } from "../../design/tone";

/** The five custom properties of a tone, by purpose. */
export interface ToneVars {
  bg: string;
  fg: string;
  border: string;
  solid: string;
  onSolid: string;
}

/** Resolved colours of one tone. */
export interface TonePalette {
  bg: string;
  fg: string;
  border: string;
  solid: string;
}

/** Resolved token colours for an engine that draws on a canvas and cannot use CSS variables. */
export interface GraphPalette {
  surface: string;
  border: string;
  borderStrong: string;
  text: string;
  textMuted: string;
  accent: string;
  focus: string;
  tones: Record<Tone, TonePalette>;
}

/** The custom-property names of a tone, for engines that draw DOM or SVG. */
export function toneVars(tone: Tone): ToneVars {
  return {
    bg: `--tone-${tone}-bg`,
    fg: `--tone-${tone}-fg`,
    border: `--tone-${tone}-border`,
    solid: `--tone-${tone}-solid`,
    onSolid: `--tone-${tone}-on-solid`,
  };
}

/**
 * Reads every colour the graph uses from the tokens, as the browser computes them now. Values
 * are returned exactly as `getComputedStyle` reports them; the SVG engine never needs them
 * (it uses the variables), a canvas engine must pass them through a colour parser first.
 */
export function readPalette(root: HTMLElement): GraphPalette {
  const style = getComputedStyle(root);
  const read = (name: string): string => style.getPropertyValue(name).trim();
  const tone = (name: Tone): TonePalette => {
    const vars = toneVars(name);
    return {
      bg: read(vars.bg),
      fg: read(vars.fg),
      border: read(vars.border),
      solid: read(vars.solid),
    };
  };
  return {
    surface: read("--color-surface"),
    border: read("--color-border"),
    borderStrong: read("--color-border-strong"),
    text: read("--color-text"),
    textMuted: read("--color-text-muted"),
    accent: read("--color-accent"),
    focus: read("--color-focus"),
    tones: Object.fromEntries(TONES.map((name) => [name, tone(name)])) as Record<Tone, TonePalette>,
  };
}

/** The palette, re-read whenever the theme changes (`data-theme` on `<html>` or the system scheme). */
export function usePalette(): GraphPalette {
  const [palette, setPalette] = useState(() => readPalette(document.documentElement));
  useEffect(() => {
    const root = document.documentElement;
    const reread = (): void => setPalette(readPalette(root));
    const observer = new MutationObserver(reread);
    observer.observe(root, { attributes: true, attributeFilter: ["data-theme"] });
    const scheme =
      typeof matchMedia === "function" ? matchMedia("(prefers-color-scheme: dark)") : null;
    scheme?.addEventListener("change", reread);
    return () => {
      observer.disconnect();
      scheme?.removeEventListener("change", reread);
    };
  }, []);
  return palette;
}
