import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { waitForHealthy } from "../../api/health";
import { useSaveSettings, type SaveRequest } from "../../api/hooks/settings";
import { keys } from "../../api/queryClient";

/** Where a save is: idle, saving, waiting for the restart, or done. */
export type ApplyPhase = "idle" | "applying" | "applied" | "saved" | "slow";

/**
 * Saves settings and, when the hub restarts to apply them, waits until it answers again
 * and refetches everything, so the page and the live stream reconnect.
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
      const back = await waitForHealthy();
      await client.invalidateQueries();
      setPhase(back ? "applied" : "slow");
    } else {
      await client.invalidateQueries({ queryKey: keys.settings });
      setPhase("saved");
    }
    return saved;
  };

  return { run, phase, pending: save.isPending || phase === "applying" };
}

/** What to tell people about `phase`. */
export function phaseText(phase: ApplyPhase): string | undefined {
  switch (phase) {
    case "applying":
      return "Applying… the hub is restarting with the new settings.";
    case "applied":
      return "Saved and applied.";
    case "saved":
      return "Saved. The new settings apply when the hub next starts.";
    case "slow":
      return "Saved, but the hub has not come back yet. Reload this page in a minute.";
    default:
      return undefined;
  }
}
