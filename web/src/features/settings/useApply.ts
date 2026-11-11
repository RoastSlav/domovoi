import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { waitForHealthy } from "../../api/health";
import { useSaveSettings, type SaveRequest } from "../../api/hooks/settings";
import { keys } from "../../api/queryClient";

/** Where a save is: idle, waiting for the restart, back online, saved for later, or slow. */
export type ApplyPhase = "idle" | "applying" | "applied" | "saved" | "slow";

/**
 * Saves settings and, when the hub restarts to apply them, waits until it answers again.
 * "Back online" is only said once the session query succeeds again, because that is the
 * first request that needs the whole hub. It then refetches what a restart changes:
 * settings, hosts, the release check and the navigation counts.
 */
export function useApplySettings() {
  const save = useSaveSettings();
  const client = useQueryClient();
  const [phase, setPhase] = useState<ApplyPhase>("idle");

  const run = async (request: SaveRequest) => {
    setPhase("idle");
    const saved = await save.mutateAsync(request);
    if (saved.applied) {
      setPhase("applying");
      const healthy = await waitForHealthy();
      let back = healthy;
      try {
        // Only a session query that is on screen is refetched; the settings page watches it.
        await client.refetchQueries({ queryKey: keys.session }, { throwOnError: true });
      } catch {
        back = false;
      }
      // A restart changes what the hub is and who is connected to it, nothing else.
      await Promise.all(
        [keys.settings, keys.hosts, keys.update, keys.overview].map((queryKey) =>
          client.invalidateQueries({ queryKey }),
        ),
      );
      setPhase(back ? "applied" : "slow");
    } else {
      await client.invalidateQueries({ queryKey: keys.settings });
      setPhase("saved");
    }
    return saved;
  };

  return {
    run,
    phase,
    pending: save.isPending || phase === "applying",
    /** Forgets the outcome, once the person has read it. */
    dismiss: () => setPhase("idle"),
  };
}

/** What to tell people about `phase`. */
export function phaseText(phase: ApplyPhase): string | undefined {
  switch (phase) {
    case "applying":
      return "Restarting the hub…";
    case "applied":
      return "Back online. The new settings are in use.";
    case "saved":
      return "Saved. The new settings apply when the hub next starts.";
    case "slow":
      return "Saved, but the hub has not come back yet. Reload this page in a minute.";
    default:
      return undefined;
  }
}
