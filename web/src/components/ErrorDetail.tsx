import { useEffect, useState } from "react";
import { retryAfterText, type DescribedError } from "../design/messages";

/** Seconds left of `seconds`, counted down once a second. */
function useCountdown(seconds: number | undefined): number | undefined {
  const [elapsed, setElapsed] = useState(0);
  useEffect(() => {
    if (seconds === undefined) {
      return undefined;
    }
    const timer = setInterval(() => setElapsed((now) => now + 1), 1000);
    return () => clearInterval(timer);
  }, [seconds]);
  return seconds === undefined ? undefined : Math.max(0, seconds - elapsed);
}

/**
 * The detail line of a described error. A rate limit counts down live ("Try again in 28 s"),
 * and invalid input lists each problem.
 *
 * Kit messages use it; page code passes errors to `LoadError`, `InlineMessage` or a toast.
 */
export function ErrorDetail({ error }: { error: DescribedError }) {
  const left = useCountdown(error.retryAfter);
  const detail = left !== undefined && left > 0 ? retryAfterText(left) : error.detail;
  const issues = error.issues ?? [];
  if (!detail && issues.length === 0) {
    return null;
  }
  return (
    <>
      {detail}
      {issues.length > 0 && (
        <ul>
          {issues.map((issue) => (
            <li key={`${issue.path}-${issue.message}`}>
              {issue.path ? `${issue.path}: ` : ""}
              {issue.message}
            </li>
          ))}
        </ul>
      )}
    </>
  );
}
