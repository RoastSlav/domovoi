import { useState } from "react";
import { useRemaining } from "../../components";
import { ApiFailure } from "../../api/types";

/** What a confirm dialog says while a rate limit holds it shut. */
const WAIT = "Too many attempts. Wait for the countdown to end.";

/**
 * Holds a confirm button shut while the hub's rate limit (429) runs: `lock(error)` after a
 * failed attempt, `reason` while the wait lasts. Any other failure leaves the button alone.
 */
export function useLockout(): { lock: (error: unknown) => void; reason: string | undefined } {
  const [until, setUntil] = useState(0);
  const remaining = useRemaining(new Date(until).toISOString());
  return {
    reason: remaining > 0 ? WAIT : undefined,
    lock: (error) => {
      if (error instanceof ApiFailure && error.status === 429) {
        setUntil(Date.now() + (error.retryAfter ?? 30) * 1000);
      }
    },
  };
}
