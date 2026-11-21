import type { DescribedError } from "../../design/messages";
import { ErrorDetail, InlineMessage } from "../../components";

/** A failed action in the catalogue's words, inside the dialog or form it came from. */
export function FailureMessage({ error }: { error: DescribedError | undefined }) {
  if (!error) {
    return null;
  }
  return (
    <InlineMessage tone="danger" title={error.title}>
      <ErrorDetail error={error} />
    </InlineMessage>
  );
}
