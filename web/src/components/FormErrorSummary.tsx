import type { Issue } from "../api/types";
import { fieldIdForPath } from "./Field";
import { InlineMessage } from "./InlineMessage";
import styles from "./form.module.css";

/**
 * The problems with a submitted form, at its top: each one links to its field (a `Field`
 * with the same `path`) and moves focus there. Issues without a field are listed as text.
 * Renders nothing when there are none.
 *
 * Use it together with the error under each field, when a form is long enough that a
 * problem can be off screen.
 */
export function FormErrorSummary({
  issues,
  title,
}: {
  issues: readonly Pick<Issue, "message" | "path">[];
  title?: string;
}) {
  if (issues.length === 0) {
    return null;
  }
  const heading =
    title ?? (issues.length === 1 ? "1 problem to fix" : `${issues.length} problems to fix`);
  return (
    <InlineMessage tone="danger" title={heading}>
      <ul className={styles.summary}>
        {issues.map((issue) => {
          const id = issue.path ? fieldIdForPath(issue.path) : "";
          return (
            <li key={`${issue.path}-${issue.message}`}>
              {id ? (
                <a
                  href={`#${id}`}
                  onClick={(event) => {
                    const field = document.getElementById(id);
                    if (field) {
                      event.preventDefault();
                      field.focus();
                      field.scrollIntoView?.({ block: "center" });
                    }
                  }}
                >
                  {issue.message}
                </a>
              ) : (
                issue.message
              )}
            </li>
          );
        })}
      </ul>
    </InlineMessage>
  );
}
