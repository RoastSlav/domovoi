import { useEffect, useState } from "react";
import type { DescribedError } from "../../design/messages";

const PREFIX = "domovoi.lastRefusal.";

/** The refusal kept for one fact, or nothing when none is kept or storage is unavailable. */
function read(id: string): DescribedError | undefined {
  try {
    const text = window.localStorage.getItem(PREFIX + id);
    if (!text) {
      return undefined;
    }
    const value: unknown = JSON.parse(text);
    if (typeof value === "object" && value !== null && "title" in value) {
      const { title, detail } = value as { title: unknown; detail?: unknown };
      if (typeof title === "string") {
        return typeof detail === "string" ? { title, detail } : { title };
      }
    }
  } catch {
    // Storage can be blocked or hold something else; the page works without it.
  }
  return undefined;
}

function write(id: string, error: DescribedError | undefined) {
  try {
    if (error) {
      window.localStorage.setItem(
        PREFIX + id,
        JSON.stringify({ title: error.title, detail: error.detail }),
      );
    } else {
      window.localStorage.removeItem(PREFIX + id);
    }
  } catch {
    // Not remembering is acceptable: the refusal is still shown for this visit.
  }
}

/**
 * The last refusal of an action on one fact, kept in this browser so a reload still says why
 * the row is "Waiting for you". `current` is the live error of the action; a new one replaces
 * the kept text, and `clear` drops it after a success.
 */
export function useRememberedRefusal(
  id: string,
  current: DescribedError | undefined,
): { shown: DescribedError | undefined; clear: () => void } {
  const [, rerender] = useState(0);
  useEffect(() => {
    if (current) {
      write(id, current);
    }
  }, [id, current]);
  return {
    shown: current ?? read(id),
    clear: () => {
      write(id, undefined);
      rerender((n) => n + 1);
    },
  };
}
