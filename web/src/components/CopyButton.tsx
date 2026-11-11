import { useEffect, useState } from "react";
import { Button } from "./Button";

/** How long "Copied" stays before the button reads "Copy" again. */
export const COPIED_MS = 1500;

/**
 * Copies `text` to the clipboard and confirms it: the label becomes "Copied" for a moment
 * and the change is announced. `onUnavailable` runs when the browser gives no clipboard
 * access (an insecure origin), so the caller can select the text instead.
 *
 * Use it beside a value people paste elsewhere: a command, a token, an ID.
 */
export function CopyButton({
  text,
  label = "Copy",
  onUnavailable,
}: {
  label?: string;
  onUnavailable?: () => void;
  text: string;
}) {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) {
      return undefined;
    }
    const timer = setTimeout(() => setCopied(false), COPIED_MS);
    return () => clearTimeout(timer);
  }, [copied]);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
    } catch {
      onUnavailable?.();
    }
  };

  return (
    <Button size="sm" icon={copied ? "check" : "copy"} onClick={() => void copy()}>
      <span aria-live="polite">{copied ? "Copied" : label}</span>
    </Button>
  );
}
