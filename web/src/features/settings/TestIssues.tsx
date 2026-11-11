import { InlineError } from "../../components";

/**
 * A failed connection test as the UI shows it: what failed and why, with each returned
 * issue on its own line naming the settings field it is about. Used by both the settings
 * section test button and the first-run wizard.
 */
export function TestIssues({ error }: { error: unknown }) {
  return <InlineError error={error} context="run the test" />;
}
