import { Banner, Progress } from "../../components";
import type { ApplyPhase } from "./useApply";

/**
 * What happened to the hub after a save: restarting (with progress), back online, slow to
 * return, or saved for a restart the hub cannot do itself. `changes` is how many changes
 * the last save held. Renders nothing while there is nothing to say.
 */
export function RestartBanner({
  phase,
  changes,
  onDismiss,
}: {
  phase: ApplyPhase;
  changes: number;
  onDismiss: () => void;
}) {
  switch (phase) {
    case "applying":
      return (
        <Banner tone="progress" title="Applying your settings">
          <Progress label="Restarting the hub…" />
        </Banner>
      );
    case "applied":
      return (
        <Banner tone="success" title="Back online" onDismiss={onDismiss}>
          The hub restarted and is using your new settings.
        </Banner>
      );
    case "slow":
      return (
        <Banner tone="warning" title="The hub has not come back yet" onDismiss={onDismiss}>
          Your settings are saved. Reload this page in a minute; if the hub stays away, check its
          service on the machine it runs on.
        </Banner>
      );
    case "saved":
      return (
        <Banner
          tone="warning"
          title={`Restart the hub to apply ${changes} ${changes === 1 ? "change" : "changes"}`}
          onDismiss={onDismiss}
        >
          Your settings are saved, but this hub cannot restart itself. Restart its service on the
          machine it runs on, and the new settings apply.
        </Banner>
      );
    default:
      return null;
  }
}
