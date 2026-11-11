import { useAck, useMute, useUnmute } from "../../api/hooks/incidents";
import { useActionFeedback } from "../../components";

/** How long an incident can be muted for: minutes and the words for them. */
export const MUTE_OPTIONS: readonly (readonly [number, string])[] = [
  [60, "1 hour"],
  [240, "4 hours"],
  [1440, "1 day"],
  [10080, "1 week"],
];

/** How a mute of `minutes` reads in a sentence: "1 hour". */
function muteLabel(minutes: number): string {
  return MUTE_OPTIONS.find(([value]) => value === minutes)?.[1] ?? `${minutes} minutes`;
}

/**
 * Acknowledge, mute and unmute for incident `id`, each reporting its result: the same
 * words from a row's menu and from the incident's own page. Muting offers Undo.
 */
export function useIncidentActions(id: string) {
  const ack = useActionFeedback(useAck(id), {
    success: "Incident acknowledged",
    error: "acknowledge the incident",
  });
  const unmute = useActionFeedback(useUnmute(id), {
    success: "Incident unmuted",
    error: "unmute the incident",
  });
  const mute = useActionFeedback(useMute(id), {
    success: (_, minutes) => `Incident muted for ${muteLabel(minutes)}`,
    error: "mute the incident",
    undo: () => unmute.mutate(),
  });
  return { ack, mute, unmute };
}
