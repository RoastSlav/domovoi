import { useQueryClient } from "@tanstack/react-query";
import { applyJournal } from "./journalFrames";
import { JOURNAL_EVENTS_URL, useJournalStream, type Live } from "./journalStream";

export {
  FIRST_RETRY_MS,
  MAX_RETRY_MS,
  PAUSED_AFTER_MS,
  retryDelay,
  type Live,
  type LiveStatus,
} from "./journalStream";

/** The hub's event stream. */
export const EVENTS_URL = JOURNAL_EVENTS_URL;

const LIVE_TOPICS = ["incidents", "approvals", "operations"] as const;

/**
 * Keeps cached incidents, actions, operations and counts fresh from the hub's journal stream,
 * reconnecting with backoff, and reports whether the stream is live.
 */
export function useLiveEvents(enabled = true): Live {
  const client = useQueryClient();
  return useJournalStream({
    topics: LIVE_TOPICS,
    enabled,
    onFrame: (frame) => void applyJournal(client, frame),
  });
}
