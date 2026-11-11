import { useState } from "react";
import { useUpdate } from "../../api/hooks/updates";
import { Banner, Button, Dialog, Markdown } from "../../components";
import { HowToUpdateDialog } from "./HowToUpdateDialog";

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
      <Banner
        tone="warning"
        title={`Domovoi ${latest} is available`}
        onDismiss={dismiss}
        action={
          <>
            <Button size="sm" variant="ghost" onClick={() => setDialog("notes")}>
              What&apos;s new
            </Button>
            <Button size="sm" onClick={() => setDialog("how")}>
              How to update
            </Button>
          </>
        }
      >
        This hub runs {info.current}.
      </Banner>
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
