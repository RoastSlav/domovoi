/** The fixed words of the replay bar. */
export const REPLAY_TEXT = {
  group: "Replay",
  open: "Replay",
  close: "Close replay",
  previous: "Previous moment",
  next: "Next moment",
  play: "Play",
  pause: "Pause",
  speed: "Speed",
  live: "Back to live",
  scrubber: "Replay position",
  liveLine: "Live",
  nothing: "Nothing to replay yet.",
  loadWhat: "load the history",
  atFirst: "Already at the first moment.",
  atLive: "Already showing the live state.",
  reduced:
    "Automatic replay is off because your system asks for reduced motion. Use Previous moment and Next moment.",
  pruned:
    "The oldest records of this operation were removed, so the replay starts later than the operation.",
  capped:
    "This operation has more records than a replay can hold, so it stops before the latest changes. Back to live shows the current state.",
  noteBody:
    "The graph shows this operation as it was. Everything else on this page, including the step list below, is live.",
} as const;

/** The replay note's title for a moment. */
export const noteTitle = (clock: string): string => `Replay: showing ${clock}`;

/** What the scrubber says at a moment: its number among the recorded moments, and its time. */
export const momentText = (index: number, count: number, clock: string): string =>
  `Moment ${index + 1} of ${count}, ${clock}`;

/** The status line at a moment. */
export const momentLine = (index: number, count: number, clock: string, caption: string): string =>
  `Moment ${index + 1} of ${count}: ${clock}. ${caption}`;

/** The inspector's note while a replay shows the graph at another time. */
export const inspectorNote = (label: string): string =>
  `The graph shows ${label}. This panel shows the step as it is now.`;
