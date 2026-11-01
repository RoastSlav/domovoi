import ReactMarkdown, { type Components } from "react-markdown";
import styles from "./ui.module.css";

const components: Components = {
  a: ({ href, children }) => (
    <a href={href} rel="noreferrer noopener" target="_blank">
      {children}
    </a>
  ),
};

/**
 * Markdown from triage or notes. Raw HTML is never rendered (no `rehype-raw`), and
 * react-markdown drops unsafe link protocols.
 */
export function Markdown({ text }: { text: string }) {
  return (
    <div className={styles.markdown}>
      <ReactMarkdown skipHtml components={components}>
        {text}
      </ReactMarkdown>
    </div>
  );
}
