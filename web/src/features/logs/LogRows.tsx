import type { LogLine } from "../../api/types";
import { levelKind, lineFields, splitMatches } from "./logLines";
import styles from "./logs.module.css";

/** `text` with the parts that match `query` marked, so a search shows where it hit. */
function Highlight({ text, query }: { query: string; text: string }) {
  return (
    <>
      {splitMatches(text, query).map((part, index) =>
        part.match ? <mark key={index}>{part.text}</mark> : part.text,
      )}
    </>
  );
}

/**
 * Log lines as rows: time, a level word toned by its kind, then the target and message in
 * monospace. Only the level word is toned, never the whole line; the word is there so colour
 * is never the only signal.
 */
export function LogRows({ lines, query }: { lines: LogLine[]; query: string }) {
  return (
    <>
      {lines.map((line, position) => {
        const [time, level, target, message] = lineFields(line);
        return (
          <div key={position} className={styles.row} data-level={line.level}>
            <span className={styles.time}>
              <Highlight text={time} query={query} />
            </span>{" "}
            <span className={styles.level} data-kind={levelKind(line.level)}>
              <Highlight text={level} query={query} />
            </span>{" "}
            <span className={styles.text}>
              <Highlight text={target} query={query} />
              {": "}
              <Highlight text={message} query={query} />
            </span>
          </div>
        );
      })}
    </>
  );
}
