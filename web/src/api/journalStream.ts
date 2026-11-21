import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import {
  parseJournalFrame,
  parseReadyFrame,
  parseResetFrame,
  refetchLive,
  type JournalFrame,
} from "./journalFrames";

/** The hub's journal-backed event stream. */
export const JOURNAL_EVENTS_URL = "/api/v1/events";

/** Whether the event stream is connected. */
export type LiveStatus = "live" | "connecting" | "offline";

/** The first reconnect delay, doubled after each failure. */
export const FIRST_RETRY_MS = 1_000;

/** How long the stream may be down before the UI says live updates are paused. */
export const PAUSED_AFTER_MS = 5_000;

/** The longest reconnect delay. */
export const MAX_RETRY_MS = 30_000;

/** The state of the event stream, and whether it has been down long enough to say so. */
export interface Live {
  /** True once the stream has been down for {@link PAUSED_AFTER_MS}, until it is back. */
  paused: boolean;
  status: LiveStatus;
}

/** What {@link useJournalStream} subscribes to and who it tells. */
export interface JournalStreamOptions {
  /** The topics to subscribe to; a change reconnects. */
  topics: readonly string[];
  /** False keeps the stream closed. */
  enabled?: boolean;
  /** Called once per journal row, in order, never for a row at or below the last seen `seq`. */
  onFrame: (frame: JournalFrame) => void;
  /**
   * Called for the ephemeral `live.start`, `live.text`, `live.tool` and `live.end` events with
   * their parsed JSON. They carry no id: they never move the resume cursor and are never
   * deduplicated or replayed.
   */
  onLive?: (name: string, data: unknown) => void;
  /** Called when the client lost its place and refetched its snapshots. */
  onReset?: () => void;
}

/** The ephemeral event names the hub sends beside journal rows. */
export const LIVE_EVENTS = ["live.start", "live.text", "live.tool", "live.end"] as const;

/** The delay before reconnect attempt `attempt`, counting from 0. */
export function retryDelay(attempt: number): number {
  return Math.min(FIRST_RETRY_MS * 2 ** attempt, MAX_RETRY_MS);
}

/**
 * Follows the hub's journal over one EventSource, resuming from the last seen `seq` after a
 * reconnect with backoff, and reports whether the stream is live.
 */
export function useJournalStream(options: JournalStreamOptions): Live {
  const client = useQueryClient();
  const { enabled = true } = options;
  const topicsKey = options.topics.join(",");
  const [status, setStatus] = useState<LiveStatus>("connecting");
  const [paused, setPaused] = useState(false);
  const latest = useRef(options);
  const lastId = useRef<number | undefined>(undefined);
  const lastTopics = useRef(topicsKey);

  useEffect(() => {
    latest.current = options;
  });

  useEffect(() => {
    if (!enabled) {
      return undefined;
    }
    if (lastTopics.current !== topicsKey) {
      // A cursor belongs to one topic set.
      lastTopics.current = topicsKey;
      lastId.current = undefined;
    }
    let source: EventSource | undefined;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let pauseTimer: ReturnType<typeof setTimeout> | undefined;
    let attempt = 0;
    let closed = false;

    const markLive = () => {
      attempt = 0;
      clearTimeout(pauseTimer);
      pauseTimer = undefined;
      setPaused(false);
      setStatus("live");
    };

    const resync = (head: number) => {
      lastId.current = head;
      void refetchLive(client, head);
      latest.current.onReset?.();
    };

    const connect = () => {
      setStatus("connecting");
      const after = lastId.current === undefined ? "" : `&after=${lastId.current}`;
      source = new EventSource(`${JOURNAL_EVENTS_URL}?topics=${topicsKey}${after}`, {
        withCredentials: true,
      });
      source.addEventListener("open", markLive);
      source.addEventListener("ready", (message) => {
        markLive();
        const ready = parseReadyFrame((message as MessageEvent).data);
        if (ready && !ready.resumed) {
          resync(ready.head_seq);
        }
      });
      source.addEventListener("journal", (message) => {
        const frame = parseJournalFrame((message as MessageEvent).data);
        if (!frame || (lastId.current !== undefined && frame.seq <= lastId.current)) {
          return;
        }
        lastId.current = frame.seq;
        latest.current.onFrame(frame);
      });
      for (const name of LIVE_EVENTS) {
        source.addEventListener(name, (message) => {
          try {
            latest.current.onLive?.(name, JSON.parse((message as MessageEvent).data as string));
          } catch {
            // A malformed live frame is dropped; the saved answer repairs the thread.
          }
        });
      }
      source.addEventListener("reset", (message) => {
        const reset = parseResetFrame((message as MessageEvent).data);
        if (reset) {
          resync(reset.head_seq);
        }
      });
      source.addEventListener("error", () => {
        source?.close();
        if (closed) {
          return;
        }
        setStatus("offline");
        // One timer per outage: failed reconnects in between do not restart it.
        pauseTimer ??= setTimeout(() => setPaused(true), PAUSED_AFTER_MS);
        timer = setTimeout(connect, retryDelay(attempt));
        attempt += 1;
      });
    };

    connect();
    return () => {
      closed = true;
      clearTimeout(timer);
      clearTimeout(pauseTimer);
      source?.close();
    };
  }, [client, enabled, topicsKey]);

  return { status, paused };
}
