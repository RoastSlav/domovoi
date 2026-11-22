/** How an edge line is drawn: the pattern differs by kind, never colour alone. */
export interface EdgeStyle {
  /** The `stroke-dasharray` attribute value in px; undefined is a solid line. */
  dash: string | undefined;
  /** Stroke width in px. */
  weight: number;
}

const STYLES: Readonly<Record<string, EdgeStyle>> = {
  depends_on: { dash: undefined, weight: 1.5 },
  caused_by: { dash: "6 4", weight: 1.5 },
  verifies: { dash: "2 3", weight: 2 },
  retry_of: { dash: "8 3 2 3", weight: 2 },
  answers: { dash: "12 4", weight: 1.5 },
};

/** The line style of an edge kind; an unknown kind is a thin solid line. */
export function edgeStyle(kind: string): EdgeStyle {
  return Object.hasOwn(STYLES, kind) ? (STYLES[kind] as EdgeStyle) : { dash: undefined, weight: 1 };
}
