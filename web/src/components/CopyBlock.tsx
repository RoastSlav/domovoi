import { useRef, useState } from "react";
import { Button } from "./Button";
import styles from "./ui.module.css";

/** Text to copy exactly, such as a command, with a Copy button. */
export function CopyBlock({ text, label }: { text: string; label: string }) {
  const pre = useRef<HTMLPreElement>(null);
  const [copied, setCopied] = useState(false);

  const selectText = () => {
    const node = pre.current;
    const selection = window.getSelection();
    if (node && selection) {
      selection.selectAllChildren(node);
    }
  };

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
    } catch {
      // No clipboard access (an insecure origin, or denied): select it for a manual copy.
      selectText();
    }
  };

  return (
    <div className={styles.copy}>
      <pre ref={pre} aria-label={label} tabIndex={0}>
        <code>{text}</code>
      </pre>
      <Button onClick={() => void copy()}>{copied ? "Copied" : "Copy"}</Button>
    </div>
  );
}
