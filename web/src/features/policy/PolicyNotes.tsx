import { InlineMessage, StatusPill } from "../../components";
import { ruleMarkMeta } from "../../design/tone";

/**
 * The hub's notes about a policy, each shown word for word.
 * @param notes The note texts.
 */
export function PolicyNotes({ notes }: { notes: readonly string[] }) {
  return (
    <>
      {notes.map((note) => (
        <InlineMessage key={note} tone="info">
          {note}
        </InlineMessage>
      ))}
    </>
  );
}

/** A small mark for a setting the hub records but does not enforce. */
export function NotEnforced() {
  return <StatusPill size="sm" meta={ruleMarkMeta("not_enforced")} />;
}
