import { useState } from "react";
import { Link } from "react-router";
import { useHosts } from "../../api/hooks/hosts";
import { useSettings } from "../../api/hooks/system";
import { getIn } from "../settings/form";
import styles from "../settings/settings.module.css";

const DISMISSED = "domovoi.setupBannerDismissed";

function wasDismissed(): boolean {
  try {
    return localStorage.getItem(DISMISSED) === "1";
  } catch {
    return false;
  }
}

/**
 * Lists the first-run steps that are still open (Discord, a model, a host) until they are
 * done or the banner is dismissed. Shown only where settings are managed in the UI.
 */
export function SetupBanner() {
  const settings = useSettings();
  const hosts = useHosts();
  const [dismissed, setDismissed] = useState(wasDismissed);
  const editable = settings.data?.editable;
  if (dismissed || !editable || !hosts.data) {
    return null;
  }
  const values = editable.values as Record<string, unknown>;
  const missing = [
    !getIn(values, "discord") && "Discord",
    !getIn(values, "llm.primary") && "a model",
    !hosts.data.some((h) => h.kind === "remote") && "a host",
  ].filter((item): item is string => Boolean(item));
  if (missing.length === 0) {
    return null;
  }
  const dismiss = () => {
    try {
      localStorage.setItem(DISMISSED, "1");
    } catch {
      // Private mode: the banner just comes back next time.
    }
    setDismissed(true);
  };
  return (
    <aside className={styles.banner} aria-label="Setup incomplete">
      Setup incomplete: add {missing.join(", ")}. <Link to="/welcome">Finish setup</Link>{" "}
      <button type="button" className={styles.dismiss} onClick={dismiss}>
        Dismiss
      </button>
    </aside>
  );
}
