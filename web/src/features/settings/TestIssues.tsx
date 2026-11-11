import { ErrorDetail, InlineMessage } from "../../components";
import { describeError } from "../../design/messages";

/**
 * A failed connection test as the UI shows it: what failed and why, with each returned
 * issue on its own line naming the settings field it is about, then what to try. Used by
 * both the settings section test button and the first-run wizard.
 */
export function TestIssues({ error, fix }: { error: unknown; fix?: string }) {
  if (!error) {
    return null;
  }
  const described = describeError(error, "run the test");
  return (
    <InlineMessage tone="danger" title={described.title}>
      <ErrorDetail error={described} />
      {fix && <p>{fix}</p>}
    </InlineMessage>
  );
}
