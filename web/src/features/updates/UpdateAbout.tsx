import { useCheckUpdate, useUpdate } from "../../api/hooks/updates";
import { ApiFailure } from "../../api/types";
import { Button } from "../../components/Button";
import { RelativeTime } from "../../components/RelativeTime";
import styles from "./updates.module.css";

/** The hub's version, when it last looked for a release, and a button that looks now. */
export function UpdateAbout() {
  const update = useUpdate();
  const check = useCheckUpdate();
  const info = update.data;
  return (
    <div className={styles.about}>
      <p>
        Version <strong>{info?.current ?? "unknown"}</strong>
        {info?.newer && info.latest && ` · Domovoi ${info.latest} is available`}
      </p>
      <p>Last checked: {info?.checked_at ? <RelativeTime at={info.checked_at} /> : "never"}</p>
      <p>
        <Button onClick={() => check.mutate()} disabled={check.isPending}>
          {check.isPending ? "Checking…" : "Check now"}
        </Button>
      </p>
      {check.error && (
        <p className={styles.error} role="alert">
          {check.error instanceof ApiFailure
            ? check.error.message
            : "Could not check for updates. Try again later."}
        </p>
      )}
    </div>
  );
}
