import { useSettings } from "../../api/hooks/system";

/**
 * Why the model cannot be asked anything now, or nothing when it can. Triage being off is
 * known up front from the settings; a spent budget is only learned from a refused question,
 * which `ChatThread` handles by itself.
 */
export function useChatOff(): string | undefined {
  const settings = useSettings();
  return settings.data && !settings.data.triage.enabled
    ? "Triage is off, so there is no model to ask. Turn it on under Settings, Triage."
    : undefined;
}
