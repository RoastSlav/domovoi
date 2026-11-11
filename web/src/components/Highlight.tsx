import styles from "./highlight.module.css";

function escape(word: string): string {
  return word.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/**
 * Text with every word of a search query marked, so a person sees why a result matched.
 * Matching ignores case and treats the query's characters literally.
 *
 * Use it for titles and excerpts in search results. Do not use it for status or emphasis
 * that is not a search match.
 */
export function Highlight({ text, query }: { query: string; text: string }) {
  const words = [...new Set(query.split(/\s+/).filter(Boolean))].sort(
    (a, b) => b.length - a.length,
  );
  if (words.length === 0) {
    return <>{text}</>;
  }
  const pattern = new RegExp(`(${words.map(escape).join("|")})`, "gi");
  // Splitting on a capture group puts every match at an odd index.
  const parts = text.split(pattern);
  return (
    <>
      {parts.map((part, index) =>
        index % 2 === 1 ? (
          <mark key={index} className={styles.mark}>
            {part}
          </mark>
        ) : (
          part
        ),
      )}
    </>
  );
}
