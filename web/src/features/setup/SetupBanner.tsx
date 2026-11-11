import { useState } from "react";
import { useHosts } from "../../api/hooks/hosts";
import { useSettings } from "../../api/hooks/system";
import { Banner, LinkButton } from "../../components";
import { getIn } from "../settings/form";

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
    <Banner
      tone="info"
      title="Setup is not finished"
      onDismiss={dismiss}
      action={
        <LinkButton to="/welcome" size="sm">
          Finish setup
        </LinkButton>
      }
    >
      Still to add: {missing.join(", ")}.
    </Banner>
  );
}
