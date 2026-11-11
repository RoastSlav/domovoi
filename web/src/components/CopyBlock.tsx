import { useRef } from "react";
import { CopyButton } from "./CopyButton";
import styles from "./data.module.css";

/**
 * Text to copy exactly, such as a command, in a monospace block with a Copy button. Without
 * clipboard access the text is selected for a manual copy.
 *
 * Use it for anything pasted into a terminal. A short value inside a sentence takes a
 * `CopyButton` alone.
 */
export function CopyBlock({ text, label }: { label: string; text: string }) {
  const pre = useRef<HTMLPreElement>(null);
  const selectText = () => {
    const node = pre.current;
    const selection = window.getSelection();
    if (node && selection) {
      selection.selectAllChildren(node);
    }
  };
  return (
    <div className={styles.copy}>
      <pre ref={pre} aria-label={label} tabIndex={0}>
        <code>{text}</code>
      </pre>
      <CopyButton text={text} onUnavailable={selectText} />
    </div>
  );
}
