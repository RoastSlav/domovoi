import { useState } from "react";
import { useDigestPreview, useSendDigest } from "../../api/hooks/system";
import { ApiFailure } from "../../api/types";
import { Button } from "../../components/Button";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { EmptyState } from "../../components/EmptyState";
import { Markdown } from "../../components/Markdown";
import ui from "../../components/ui.module.css";
import styles from "./digest.module.css";

function sendFailureText(error: unknown): string {
  if (error instanceof ApiFailure && error.status === 503) {
    return "No chat is configured to send to.";
  }
  return "Could not send the digest. Try again.";
}

/** Today's digest as it will be sent, and sending it now. */
export function DigestPage() {
  const preview = useDigestPreview();
  const send = useSendDigest();
  const [confirming, setConfirming] = useState(false);

  return (
    <>
      <div className={ui.pageHead}>
        <h1>Digest</h1>
        <Button
          variant="primary"
          onClick={() => setConfirming(true)}
          disabled={!preview.data || send.isPending}
        >
          Send now
        </Button>
      </div>
      {send.isSuccess && <p role="status">Sent.</p>}
      {send.error && (
        <p className={styles.error} role="alert">
          {sendFailureText(send.error)}
        </p>
      )}
      {preview.isPending && <p>Loading…</p>}
      {preview.error && <EmptyState title="Could not build the digest" />}
      {preview.data && (
        <article className={styles.preview} aria-label={`Digest for ${preview.data.date}`}>
          <p className={styles.date}>For {preview.data.date}</p>
          <Markdown text={preview.data.markdown} />
        </article>
      )}
      {confirming && (
        <ConfirmDialog
          title="Send the digest now?"
          confirmLabel="Send"
          busy={send.isPending}
          onConfirm={() => send.mutate(undefined, { onSettled: () => setConfirming(false) })}
          onCancel={() => setConfirming(false)}
        >
          <p>It goes to the configured chat now, as well as at its usual time.</p>
        </ConfirmDialog>
      )}
    </>
  );
}
