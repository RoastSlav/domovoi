import ReactMarkdown, { type Components } from "react-markdown";

const components: Components = {
  a: ({ href, children }) => (
    <a href={href} rel="noreferrer noopener" target="_blank">
      {children}
    </a>
  ),
};

/**
 * The Markdown renderer itself, loaded on demand by `Markdown` so the library stays out of
 * the first download. Raw HTML is never rendered (no `rehype-raw`), and react-markdown drops
 * unsafe link protocols.
 */
export default function MarkdownView({ text }: { text: string }) {
  return (
    <ReactMarkdown skipHtml components={components}>
      {text}
    </ReactMarkdown>
  );
}
