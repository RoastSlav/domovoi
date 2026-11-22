import type { OperationTrace } from "../../../api/operationTrace";
import { executionStateMeta, operationStatusMeta, stepKindMeta } from "../../../design/tone";
import type { HistoryEvent } from "../../../api/types";

/** One moment of a replay: a whole second in which something was recorded. */
export interface Frame {
  /** The whole second, in UTC milliseconds. */
  atMs: number;
  events: readonly HistoryEvent[];
  /** The highest journal sequence number in this second. */
  seq: number;
}

/** Where a replay stands: at a recorded moment, or at the live state. */
export type ReplayPosition = { kind: "frame"; atMs: number } | { kind: "live" };

const MAX_PARTS = 3;

/** An ISO time floored to a whole second in UTC milliseconds; null when it does not parse. */
export function floorSecond(iso: string | null | undefined): number | null {
  const ms = iso ? Date.parse(iso) : Number.NaN;
  return Number.isNaN(ms) ? null : Math.floor(ms / 1000) * 1000;
}

/**
 * Whole-second moments in ascending order. An event whose time does not parse is ignored;
 * events inside a frame are in sequence order.
 */
export function buildFrames(events: readonly HistoryEvent[]): Frame[] {
  const bySecond = new Map<number, HistoryEvent[]>();
  for (const event of events) {
    const atMs = floorSecond(event.at);
    if (atMs !== null) {
      const bucket = bySecond.get(atMs);
      if (bucket) {
        bucket.push(event);
      } else {
        bySecond.set(atMs, [event]);
      }
    }
  }
  return [...bySecond.entries()]
    .sort(([a], [b]) => a - b)
    .map(([atMs, bucket]) => {
      const ordered = [...bucket].sort((a, b) => a.seq - b.seq);
      return { atMs, events: ordered, seq: ordered[ordered.length - 1]?.seq ?? 0 };
    });
}

/**
 * The positions the slider walks. When the history reaches the present the last frame IS the
 * live state and is replaced by live; otherwise live is appended. No frames gives just live.
 */
export function positionsOf(frames: readonly Frame[], endsAtLive: boolean): ReplayPosition[] {
  const kept = endsAtLive ? frames.slice(0, -1) : frames;
  return [...kept.map((f): ReplayPosition => ({ kind: "frame", atMs: f.atMs })), { kind: "live" }];
}

/** One short sentence for a moment, from the words of the vocabularies; at most three parts. */
export function frameCaption(frame: Frame, final: OperationTrace): string {
  const kindOf = new Map(final.steps.map((s) => [s.id, s.kind]));
  const created = new Map<string, number>();
  const actions: string[] = [];
  const statuses: string[] = [];
  let other = 0;
  for (const event of frame.events) {
    if (event.kind === "operation.step_created") {
      const kind = event.step == null ? undefined : kindOf.get(event.step);
      const label = kind === undefined ? "step" : stepKindMeta(kind).label;
      created.set(label, (created.get(label) ?? 0) + 1);
    } else if (event.kind === "operation.execution" && event.state != null) {
      actions.push(`Action: ${executionStateMeta(event.state).label}`);
    } else if (event.kind === "operation.status" && event.state != null) {
      statuses.push(`Operation: ${operationStatusMeta(event.state).label}`);
    } else {
      other += 1;
    }
  }
  const parts = [
    ...[...created].map(([label, n]) => `${n > 1 ? `${label} ×${n}` : label} added`),
    ...actions,
    ...statuses,
    ...(other > 0 ? [`${other} other ${other === 1 ? "change" : "changes"}`] : []),
  ];
  if (parts.length === 0) {
    return "Recorded";
  }
  const shown = parts.slice(0, MAX_PARTS).join("; ");
  return parts.length > MAX_PARTS ? `${shown}; and ${parts.length - MAX_PARTS} more` : shown;
}
