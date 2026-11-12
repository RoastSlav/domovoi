/**
 * Reads the colour tokens out of `tokens.css` and computes WCAG contrast ratios, so a test
 * can prove every shipped colour pair is readable without opening a browser.
 */

/** An sRGB colour with channels 0 to 255. */
export type Rgb = [number, number, number];

/** The two themes the stylesheet defines. */
export type Theme = "light" | "dark";

/** Every custom property of one theme, fully resolved to a colour. */
export type ResolvedTokens = Record<string, Rgb>;

function parseHex(hex: string): Rgb {
  const digits = hex.slice(1);
  const full =
    digits.length === 3
      ? digits
          .split("")
          .map((c) => c + c)
          .join("")
      : digits;
  return [
    parseInt(full.slice(0, 2), 16),
    parseInt(full.slice(2, 4), 16),
    parseInt(full.slice(4, 6), 16),
  ];
}

function declarations(block: string): Map<string, string> {
  const map = new Map<string, string>();
  for (const match of block.matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) {
    map.set(match[1] ?? "", (match[2] ?? "").trim());
  }
  return map;
}

/** The body of the first block that starts with `opener` (its braces matched). */
function blockAfter(css: string, opener: string): string {
  const start = css.indexOf(opener);
  if (start === -1) {
    return "";
  }
  let depth = 0;
  for (let i = css.indexOf("{", start); i < css.length; i += 1) {
    if (css[i] === "{") {
      depth += 1;
    } else if (css[i] === "}") {
      depth -= 1;
      if (depth === 0) {
        return css.slice(css.indexOf("{", start) + 1, i);
      }
    }
  }
  return "";
}

/** Blends `a` over `b`, `percent` of `a` (the `color-mix(in srgb, …)` the stylesheet uses). */
function mix(a: Rgb, b: Rgb, percent: number): Rgb {
  const t = percent / 100;
  return [0, 1, 2].map((i) => (a[i] ?? 0) * t + (b[i] ?? 0) * (1 - t)) as Rgb;
}

/**
 * Resolves the tokens of one theme. The dark theme is the light `:root` block with the
 * explicit dark block laid over it, as the browser does.
 */
export function resolveTokens(tokensCss: string, theme: Theme): ResolvedTokens {
  const raw = declarations(blockAfter(tokensCss, ":root {"));
  if (theme === "dark") {
    for (const [name, value] of declarations(blockAfter(tokensCss, ':root[data-theme="dark"]'))) {
      raw.set(name, value);
    }
  }
  const resolved: ResolvedTokens = {};
  const resolve = (name: string, seen: Set<string>): Rgb | undefined => {
    if (resolved[name]) {
      return resolved[name];
    }
    const value = raw.get(name);
    if (value === undefined || seen.has(name)) {
      return undefined;
    }
    seen.add(name);
    let colour: Rgb | undefined;
    const hex = /^#[0-9a-fA-F]{3,6}$/.exec(value);
    const ref = /^var\((--[\w-]+)\)$/.exec(value);
    const blend = /^color-mix\(in srgb, var\((--[\w-]+)\) (\d+)%, var\((--[\w-]+)\)\)$/.exec(value);
    if (hex) {
      colour = parseHex(value);
    } else if (ref) {
      colour = resolve(ref[1] ?? "", seen);
    } else if (blend) {
      const a = resolve(blend[1] ?? "", seen);
      const b = resolve(blend[3] ?? "", new Set());
      colour = a && b ? mix(a, b, Number(blend[2])) : undefined;
    }
    if (colour) {
      resolved[name] = colour;
    }
    return colour;
  };
  for (const name of raw.keys()) {
    resolve(name, new Set());
  }
  return resolved;
}

function channel(value: number): number {
  const c = value / 255;
  return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
}

function luminance([r, g, b]: Rgb): number {
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
}

/** The WCAG contrast ratio of two colours, 1 to 21. */
export function contrast(a: Rgb, b: Rgb): number {
  const [light, dark] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return ((light ?? 0) + 0.05) / ((dark ?? 0) + 0.05);
}

/** One pair a theme must keep readable: `fg` drawn on `bg`, at least `min` to 1. */
export interface Pair {
  fg: string;
  bg: string;
  min: number;
  why: string;
}

const TONES = ["danger", "warning", "success", "pending", "progress", "info", "neutral"] as const;
const SURFACES = [
  "--color-bg",
  "--color-surface",
  "--color-surface-raised",
  "--color-surface-overlay",
] as const;

/** Every colour pair the design system promises is readable (WCAG 2.2 AA). */
export function requiredPairs(): Pair[] {
  const pairs: Pair[] = [];
  for (const bg of SURFACES) {
    for (const fg of ["--color-text", "--color-text-muted", "--color-text-subtle"]) {
      pairs.push({ fg, bg, min: 4.5, why: "text" });
    }
    pairs.push({ fg: "--color-primary", bg, min: 4.5, why: "links and primary text" });
    pairs.push({ fg: "--color-accent", bg, min: 4.5, why: "links, selected items, accent text" });
    pairs.push({ fg: "--color-frame", bg, min: 4.5, why: "frame accents used as text" });
  }
  for (const bg of SURFACES) {
    pairs.push({ fg: "--color-focus", bg, min: 3, why: "focus ring" });
    pairs.push({ fg: "--color-border-strong", bg, min: 3, why: "control border" });
  }
  for (const fg of ["--color-text", "--color-text-muted", "--color-accent"]) {
    pairs.push({ fg, bg: "--color-accent-tint", min: 4.5, why: "selected row or tinted area" });
  }
  pairs.push({ fg: "--color-on-brand", bg: "--color-brand", min: 4.5, why: "logo mark" });
  pairs.push({ fg: "--color-on-primary", bg: "--color-primary", min: 4.5, why: "primary button" });
  pairs.push({
    fg: "--color-on-primary",
    bg: "--color-primary-hover",
    min: 4.5,
    why: "primary button on hover",
  });
  pairs.push({
    fg: "--color-sidebar-text",
    bg: "--color-sidebar-bg",
    min: 4.5,
    why: "sidebar text",
  });
  pairs.push({
    fg: "--color-sidebar-text-muted",
    bg: "--color-sidebar-bg",
    min: 4.5,
    why: "sidebar muted text",
  });
  pairs.push({
    fg: "--color-sidebar-accent",
    bg: "--color-sidebar-bg",
    min: 3,
    why: "selected navigation item",
  });
  pairs.push({
    fg: "--color-sidebar-text",
    bg: "--color-sidebar-raised",
    min: 4.5,
    why: "sidebar hover and selected row",
  });
  for (const tone of TONES) {
    const t = `--tone-${tone}`;
    pairs.push({ fg: `${t}-fg`, bg: `${t}-bg`, min: 4.5, why: "status pill text" });
    pairs.push({ fg: `${t}-fg`, bg: "--color-bg", min: 4.5, why: "status text on the page" });
    pairs.push({ fg: `${t}-fg`, bg: "--color-surface", min: 4.5, why: "status text on a card" });
    pairs.push({
      fg: `${t}-fg`,
      bg: "--color-surface-overlay",
      min: 4.5,
      why: "status text in a menu, dialog or toast",
    });
    pairs.push({ fg: `${t}-on-solid`, bg: `${t}-solid`, min: 4.5, why: "count or filled badge" });
    pairs.push({
      fg: `${t}-solid`,
      bg: "--color-surface",
      min: 3,
      why: "status dot, icon, stripe",
    });
  }
  pairs.push({
    fg: "--tone-danger-fg",
    bg: "--tone-danger-faint",
    min: 4.5,
    why: "text on a row that needs action",
  });
  pairs.push({
    fg: "--color-text",
    bg: "--tone-danger-faint",
    min: 4.5,
    why: "text on a row that needs action",
  });
  return pairs;
}
