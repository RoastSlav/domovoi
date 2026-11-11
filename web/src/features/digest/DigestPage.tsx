import { useState } from "react";
import { useDigestPreview, useSendDigest } from "../../api/hooks/system";
import {
  Button,
  ConfirmDialog,
  LoadError,
  Loading,
  Markdown,
  PageHeader,
  useActionFeedback,
} from "../../components";
import styles from "./digest.module.css";

/** Today's digest as it will be sent, and sending it now. */
export function DigestPage() {
  const preview = useDigestPreview();
  const send = useActionFeedback(useSendDigest(), {
    success: "Digest sent",
    error: "send the digest",
  });
  const [confirming, setConfirming] = useState(false);

  return (
    <>
      <PageHeader
        title="Digest"
        topic="the digest"
        docs="concepts/digest.html"
        intro="One summary of every host per day: what changed, trends and open incidents."
        actions={
          <Button
            variant="primary"
            onClick={() => setConfirming(true)}
            loading={send.isPending}
            disabledReason={preview.data ? undefined : "The digest has not loaded yet."}
          >
            Send now
          </Button>
        }
      />
      {preview.isPending && <Loading />}
      {preview.error && !preview.data && (
        <LoadError error={preview.error} context="build the digest" onRetry={preview.refetch} />
      )}
      {preview.data && (
        <article className={styles.preview} aria-label={`Digest for ${preview.data.date}`}>
          <p className={styles.date}>For {preview.data.date}</p>
          <Markdown text={preview.data.markdown} />
        </article>
      )}
      {confirming && (
        <ConfirmDialog
          title="Send the digest now?"
          consequence="It goes to the configured chat now, as well as at its usual time."
          confirmLabel="Send digest"
          busy={send.isPending}
          onConfirm={() => {
            // The result arrives as a toast, which needs the dialog out of the way.
            setConfirming(false);
            send.mutate(undefined);
          }}
          onCancel={() => setConfirming(false)}
        />
      )}
    </>
  );
}
