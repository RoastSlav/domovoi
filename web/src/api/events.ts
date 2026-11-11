import { useQueryClient, type QueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { keys } from "./queryClient";
import type { UiEvent } from "./types";

/** The hub's event stream. */
export const EVENTS_URL = "/api/v1/events";

/** Whether the event stream is connected. */
export type LiveStatus = "live" | "connecting" | "offline";

/** The first reconnect delay, doubled after each failure. */
export const FIRST_RETRY_MS = 1_000;

/** How long the stream may be down before the UI says live updates are paused. */
export const PAUSED_AFTER_MS = 5_000;

/** The state of the event stream, and whether it has been down long enough to say so. */
export interface Live {
  /** True once the stream has been down for {@link PAUSED_AFTER_MS}, until it is back. */
  paused: boolean;
  status: LiveStatus;
}

/** The longest reconnect delay. */
export const MAX_RETRY_MS = 30_000;

/** The delay before reconnect attempt `attempt`, counting from 0. */
export function retryDelay(attempt: number): number {
  return Math.min(FIRST_RETRY_MS * 2 ** attempt, MAX_RETRY_MS);
}

/** Invalidates the queries `event` makes stale. */
export async function applyEvent(client: QueryClient, event: UiEvent): Promise<void> {
  const stale: (readonly unknown[])[] = [
    keys.incidentLists,
    keys.incident(event.incident_id),
    keys.overview,
  ];
  if (event.kind === "action") {
    stale.push(keys.actions);
  }
  await Promise.all(stale.map((queryKey) => client.invalidateQueries({ queryKey })));
}

function parse(data: unknown): UiEvent | undefined {
  try {
    const event: unknown = JSON.parse(String(data));
    if (typeof event === "object" && event !== null && "incident_id" in event) {
      return event as UiEvent;
    }
  } catch {
    // A malformed event is ignored; the next one refreshes.
  }
  return undefined;
}

/**
 * Keeps cached incidents, actions and counts fresh from the hub's event stream, reconnecting
 * with backoff, and reports whether the stream is live.
 */
export function useLiveEvents(enabled = true): Live {
  const client = useQueryClient();
  const [status, setStatus] = useState<LiveStatus>("connecting");
  const [paused, setPaused] = useState(false);

  useEffect(() => {
    if (!enabled) {
      return undefined;
    }
    let source: EventSource | undefined;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let pauseTimer: ReturnType<typeof setTimeout> | undefined;
    let attempt = 0;
    let closed = false;

    const onEvent = (message: MessageEvent) => {
      const event = parse(message.data);
      if (event) {
        void applyEvent(client, event);
      }
    };

    const connect = () => {
      setStatus("connecting");
      source = new EventSource(EVENTS_URL, { withCredentials: true });
      source.addEventListener("open", () => {
        // Whatever changed while disconnected is unknown, so refresh it all after a gap.
        if (attempt > 0) {
          void client.invalidateQueries({ queryKey: keys.incidents });
          void client.invalidateQueries({ queryKey: keys.actions });
          void client.invalidateQueries({ queryKey: keys.overview });
        }
        attempt = 0;
        clearTimeout(pauseTimer);
        pauseTimer = undefined;
        setPaused(false);
        setStatus("live");
      });
      source.addEventListener("incident", onEvent);
      source.addEventListener("action", onEvent);
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
  }, [client, enabled]);

  return { status, paused };
}
