import { lazy, Suspense } from "react";
import styles from "./data.module.css";

const MarkdownView = lazy(() => import("./MarkdownView"));

/**
 * Markdown from triage or notes. Raw HTML is never rendered. The renderer is its own
 * download, fetched the first time Markdown is shown; until it arrives the text shows as
 * written, so nothing is blank and little moves.
 *
 * Use it for text written by the model or in notes. Do not use it for UI text you wrote.
 */
export function Markdown({ text }: { text: string }) {
  return (
    <div className={styles.markdown}>
      <Suspense fallback={<p className={styles.markdownPlain}>{text}</p>}>
        <MarkdownView text={text} />
      </Suspense>
    </div>
  );
}
