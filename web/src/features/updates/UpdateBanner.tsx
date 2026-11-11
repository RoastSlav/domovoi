import { useState } from "react";
import { useUpdate } from "../../api/hooks/updates";
import { Button, Dialog, Markdown } from "../../components";
import { HowToUpdateDialog } from "./HowToUpdateDialog";
import styles from "./updates.module.css";

const DISMISSED_PREFIX = "update-dismissed:";

function wasDismissed(latest: string): boolean {
  try {
    return localStorage.getItem(DISMISSED_PREFIX + latest) === "1";
  } catch {
    return false;
  }
}

function rememberDismissed(latest: string): void {
  try {
    localStorage.setItem(DISMISSED_PREFIX + latest, "1");
  } catch {
    // Private mode: the banner just comes back next time.
  }
}

type DialogKind = "notes" | "how" | null;

/** Announces a newer release and how to install it, until this version is dismissed. */
export function UpdateBanner() {
  const update = useUpdate();
  const [closed, setClosed] = useState<string | null>(null);
  const [dialog, setDialog] = useState<DialogKind>(null);
  const info = update.data;
  const latest = info?.latest;
  if (!info?.newer || !latest || closed === latest || wasDismissed(latest)) {
    return null;
  }

  const close = () => setDialog(null);
  const dismiss = () => {
    rememberDismissed(latest);
    setClosed(latest);
  };

  return (
    <>
      <aside className={styles.banner} aria-label="Update available">
        <span>
          <strong>Domovoi {latest}</strong> is available (this hub runs {info.current}).
        </span>{" "}
        <button type="button" className={styles.link} onClick={() => setDialog("notes")}>
          What&apos;s new
        </button>{" "}
        <button type="button" className={styles.link} onClick={() => setDialog("how")}>
          How to update
        </button>{" "}
        <button type="button" className={styles.dismiss} onClick={dismiss}>
          Dismiss
        </button>
      </aside>
      {dialog === "notes" && (
        <Dialog
          title={`What's new in Domovoi ${latest}`}
          actions={<Button onClick={close}>Close</Button>}
          onClose={close}
          wide
        >
          {info.notes ? <Markdown text={info.notes} /> : <p>No release notes for this version.</p>}
          {info.url && (
            <p>
              <a href={info.url} target="_blank" rel="noreferrer noopener">
                View the release on GitHub
              </a>
            </p>
          )}
        </Dialog>
      )}
      {dialog === "how" && <HowToUpdateDialog onClose={close} />}
    </>
  );
}
