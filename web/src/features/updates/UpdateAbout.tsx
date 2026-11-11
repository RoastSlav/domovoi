import { useCheckUpdate, useUpdate } from "../../api/hooks/updates";
import { Button, RelativeTime, useActionFeedback } from "../../components";
import styles from "./updates.module.css";

/** The hub's version, when it last looked for a release, and a button that looks now. */
export function UpdateAbout() {
  const update = useUpdate();
  const check = useActionFeedback(useCheckUpdate(), {
    success: (found) =>
      found.newer && found.latest
        ? `Domovoi ${found.latest} is available`
        : { title: "You are on the latest version", tone: "info" },
    error: "check for updates",
  });
  const info = update.data;
  return (
    <div className={styles.about}>
      <p>
        Version <strong>{info?.current ?? "unknown"}</strong>
        {info?.newer && info.latest && ` · Domovoi ${info.latest} is available`}
      </p>
      <p>Last checked: {info?.checked_at ? <RelativeTime at={info.checked_at} /> : "never"}</p>
      <p>
        <Button icon="refresh" onClick={() => check.mutate()} loading={check.isPending}>
          Check now
        </Button>
      </p>
    </div>
  );
}
